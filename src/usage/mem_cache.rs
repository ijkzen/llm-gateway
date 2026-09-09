//! 用量内存缓存（LB 选路热路径，P3）：与数据库缓存同新鲜度口径
//! （fetched_at 10 分钟，`persist::cache_age_fresh*` 同一判定），命中免一次
//! DB 往返；缓存缺失时的真实抓取按 provider 单飞去重（同一瞬间多个并发
//! 请求只发一次上游调用）。Provider 更新/删除时调用 [`UsageMemCache::invalidate`]
//! 保持与数据库缓存同失效语义。

use std::collections::HashMap;
use std::sync::Arc;

use sea_orm::DatabaseConnection;

use super::persist::{cache_age_fresh, cache_age_fresh_at, fetch_and_store};
use super::types::UsageData;

/// 并发抓取 in-flight 表：provider_id → 完成通知通道。
type InFlightMap = tokio::sync::Mutex<HashMap<i32, tokio::sync::watch::Sender<Option<UsageData>>>>;

#[derive(Clone, Default)]
pub struct UsageMemCache {
    entries: Arc<tokio::sync::Mutex<HashMap<i32, UsageData>>>,
    in_flight: Arc<InFlightMap>,
}

impl UsageMemCache {
    /// 读内存缓存（新鲜才返回）。
    pub async fn read(&self, provider_id: i32) -> Option<UsageData> {
        let entries = self.entries.lock().await;
        let data = entries.get(&provider_id)?;
        if cache_age_fresh(data.fetched_at) {
            Some(data.clone())
        } else {
            None
        }
    }

    /// 批量读（遍历条目，供选路一次性收集；同一 now 判定，同批次口径一致）。
    pub async fn read_many(&self, provider_ids: &[i32]) -> HashMap<i32, UsageData> {
        let entries = self.entries.lock().await;
        let now = chrono::Utc::now();
        provider_ids
            .iter()
            .filter_map(|id| {
                let data = entries.get(id)?;
                if cache_age_fresh_at(data.fetched_at, now) {
                    Some((*id, data.clone()))
                } else {
                    None
                }
            })
            .collect()
    }

    /// 写入内存缓存（真实抓取或数据库直出的数据回填）。
    pub async fn store(&self, data: UsageData) {
        self.entries.lock().await.insert(data.provider_id, data);
    }

    /// 失效单家（Provider 更新/删除后调用，避免旧凭据用量残留）。
    pub async fn invalidate(&self, provider_id: i32) {
        self.entries.lock().await.remove(&provider_id);
        self.in_flight.lock().await.remove(&provider_id);
    }

    /// 并发去重的真实抓取：同 provider 同时只发一次上游调用，其余请求等结果。
    /// `fetch` 为实际抓取闭包（生产走 `fetch_and_store`），可注入计数便于测试。
    pub async fn fetch_shared_with<F, Fut>(&self, provider_id: i32, fetch: F) -> Option<UsageData>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Option<UsageData>>,
    {
        let mut waiter: Option<tokio::sync::watch::Receiver<Option<UsageData>>> = None;
        let creator_tx = {
            let mut in_flight = self.in_flight.lock().await;
            match in_flight.get(&provider_id) {
                Some(tx) => {
                    waiter = Some(tx.subscribe());
                    None
                }
                None => {
                    let (tx, _rx) = tokio::sync::watch::channel(None);
                    in_flight.insert(provider_id, tx.clone());
                    Some(tx)
                }
            }
        };

        if let Some(mut rx) = waiter {
            // 等待创建者完成；创建者被取消时 guard 也会通知 None，不会悬挂。
            let _ = rx.changed().await;
            return rx.borrow().clone();
        }

        // 创建者：负责真实抓取并通知等待者；guard 保证异常路径（future 被
        // 取消）也会清理 in-flight 并通知 None，避免等待者悬挂。
        struct Cleanup<'a> {
            provider_id: i32,
            in_flight: &'a Arc<InFlightMap>,
            tx: tokio::sync::watch::Sender<Option<UsageData>>,
            sent: bool,
        }
        impl Drop for Cleanup<'_> {
            fn drop(&mut self) {
                // 尽力清理：锁竞争时放弃（等待者仍有 10 分钟新鲜度回退路径）。
                if let Ok(mut in_flight) = self.in_flight.try_lock() {
                    in_flight.remove(&self.provider_id);
                }
                if !self.sent && self.tx.borrow().is_none() {
                    let _ = self.tx.send(None);
                }
            }
        }
        let mut cleanup = Cleanup {
            provider_id,
            in_flight: &self.in_flight,
            tx: creator_tx.expect("创建者分支必有 sender"),
            sent: false,
        };
        let result = fetch().await;
        if let Some(data) = &result {
            self.store(data.clone()).await;
        }
        cleanup.sent = true;
        let _ = cleanup.tx.send(result.clone());
        result
    }

    /// 并发去重的真实抓取（生产入口：抓取并写数据库缓存，成功回填内存）。
    pub async fn fetch_shared(
        &self,
        db: &DatabaseConnection,
        provider_id: i32,
    ) -> Option<UsageData> {
        let db = db.clone();
        self.fetch_shared_with(provider_id, move || async move {
            fetch_and_store(&db, provider_id).await.ok()
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::types::{BalanceItem, UsageKind};
    use chrono::Utc;

    fn balance_data(provider_id: i32, amounts: &[f64]) -> UsageData {
        UsageData {
            provider_id,
            fetched_at: Utc::now(),
            kind: UsageKind::Balance,
            plan: None,
            windows: vec![],
            balances: amounts
                .iter()
                .enumerate()
                .map(|(i, a)| BalanceItem {
                    label: "余额".to_string(),
                    amount: *a,
                    currency: None,
                    primary: i == 0,
                })
                .collect(),
        }
    }

    #[tokio::test]
    async fn mem_cache_reads_fresh_and_expired() {
        let cache = UsageMemCache::default();
        let mut fresh = balance_data(7, &[50.0]);
        fresh.fetched_at = Utc::now();
        cache.store(fresh.clone()).await;
        assert_eq!(cache.read(7).await.map(|d| d.provider_id), Some(7));
        assert_eq!(cache.read_many(&[7, 8]).await.len(), 1, "缺失 id 不计入");

        // 过期（fetched_at 早于 TTL）按缺失处理。
        let mut stale = balance_data(7, &[50.0]);
        stale.fetched_at = Utc::now() - chrono::TimeDelta::minutes(11);
        cache.store(stale).await;
        assert!(cache.read(7).await.is_none());
    }

    #[tokio::test]
    async fn mem_cache_invalidate_removes_entry() {
        let cache = UsageMemCache::default();
        let data = balance_data(9, &[10.0]);
        cache.store(data).await;
        assert!(cache.read(9).await.is_some());
        cache.invalidate(9).await;
        assert!(cache.read(9).await.is_none());
    }

    #[tokio::test]
    async fn mem_cache_fetch_shared_deduplicates_concurrent_calls() {
        let cache = UsageMemCache::default();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));

        // 同一 provider 并发 4 路抓取：真实 fetch 闭包只应执行一次（单飞）。
        let mut handles = Vec::new();
        for _ in 0..4 {
            let cache = cache.clone();
            let calls = calls.clone();
            handles.push(tokio::spawn(async move {
                cache
                    .fetch_shared_with(42, move || {
                        let calls = calls.clone();
                        async move {
                            // 模拟真实抓取耗时：保证并发窗口重叠，单飞才可判定。
                            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
                            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                            Some(balance_data(42, &[88.0]))
                        }
                    })
                    .await
            }));
        }
        let mut results = Vec::new();
        for handle in handles {
            results.push(handle.await.unwrap());
        }
        assert!(results.iter().all(|r| r.is_some()));
        assert_eq!(
            calls.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "并发抓取应只执行一次上游调用"
        );
        // 成功结果已回填内存缓存。
        assert!(cache.read(42).await.is_some());
    }
}

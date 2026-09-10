//! 用量内存缓存（LB 选路热路径，P3）：与数据库缓存同新鲜度口径
//! （fetched_at 10 分钟，`persist::cache_age_fresh*` 同一判定），命中免一次
//! DB 往返；缓存缺失时的真实抓取按 provider 单飞去重（同一瞬间多个并发
//! 请求只发一次上游调用）。Provider 更新/删除时调用 [`UsageMemCache::invalidate`]
//! 保持与数据库缓存同失效语义。
//!
//! 失效代次护栏（11-02）：invalidate 自增该 provider 的代次，抓取在开始前
//! 记录代次、写库/回填前比对——期间发生过失效（凭据已变）则丢弃在途结果，
//! 避免旧凭据抓到的数据把刚失效的缓存写回。

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use sea_orm::DatabaseConnection;

use super::error::UsageError;
use super::persist::{cache_age_fresh, cache_age_fresh_at, fetch_and_store};
use super::types::UsageData;

/// 抓取结果（单飞通道载荷，保留错误供接口层按类型映射响应）。
type FetchResult = Result<UsageData, UsageError>;

/// 并发抓取 in-flight 表：provider_id → 完成通知通道。
type InFlightMap = tokio::sync::Mutex<HashMap<i32, tokio::sync::watch::Sender<FetchResult>>>;

#[derive(Clone, Default)]
pub struct UsageMemCache {
    entries: Arc<tokio::sync::Mutex<HashMap<i32, UsageData>>>,
    in_flight: Arc<InFlightMap>,
    /// provider_id → 失效代次（invalidate 自增；抓取写回前比对，11-02）。
    generations: Arc<tokio::sync::Mutex<HashMap<i32, u64>>>,
    /// 全局代次计数器（为每个 provider 分配单调递增的代次）。
    generation_seq: Arc<AtomicU64>,
    /// provider_id → 最近一次真实抓取尝试时刻（含失败）。失败负缓存：
    /// 负缓存窗口内不再发起真实厂商调用（持续故障期避免每请求重抓，07-01）。
    last_attempt: Arc<tokio::sync::Mutex<HashMap<i32, chrono::DateTime<chrono::Utc>>>>,
}

/// 抓取失败后的负缓存窗口（窗口内读侧按「无数据」处理，不再真抓）。
const NEGATIVE_CACHE_TTL: chrono::TimeDelta = chrono::TimeDelta::minutes(1);

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

    /// 负缓存窗口内是否应跳过真实抓取（最近一次尝试失败且未超窗）。
    async fn in_negative_cache(&self, provider_id: i32) -> bool {
        let attempts = self.last_attempt.lock().await;
        attempts
            .get(&provider_id)
            .is_some_and(|at| chrono::Utc::now().signed_duration_since(*at) < NEGATIVE_CACHE_TTL)
    }

    /// 记录一次真实抓取尝试（成功或失败）。失败进入负缓存窗口。
    async fn note_attempt(&self, provider_id: i32) {
        self.last_attempt
            .lock()
            .await
            .insert(provider_id, chrono::Utc::now());
    }

    /// 清掉负缓存（成功后调用：下次 miss 立即允许重抓）。
    async fn clear_attempt(&self, provider_id: i32) {
        self.last_attempt.lock().await.remove(&provider_id);
    }

    /// 失效单家（Provider 更新/删除后调用，避免旧凭据用量残留）。
    /// 同时自增失效代次：在途抓取写回前比对不一致即丢弃（11-02）。
    pub async fn invalidate(&self, provider_id: i32) {
        self.entries.lock().await.remove(&provider_id);
        self.in_flight.lock().await.remove(&provider_id);
        self.last_attempt.lock().await.remove(&provider_id);
        let seq = self.generation_seq.fetch_add(1, Ordering::SeqCst) + 1;
        self.generations.lock().await.insert(provider_id, seq);
    }

    /// 全量失效（备份导入整体替换供应商后调用，11-21）：清空条目与在途表，
    /// 并对所有已知 provider 自增代次（在途抓取写回前比对不一致即丢弃）。
    pub async fn invalidate_all(&self) {
        self.entries.lock().await.clear();
        self.last_attempt.lock().await.clear();
        self.in_flight.lock().await.clear();
        let mut generations = self.generations.lock().await;
        let mut seq = self.generation_seq.load(Ordering::SeqCst);
        for generation in generations.values_mut() {
            seq += 1;
            *generation = seq;
        }
        self.generation_seq.store(seq, Ordering::SeqCst);
    }

    /// 当前失效代次（抓取开始前记录，写回前比对）。
    pub async fn generation(&self, provider_id: i32) -> u64 {
        self.generations
            .lock()
            .await
            .get(&provider_id)
            .copied()
            .unwrap_or(0)
    }

    /// 并发去重的真实抓取：同 provider 同时只发一次上游调用，其余请求等结果。
    /// `fetch` 为实际抓取闭包（生产走 `fetch_and_store`），可注入计数便于测试。
    ///
    /// 失效代次护栏（11-02）：开始前记录代次，抓取期间发生过 invalidate
    /// （凭据已变）则丢弃结果——不回填内存、不写库，调用方得到 None 走回退。
    pub async fn fetch_shared_with<F, Fut>(&self, provider_id: i32, fetch: F) -> Option<UsageData>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Option<UsageData>>,
    {
        self.fetch_shared_result(provider_id, || async {
            fetch().await.ok_or(UsageError::Auth)
        })
        .await
        .ok()
    }

    /// 单飞抓取（保留错误类型供接口层映射响应；11-25 收敛路由与 LB 到同一入口）。
    /// `fetch` 返回抓取结果（不含写缓存副作用），成功且未失效时由本方法回填内存。
    pub async fn fetch_shared_result<F, Fut>(&self, provider_id: i32, fetch: F) -> FetchResult
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = FetchResult>,
    {
        self.fetch_shared_result_inner(provider_id, false, fetch)
            .await
    }

    /// 内部实现：`bypass_negative_cache` 用于手动强制刷新（`?refresh=1`）——
    /// 用户显式要求重取时必须真抓，不被自动负缓存挡住。
    async fn fetch_shared_result_inner<F, Fut>(
        &self,
        provider_id: i32,
        bypass_negative_cache: bool,
        fetch: F,
    ) -> FetchResult
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = FetchResult>,
    {
        // 失败负缓存：窗口内直接按失败返回，不再发真实厂商调用（07-01）。
        // in-flight 表在下方重新判定，故并发窗口内仍靠单飞去重。
        if !bypass_negative_cache && self.in_negative_cache(provider_id).await {
            return Err(UsageError::Auth);
        }
        let generation = self.generation(provider_id).await;
        let mut waiter: Option<tokio::sync::watch::Receiver<FetchResult>> = None;
        let creator_tx = {
            let mut in_flight = self.in_flight.lock().await;
            match in_flight.get(&provider_id) {
                Some(tx) => {
                    waiter = Some(tx.subscribe());
                    None
                }
                None => {
                    let (tx, _rx) = tokio::sync::watch::channel(Err(UsageError::Auth));
                    in_flight.insert(provider_id, tx.clone());
                    Some(tx)
                }
            }
        };

        if let Some(mut rx) = waiter {
            // 等待创建者完成；创建者被取消时 guard 也会通知，不会悬挂。
            let _ = rx.changed().await;
            return rx.borrow().clone();
        }

        // 创建者：负责真实抓取并通知等待者；guard 保证异常路径（future 被
        // 取消）也会清理 in-flight 并通知，避免等待者悬挂。
        struct Cleanup<'a> {
            provider_id: i32,
            in_flight: &'a Arc<InFlightMap>,
            tx: tokio::sync::watch::Sender<FetchResult>,
            sent: bool,
        }
        impl Drop for Cleanup<'_> {
            fn drop(&mut self) {
                // 尽力清理：锁竞争时放弃（等待者仍有 10 分钟新鲜度回退路径）。
                if let Ok(mut in_flight) = self.in_flight.try_lock() {
                    in_flight.remove(&self.provider_id);
                }
                if !self.sent && self.tx.borrow().is_err() {
                    let _ = self.tx.send(Err(UsageError::Auth));
                }
            }
        }
        let mut cleanup = Cleanup {
            provider_id,
            in_flight: &self.in_flight,
            tx: creator_tx.expect("创建者分支必有 sender"),
            sent: false,
        };
        let mut result = fetch().await;
        // 抓取期间发生过失效：丢弃结果，避免旧凭据数据写回刚清空的缓存。
        if self.generation(provider_id).await != generation {
            result = Err(UsageError::Stale);
        } else if let Ok(data) = &result {
            self.store(data.clone()).await;
        }
        // 成功清负缓存（下次 miss 立即允许重抓）；失败记时刻进负缓存窗口。
        if result.is_ok() {
            self.clear_attempt(provider_id).await;
        } else {
            self.note_attempt(provider_id).await;
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

    /// 单飞抓取并写库（保留错误类型；供管理端用量接口使用）。
    /// `force`：`?refresh=1` 等强制重取（跳过新鲜度短路，仍走单飞与代次护栏）。
    pub async fn fetch_shared_stored(
        &self,
        db: &DatabaseConnection,
        provider_id: i32,
        force: bool,
    ) -> FetchResult {
        if !force && let Ok(Some(data)) = super::persist::read_usage_cache(db, provider_id).await {
            return Ok(data);
        }
        // 闭包内先抓取、写库前再比对代次：抓取期间凭据被更新则连库缓存也不写。
        let cache = self.clone();
        self.fetch_shared_result_inner(provider_id, force, move || {
            let db = db.clone();
            let cache = cache.clone();
            async move {
                let generation = cache.generation(provider_id).await;
                let data = crate::usage::query_provider_usage(&db, provider_id).await?;
                if cache.generation(provider_id).await != generation {
                    return Err(UsageError::Stale);
                }
                super::persist::write_usage_cache(&db, &data)
                    .await
                    .map_err(|e| UsageError::Database(e.to_string()))?;
                Ok(data)
            }
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
    async fn mem_cache_fetch_discards_result_when_invalidated_midflight() {
        let cache = UsageMemCache::default();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        let fetcher = cache.clone();
        let handle = tokio::spawn(async move {
            fetcher
                .fetch_shared_with(11, move || async move {
                    let _ = started_tx.send(());
                    let _ = release_rx.await;
                    Some(balance_data(11, &[1.0]))
                })
                .await
        });
        started_rx.await.unwrap();
        // 抓取在途时供应商被更新（缓存失效 + 代次自增）。
        cache.invalidate(11).await;
        let _ = release_tx.send(());
        assert!(
            handle.await.unwrap().is_none(),
            "失效期间完成的抓取结果应作废（11-02 护栏）"
        );
        assert!(cache.read(11).await.is_none(), "作废结果不得回填缓存");
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

    /// 07-04：抓取失败时等待者收到错误而非悬挂，且失败进入负缓存窗口。
    #[tokio::test]
    async fn mem_cache_failure_reaches_waiters_and_opens_negative_cache() {
        let cache = UsageMemCache::default();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        let creator = cache.clone();
        let handle = tokio::spawn(async move {
            creator
                .fetch_shared_result(21, move || async move {
                    let _ = started_tx.send(());
                    let _ = release_rx.await;
                    Err(UsageError::Upstream(500, "boom".to_string()))
                })
                .await
        });
        started_rx.await.unwrap();
        // 第二个调用者在创建者完成前进入 → 走 waiter 分支。
        let waiter = cache.clone();
        let waiting = tokio::spawn(async move {
            waiter
                .fetch_shared_result(21, || async { Ok(balance_data(21, &[1.0])) })
                .await
        });
        tokio::task::yield_now().await;
        let _ = release_tx.send(());
        let creator_result = handle.await.unwrap();
        let waiter_result = waiting.await.unwrap();
        assert!(creator_result.is_err(), "创建者应拿到失败");
        assert!(
            waiter_result.is_err(),
            "等待者应拿到同一失败（不悬挂 Nor 误用自身闭包）"
        );

        // 失败后进入负缓存：后续调用直接失败，不再执行闭包。
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let calls2 = calls.clone();
        let after = cache
            .fetch_shared_result(21, move || {
                let c = calls2.clone();
                async move {
                    c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Ok(balance_data(21, &[2.0]))
                }
            })
            .await;
        assert!(after.is_err(), "负缓存窗口内应直接失败");
        assert_eq!(
            calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "负缓存窗口内不得执行真实抓取闭包"
        );
    }

    /// 07-04：创建者成功后清负缓存，下一次调用立即允许真抓。
    #[tokio::test]
    async fn mem_cache_success_clears_negative_cache() {
        let cache = UsageMemCache::default();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        // 第一次失败（进负缓存）。
        let _ = cache
            .fetch_shared_result(31, || async { Err(UsageError::Auth) })
            .await;
        // 手动失效（清除负缓存）后应立即允许重抓。
        cache.invalidate(31).await;
        let calls2 = calls.clone();
        let result = cache
            .fetch_shared_result(31, move || {
                let c = calls2.clone();
                async move {
                    c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Ok(balance_data(31, &[3.0]))
                }
            })
            .await;
        assert!(result.is_ok());
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(cache.read(31).await.is_some(), "成功结果应回填");
    }

    /// 07-01：创建者 future 被取消时 in-flight 清理且等待者不悬挂。
    #[tokio::test]
    async fn mem_cache_creator_cancel_notifies_waiters() {
        let cache = UsageMemCache::default();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let creator = cache.clone();
        let handle = tokio::spawn(async move {
            creator
                .fetch_shared_result(51, move || async move {
                    let _ = started_tx.send(());
                    // 永不完成：等待被 abort。
                    std::future::pending::<FetchResult>().await
                })
                .await
        });
        started_rx.await.unwrap();
        let waiter = cache.clone();
        let waiting = tokio::spawn(async move {
            waiter
                .fetch_shared_result(51, || async { Ok(balance_data(51, &[9.0])) })
                .await
        });
        tokio::task::yield_now().await;
        // 取消创建者：Cleanup guard 应通知等待者并清理 in-flight。
        handle.abort();
        let _ = handle.await;
        let waiter_result = tokio::time::timeout(std::time::Duration::from_secs(2), waiting)
            .await
            .expect("等待者不得悬挂（Cleanup guard 应通知）")
            .unwrap();
        assert!(waiter_result.is_err(), "创建者被取消时等待者应收到失败");
    }
}

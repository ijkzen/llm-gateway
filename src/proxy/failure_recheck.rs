//! 失败复查：成员请求失败后，对开启用量查询的供应商异步做实时用量核验。
//!
//! 耗尽 → 走额度门控禁用（`apply_usage_gate`，额度恢复后 usage_refresh 自动
//! 恢复）；充足 → 不动作（失败已由连续失败计数路径记录）；抓取失败按无数据
//! 处理，不禁用也不影响计数。
//!
//! 同一供应商 60 秒内不重复触发：时间窗即去重（窗口远大于单次抓取时长），
//! 避免连续失败风暴打爆用量接口。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sea_orm::{DatabaseConnection, EntityTrait};

use crate::entity::provider;
use crate::state::AppState;
use crate::usage::persist;

/// 复查节流窗口。
pub const RECHECK_INTERVAL: Duration = Duration::from_secs(60);

/// 失败复查触发资格（provider 粒度，Clone 共享同一份状态）。
#[derive(Clone, Default)]
pub struct RecheckGate {
    last_triggered: Arc<Mutex<HashMap<i32, Instant>>>,
}

impl RecheckGate {
    fn try_acquire_with_interval(&self, provider_id: i32, interval: Duration) -> bool {
        let mut last = self.last_triggered.lock().expect("recheck gate lock");
        let now = Instant::now();
        match last.get(&provider_id) {
            Some(t) if now.duration_since(*t) < interval => false,
            _ => {
                last.insert(provider_id, now);
                true
            }
        }
    }

    fn try_acquire(&self, provider_id: i32) -> bool {
        self.try_acquire_with_interval(provider_id, RECHECK_INTERVAL)
    }
}

/// 失败复查入口：节流通过则后台执行核验（不阻塞转发降级链路）。
pub fn trigger(state: &AppState, provider_id: i32, request_id: &str) {
    if !state.recheck_gate.try_acquire(provider_id) {
        return;
    }
    let db = state.db.clone();
    let request_id = request_id.to_string();
    tokio::spawn(async move {
        handle_failure(&db, provider_id, &request_id).await;
    });
}

/// 执行一次核验：强制实时抓取用量并落库缓存，再按额度门控判定。
async fn handle_failure(db: &DatabaseConnection, provider_id: i32, request_id: &str) {
    let p = match provider::Entity::find_by_id(provider_id).one(db).await {
        Ok(Some(p)) => p,
        Ok(None) => return,
        Err(e) => {
            tracing::warn!(provider_id, request_id, "失败复查读取供应商失败：{e}");
            return;
        }
    };
    if !crate::usage::usage_enabled(&p.extra) {
        return;
    }
    match persist::fetch_and_store(db, provider_id).await {
        Ok(data) => {
            if let Err(e) = persist::apply_usage_gate(db, &p, &data).await {
                tracing::warn!(provider_id, request_id, "失败复查额度门控执行失败：{e}");
            }
        }
        Err(e) => {
            tracing::warn!(
                provider_id,
                request_id,
                error = %e,
                "失败复查用量抓取失败，跳过核验"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_allows_first_then_throttles() {
        let gate = RecheckGate::default();
        let interval = Duration::from_millis(50);
        assert!(gate.try_acquire_with_interval(1, interval));
        assert!(
            !gate.try_acquire_with_interval(1, interval),
            "窗口内不重复触发"
        );
        assert!(
            gate.try_acquire_with_interval(2, interval),
            "供应商之间独立"
        );
    }

    #[test]
    fn gate_reacquires_after_window() {
        let gate = RecheckGate::default();
        assert!(gate.try_acquire_with_interval(1, Duration::ZERO));
        std::thread::sleep(Duration::from_millis(5));
        assert!(gate.try_acquire_with_interval(1, Duration::from_millis(1)));
    }
}

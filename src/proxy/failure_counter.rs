//! 连续失败计数（内存，provider 粒度）。
//!
//! 该供应商任一转发请求失败 +1（不论失败能否重试），任一请求成功清零，
//! 进程重启清零。达到设置项 `max_consecutive_failures` 阈值时由调用方执行
//! 连续失败熔断（`provider_repo::disable_provider_on_failures`）。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// provider 粒度的内存连续失败计数器（Clone 共享同一份状态）。
#[derive(Clone, Default)]
pub struct FailureCounter {
    counters: Arc<Mutex<HashMap<i32, u32>>>,
}

impl FailureCounter {
    /// 记一次失败，返回累计连续失败次数。
    pub fn record_failure(&self, provider_id: i32) -> u32 {
        let mut counters = self.counters.lock().expect("failure counters lock");
        let entry = counters.entry(provider_id).or_insert(0);
        *entry += 1;
        *entry
    }

    /// 该供应商任一请求成功即清零。
    pub fn reset(&self, provider_id: i32) {
        self.counters
            .lock()
            .expect("failure counters lock")
            .remove(&provider_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_incrementally_per_provider() {
        let counter = FailureCounter::default();
        assert_eq!(counter.record_failure(1), 1);
        assert_eq!(counter.record_failure(1), 2);
        assert_eq!(counter.record_failure(2), 1, "供应商之间相互独立");
    }

    #[test]
    fn reset_clears_counter() {
        let counter = FailureCounter::default();
        counter.record_failure(1);
        counter.record_failure(1);
        counter.reset(1);
        assert_eq!(counter.record_failure(1), 1, "清零后从 1 重新计");
    }

    #[test]
    fn reset_unknown_provider_is_noop() {
        let counter = FailureCounter::default();
        counter.reset(42);
        assert_eq!(counter.record_failure(42), 1);
    }
}

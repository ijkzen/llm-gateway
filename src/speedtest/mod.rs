//! 模型测速（TPS 基准，ADR-0027）。
//!
//! 与「自动恢复探测」「边界实测」共用同一套 `api_key_name='test'` 流量标记，
//! 但语义不同：那两者关心「这个供应商还能不能用」，本模块关心「这个模型吐字
//! 有多快」。因此请求形态也不同——探测发最小请求（`TEST_PROMPT` =「你好」、
//! 非流式），测速发长文生成请求（流式 + 目标 2048 tokens），才能观测到稳定的
//! TPS 而不是噪声。
//!
//! 三个不变量：
//! - **不入统计**：测速请求照常写 `request` 表供审计，但 `api_key_name='test'`
//!   使 12 个 stats 端点与快照统一排除（ADR-0024 的基础谓词）。
//! - **不计熔断**：测速失败不触碰可用性状态机，一次批量测速不会把供应商打成
//!   连续失败禁用，也不会触发飞书通知。
//! - **只留最近一次运行**：开始新运行即删除旧运行与结果行，页面语义永远是
//!   「正在跑 / 上一次的成绩」。

pub mod bench;
pub mod candidate;
pub mod runner;
pub mod store;

pub use bench::{BenchOutcome, bench_one, to_result_model};
pub use candidate::{Candidate, Filter, resolve};
pub use runner::{StartOutcome, cancel, recover_on_startup, start};

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// 测速题面：要求长文输出，使 TPS 样本足够大（短回答会被上游缓冲成单块，
/// 输出窗口压缩到几十毫秒，算出来的 TPS 全是噪声）。
pub const SPEED_TEST_PROMPT: &str = "请用中文系统讲解一门编程语言，内容包括：语言的设计哲学与典型\
应用场景、基础语法（变量、类型、控制流）、函数与模块组织、错误处理惯用法、并发或异步模型、\
标准库常用能力，并给出至少三段带注释的示例代码。要求内容尽量完整详实，篇幅尽量长。";

/// 输出 token 上限取模型登记值（`max_output_tokens`），不再夹取。
///
/// 曾经夹到 `[512, 2048]`，理由是「大于它单模型耗时过长」；实测推翻了这个
/// 权衡：推理模型（如 OpenDesign 的 deepseek-v4.1-flash，登记 384000）在
/// 2048 预算下会把全部预算烧在思考上、正文一个 token 都不出，ttft 与 TPS
/// 双双缺失。放开上限让模型自然生成完，才有可测的输出阶段。
///
/// 登记值缺失（0）时退回 [`SPEED_TEST_FALLBACK_TOKENS`]：传 `max_tokens: 0`
/// 会被多数上游直接拒绝。
pub const SPEED_TEST_FALLBACK_TOKENS: i64 = 2048;
/// 测速请求的 request_id 前缀，便于在请求日志里与自动探测（`test-`）区分。
pub const SPEED_TEST_REQUEST_PREFIX: &str = "speedtest-";

/// 本次请求的输出 token 上限：直接用模型登记值，未登记时兜底。
///
/// 刻意**不设上限**：见 [`SPEED_TEST_FALLBACK_TOKENS`] 的说明。相应地，单模型
/// 也没有硬超时——只保留上游客户端自身的连接/读超时（`proxy::upstream` 的
/// 四常量，最坏约 260 秒），自然生成完即结束。
pub fn target_max_tokens(max_output_tokens: i64) -> i64 {
    if max_output_tokens > 0 {
        max_output_tokens
    } else {
        SPEED_TEST_FALLBACK_TOKENS
    }
}

/// 运行中标志（单槽位，内存态）。
///
/// 槽里存的是**当前运行的取消旗标**：有值即表示有运行占位。取消只置位旗标，
/// 由调度循环在各模型开跑前检查；正在跑的请求允许自然结束（中断正在读的
/// 上游流会浪费已发出的 token）。
#[derive(Clone, Default)]
pub struct SpeedTestSlot(Arc<Mutex<Option<Arc<AtomicBool>>>>);

impl SpeedTestSlot {
    /// 尝试占用槽位。已有运行返回 None（单实例约束）。
    pub fn try_begin(&self) -> Option<(Arc<AtomicBool>, SlotGuard)> {
        let mut guard = self.0.lock().expect("speed test slot lock");
        if guard.is_some() {
            return None;
        }
        let flag = Arc::new(AtomicBool::new(false));
        *guard = Some(flag.clone());
        Some((flag.clone(), SlotGuard { slot: self.clone() }))
    }

    /// 请求取消当前运行；无运行返回 false。
    pub fn request_cancel(&self) -> bool {
        let guard = self.0.lock().expect("speed test slot lock");
        match guard.as_ref() {
            Some(flag) => {
                flag.store(true, Ordering::SeqCst);
                true
            }
            None => false,
        }
    }

    /// 是否有运行占位。
    pub fn is_running(&self) -> bool {
        self.0.lock().expect("speed test slot lock").is_some()
    }
}

/// 槽位占用凭证：drop 时清空槽位（运行结束、panic 或以任一方式退出都释放）。
pub struct SlotGuard {
    slot: SpeedTestSlot,
}

impl Drop for SlotGuard {
    fn drop(&mut self) {
        let mut guard = self.slot.0.lock().expect("speed test slot lock");
        *guard = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_max_tokens_uses_registered_value_without_cap() {
        assert_eq!(
            target_max_tokens(0),
            SPEED_TEST_FALLBACK_TOKENS,
            "未登记走兜底"
        );
        assert_eq!(target_max_tokens(128), 128, "小值不再抬高");
        assert_eq!(target_max_tokens(1000), 1000);
        assert_eq!(target_max_tokens(2048), 2048);
        assert_eq!(target_max_tokens(384_000), 384_000, "大值不再夹到 2048");
    }

    #[test]
    fn slot_allows_only_one_run_and_frees_on_drop() {
        let slot = SpeedTestSlot::default();
        let (_flag, guard) = slot.try_begin().expect("首次占用应成功");
        assert!(slot.is_running());
        assert!(slot.try_begin().is_none(), "运行中不允许第二次占用");

        drop(guard);
        assert!(!slot.is_running(), "凭证 drop 后应释放槽位");
        assert!(slot.try_begin().is_some());
    }

    #[test]
    fn cancel_sets_flag_and_reports_absence_when_idle() {
        let slot = SpeedTestSlot::default();
        assert!(!slot.request_cancel(), "空闲时取消应返回 false");

        let (flag, _guard) = slot.try_begin().expect("占用成功");
        assert!(!flag.load(Ordering::SeqCst));
        assert!(slot.request_cancel());
        assert!(flag.load(Ordering::SeqCst), "取消应置位旗标");
    }
}

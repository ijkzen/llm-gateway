//! 统计快照（request_log_snapshot，ADR-0021）：闭口时间桶的预聚合与读路径支撑。
//!
//! 分工：
//! - `core.rs`：四级桶帧（hour/day/month/year，设置表时区固定偏移模型）、闭桶判定
//!   （终点 + 固化余量）、窗口 →「快照闭桶 + 实时兑底段」分解（纯函数）。
//! - `registry.rs`：主体类型与指标名（单一事实源，生成/读取共用）。
//! - `generator.rs`/`reader.rs`/`tasks.rs`：固化、读侧合并与内置任务（均已接入）。

mod core;
mod generator;
mod math;
mod reader;
mod registry;
mod subject;
mod tasks;

/// 公共 API（集成测试使用）：桶帧与固化入口。
pub use core::{Frame, Level};
pub use generator::finalize_bucket;

pub(crate) use core::{MARGIN_MS, natural_periods, period_key_of_day_index, period_key_of_ts};
// 测试专用（tasks/generator 单测直驱桶帧）。
#[cfg(test)]
pub(crate) use core::{HOUR_MS, frames_covering};
pub(crate) use math::{percentile, round_5, weighted_ratio};
pub(crate) use reader::{Coverage, bucket_finalized, coverage, snapshot_rows, trim_zero_prefix};
pub(crate) use registry::metrics;
pub(crate) use registry::{
    ENTITY_API_KEY, ENTITY_API_KEY_MODEL, ENTITY_MODEL, ENTITY_PROVIDER, ENTITY_VIRTUAL_MODEL,
    ENTITY_VM_MEMBER, ENTITY_WHOLE, percentile_level_ok,
};
pub(crate) use registry::{expr_of, select_list, success_prims};
pub(crate) use subject::{
    api_key_reconcile_names, demote_if_unresolved, resolve_api_key_id, resolve_pm_key,
    resolve_pm_keys_for_filter,
};
pub(crate) use tasks::{run_snapshot_generation, run_snapshot_heal};

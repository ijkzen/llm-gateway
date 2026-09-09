//! 统计快照（request_log_snapshot，ADR-0021）：闭口时间桶的预聚合与读路径支撑。
//!
//! 分工：
//! - `core.rs`：四级桶帧（hour/day/month/year，设置表时区固定偏移模型）、闭桶判定
//!   （终点 + 固化余量）、窗口 →「快照闭桶 + 实时兑底段」分解（纯函数）。
//! - `registry.rs`：主体类型与指标名（单一事实源，生成/读取共用）。
//! - 生成器、读路径合并、内置任务在后续模块。
#![allow(dead_code)] // 读路径/任务接入前，纯核心与生成器先落地供直测

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

#[allow(unused_imports)] // 端点接入中，部分面暂未消费
pub(crate) use core::{
    DAY_MS, HOUR_MS, MARGIN_MS, decompose, frames_covering, natural_periods,
    period_key_of_day_index, period_key_of_ts, period_start_ms,
};
pub(crate) use math::{percentile, round_5, weighted_ratio};
#[allow(unused_imports)]
pub(crate) use reader::{Coverage, bucket_finalized, coverage, snapshot_rows, trim_zero_prefix};
pub(crate) use registry::metrics;
pub(crate) use registry::{
    ENTITY_API_KEY, ENTITY_API_KEY_MODEL, ENTITY_MODEL, ENTITY_PROVIDER, ENTITY_VIRTUAL_MODEL,
    ENTITY_VM_MEMBER, ENTITY_WHOLE, percentile_level_ok,
};
#[allow(unused_imports)]
pub(crate) use registry::{expr_of, metric_exprs, select_list, success_prims};
#[allow(unused_imports)]
pub(crate) use subject::{
    api_key_reconcile_names, demote_if_unresolved, resolve_api_key_id, resolve_pm_key,
};
pub(crate) use tasks::{run_snapshot_generation, run_snapshot_heal};

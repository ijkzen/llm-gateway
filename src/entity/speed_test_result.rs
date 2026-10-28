use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// SpeedTestResult: 一次测速运行中单个供应商模型的成绩行（ADR-0027）。
///
/// 粒度是 `provider_model`（供应商 + 上游模型名），虚拟模型只作为候选过滤维度。
/// 供应商名与模型名冗余成快照字段：供应商改名/删除后旧成绩仍可读，页面也不必
/// 依赖 `request` 表的保留期（原始逐行数据经 `request_id` 可回溯审计）。
///
/// 行状态：`pending`（排队）/ `done`（成功出分）/ `failed`（非 2xx、网络错误
/// 或超预算）/ `cancelled`（运行被取消时尚未开始）。失败只由传输层决定，
/// 输出 token 偏短不算失败，在汇总层标「样本不足」。
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "speed_test_result")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = true)]
    pub id: i32,
    pub run_id: i32,
    pub provider_id: i32,
    /// 供应商名快照（行渲染不 join provider，供应商删除后仍可读）。
    pub provider_name: String,
    /// 上游模型主键快照（`provider_model.model_id`）。
    pub provider_model_pk: i32,
    /// 上游模型名快照（实际发给上游的 model）。
    pub model_id: String,
    pub status: String,
    /// 首 token 耗时（毫秒）；语义同 `request.ttft`。
    #[sea_orm(nullable)]
    pub ttft_ms: Option<i64>,
    /// 输出 token 总数（含推理 token）。
    #[sea_orm(nullable)]
    pub output_tokens: Option<i64>,
    /// 输出阶段耗时（毫秒）；语义同 `request.output_tokens_time`。
    #[sea_orm(nullable)]
    pub output_tokens_time_ms: Option<i64>,
    /// 端到端耗时（毫秒）：从网络阶段起点到流结束。
    #[sea_orm(nullable)]
    pub total_ms: Option<i64>,
    /// TPS，与 `request.tps` 同口径（output_tokens / (ttft + 输出耗时)）。
    #[sea_orm(nullable)]
    pub tps: Option<f64>,
    /// 对应 `request` 表主键，供跳转请求日志审计。
    #[sea_orm(nullable)]
    pub request_id: Option<String>,
    /// 失败原因（截断 200 字符，与 request 表同一口径）。
    #[sea_orm(nullable)]
    pub fail_reason: Option<String>,
    /// 该行终态化时刻（毫秒时间戳）。
    #[sea_orm(nullable)]
    pub finished_at: Option<i64>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// Request: 每次 /v1 转发请求的指标记录（成功与失败均落一行）。
///
/// 字段口径：
/// - `ttft`：流式首 token 耗时（毫秒），从「上游建连完成」到「收到首个内容
///   块」（含上游排队/处理等待），非流式为 NULL。
/// - `input_tokens`：归一后的输入 token（含缓存命中部分）；上游未返回 usage 时为 NULL。
/// - `input_cache_tokens`：缓存命中 token（Anthropic 的 cache_read + cache_creation）。
/// - `output_tokens`：输出 token 总数（含推理/思考 token）；usage 缺失时为 NULL。
/// - `output_tokens_time`：输出阶段耗时（毫秒）；流式为末 token − 首 token，
///   非流式为响应体接收完成 − 响应头到达。
/// - `network_latency`：TCP 建连 + TLS 握手耗时（毫秒），每请求独立建连实测。
/// - `tps`：output_tokens / output_tokens_time（秒），分母无效时为 0。
///
/// 注意：`output_tokens_time` 是「token 到达窗口」，上游对短回复常缓冲后
/// 一次性冲刷，窗口会被压缩到几十毫秒甚至 0（单 chunk 突发，此时 tps 为 0
/// 但实际是最快）。网关刻意保留原始值不做兜底；口径修正（如窗口 <100ms 时
/// 退回整段上游耗时）应在指标展示层按需处理，参考 nyro 的展示端兜底。
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "request")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub request_id: String,
    /// 命中的虚拟模型。
    pub virtual_model_id: i32,
    /// 实际服务的供应商成员（failover 时为最终成功者，全败为最后尝试者）。
    pub provider_id: i32,
    /// 该成员的真实模型 ID（即实际发给上游的 model）。
    pub model_id: String,
    /// 客户端请求是否为流式。
    pub stream: bool,
    #[sea_orm(nullable)]
    pub ttft: Option<i64>,
    #[sea_orm(nullable)]
    pub input_tokens: Option<i64>,
    pub input_cache_tokens: i64,
    pub input_cache_rate: f64,
    #[sea_orm(nullable)]
    pub output_tokens: Option<i64>,
    #[sea_orm(nullable)]
    pub output_tokens_time: Option<i64>,
    pub tps: f64,
    pub network_latency: i64,
    /// 收到请求的时刻（毫秒时间戳）。
    pub start_time: i64,
    /// 完成回写或连接中断的时刻（毫秒时间戳）。
    pub end_time: i64,
    pub request_time: i64,
    /// 上游 HTTP 状态 < 400 即成功。
    pub success: bool,
    /// 失败原因；成功但客户端中断时记「客户端提前断开」。
    #[sea_orm(nullable)]
    pub fail_reason: Option<String>,
    #[sea_orm(nullable)]
    pub total_tokens: Option<i64>,
    /// 调用方 API Key 名称。
    pub api_key_name: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

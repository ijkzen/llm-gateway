use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// 虚拟模型负载均衡策略。
#[derive(Clone, Debug, PartialEq, Eq, EnumIter, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "i32", db_type = "Integer")]
pub enum LoadBalancingStrategy {
    /// 订阅制优先
    SubscriptionFirst = 0,
    /// 按量付费优先
    PayAsYouGoFirst = 1,
    /// 轮转
    RoundRobin = 2,
    /// 随机
    Random = 3,
}

/// 虚拟模型降级策略。
#[derive(Clone, Debug, PartialEq, Eq, EnumIter, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "i32", db_type = "Integer")]
pub enum FallbackStrategy {
    /// 直接失败
    FailDirectly = 0,
    /// 依次重试本虚拟模型内其他被启用的成员
    RetryEnabledMembers = 1,
}

/// 虚拟模型接口类型（编号与供应商协议类型对齐）。
pub const INTERFACE_OPENAI_COMPAT: i32 = 0;
pub const INTERFACE_OPENAI_RESPONSES: i32 = 1;
pub const INTERFACE_ANTHROPIC_MESSAGES: i32 = 2;
/// 保留值：暂无 Gemini 原生端点，仅占位对齐协议编号。
pub const INTERFACE_GEMINI: i32 = 3;
pub const INTERFACE_FULL_COMPATIBLE: i32 = 4;

/// /v1/chat/completions 与 /v1/models 服务的接口类型
/// （OpenAI Compatible 与 Full Compatible）。
pub const CHAT_SERVED_TYPES: [i32; 2] = [INTERFACE_OPENAI_COMPAT, INTERFACE_FULL_COMPATIBLE];

/// VirtualModel:对外暴露的虚拟模型，聚合多个供应商模型条目。
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "virtual_model")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = true)]
    pub virtual_model_id: i32,
    /// 对外暴露的模型 ID,客户端调用时使用;全局唯一。
    #[sea_orm(unique)]
    pub display_id: String,
    /// 是否启用;禁用后不出现在 /v1/models。
    #[sea_orm(default_value = "1")]
    pub enable: bool,
    /// 负载均衡策略:0=订阅制优先、1=按量付费优先、2=轮转、3=随机。
    #[sea_orm(default_value = "0")]
    pub load_balancing_strategy: i32,
    /// 降级策略:0=直接失败、1=依次重试其他启用成员。
    #[sea_orm(default_value = "0")]
    pub fallback_strategy: i32,
    /// 接口类型:0=OpenAI Compat、1=Responses、2=Anthropic Messages、
    /// 3=Gemini(保留)、4=Full Compatible。历史行由迁移 23 回填 4。
    #[sea_orm(default_value = "0")]
    pub interface_type: i32,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

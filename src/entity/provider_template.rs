use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// Provider 接入协议类型。
#[derive(Clone, Debug, PartialEq, Eq, EnumIter, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "i32", db_type = "Integer")]
pub enum ProtocolType {
    /// OpenAI Compatible（/chat/completions 等）
    OpenAiCompatible = 0,
    /// OpenAI Response API
    OpenAiResponse = 1,
    /// Anthropic Messages API
    AnthropicMessage = 2,
    /// Gemini（Generative Language API）
    Gemini = 3,
}

/// Provider 付费模式。
#[derive(Clone, Debug, PartialEq, Eq, EnumIter, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "i32", db_type = "Integer")]
pub enum BillingMode {
    /// 按量付费
    PayAsYouGo = 0,
    /// 订阅制
    Subscription = 1,
}

/// Provider 模板：一个模型提供商的接入信息与展示所需额外字段。
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "provider_template")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = true)]
    pub provider_template_id: i32,
    #[sea_orm(unique)]
    pub name: String,
    pub base_url: String,
    pub protocol_type: i32,
    pub billing_mode: i32,
    /// 展示额外信息（余额、月度用量等）所需字段，JSON 字符串，value 为字符串。
    #[sea_orm(default_value = "{}")]
    pub extra: String,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

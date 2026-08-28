use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// Provider 可用状态。
#[derive(Clone, Debug, PartialEq, Eq, EnumIter, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "i32", db_type = "Integer")]
pub enum ProviderStatus {
    /// 可用
    Available = 0,
    /// 不可用
    Unavailable = 1,
}

/// Provider:用户配置的一个模型提供商接入实例。
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "provider")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = true)]
    pub id: i32,
    /// 提供商名称,唯一。
    #[sea_orm(unique)]
    pub name: String,
    /// 是否启用。
    #[sea_orm(default_value = "1")]
    pub enable: bool,
    pub base_url: String,
    /// API 密钥,加密保存(见 crypto 模块)。
    pub api_key: String,
    /// 自定义请求头,JSON 字符串。
    #[sea_orm(default_value = "{}")]
    pub custom_header: String,
    /// 可用状态:0=available、1=unavailable。
    #[sea_orm(default_value = "0")]
    pub status: i32,
    /// 协议类型:0=OpenAI Compatible、1=OpenAI Response、2=Anthropic Message、3=Gemini。
    #[sea_orm(default_value = "0")]
    pub protocol_type: i32,
    /// 付费模式:0=按量付费、1=订阅制。
    #[sea_orm(default_value = "0")]
    pub billing_mode: i32,
    /// 额外字段,JSON 字符串。
    #[sea_orm(default_value = "{}")]
    pub extra: String,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

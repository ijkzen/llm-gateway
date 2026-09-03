use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

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
    /// 协议类型:0=OpenAI Compatible、1=OpenAI Response、2=Anthropic Message、3=Gemini。
    #[sea_orm(default_value = "0")]
    pub protocol_type: i32,
    /// 付费模式:0=按量付费、1=订阅制。
    #[sea_orm(default_value = "0")]
    pub billing_mode: i32,
    /// 额外字段,JSON 字符串。
    #[sea_orm(default_value = "{}")]
    pub extra: String,
    /// 列表排序权重,越小越靠前。
    #[sea_orm(default_value = "0")]
    pub sort_order: i32,
    /// 是否经网络代理（HTTP 代理）转发该供应商请求。
    #[sea_orm(default_value = "0")]
    pub proxy_enabled: bool,
    /// HTTP 代理地址（如 `http://127.0.0.1:7890`，无认证）。
    #[sea_orm(default_value = "")]
    pub proxy_addr: String,
    /// 连续失败禁用标记：由转发链路连续失败熔断设置；用量定时刷新不自动恢复，
    /// 仅管理员手动启用时清除（与额度门控禁用的自动恢复路径区分）。
    #[sea_orm(default_value = "0")]
    pub failure_disabled: bool,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

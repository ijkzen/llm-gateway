use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

// 14-06：原 ProtocolType / BillingMode 两个 DeriveActiveEnum 全仓零消费（校验点
// 都用裸数字或 provider_model::refresh 的 PROTOCOL_* 常量），删除以免误导为单源。
// 协议编号的事实源：`provider_model::refresh::PROTOCOL_*`；付费模式见本模块常量。

/// 付费模式编号（按量付费）。
pub const BILLING_MODE_PAY_AS_YOU_GO: i32 = 0;
/// 付费模式编号（订阅制）。
pub const BILLING_MODE_SUBSCRIPTION: i32 = 1;

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

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// 通知渠道配置（当前仅飞书，ADR-0023）：单行表，`channel` 为主键。
///
/// `config` 整段 AES-256-GCM 加密落库（`enc:v1:` 前缀，密钥与 api_key 同源），
/// 明文为 JSON `{appId, appSecret, receiverType, receiver}`；接口对外只返回
/// 掩码后的 appSecret（同「key 掩码」口径）。
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "notification_channel")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub channel: String,
    pub config: String,
    /// 通知总开关：关闭时不发送任何自动转换通知。
    pub enable: bool,
    /// 最近一次发送失败原因（空 = 上次成功或尚未发送过）。
    pub last_error: String,
    /// 最近一次发送时间（成功与失败都记）。
    pub last_sent_at: Option<DateTimeUtc>,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

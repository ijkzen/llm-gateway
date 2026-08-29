use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// Session: 管理后台登录会话。主键为登录令牌的 SHA-256 摘要（不落明文，
/// 数据库泄露后无法复用令牌）。
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "session")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    /// 归属用户。
    pub user_id: i32,
    pub created_at: DateTimeUtc,
    /// 过期时间，过期会话在校验与登录时清理。
    pub expires_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

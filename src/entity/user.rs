use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// User: 管理后台登录用户（当前为单用户系统，首次启动时通过初始化流程创建）。
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "user")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = true)]
    pub id: i32,
    /// 用户名，唯一。
    #[sea_orm(unique)]
    pub username: String,
    /// argon2id 密码哈希（见 auth 模块）。
    pub password_hash: String,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

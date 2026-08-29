use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// ApiKey:调用方访问网关的凭证,服务端生成,加密保存。
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "api_key")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = true)]
    pub id: i32,
    /// 凭证名称,唯一。
    #[sea_orm(unique)]
    pub name: String,
    /// 密钥,服务端生成,加密保存(见 crypto 模块)。
    pub key: String,
    /// 是否启用。
    #[sea_orm(default_value = "1")]
    pub enable: bool,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

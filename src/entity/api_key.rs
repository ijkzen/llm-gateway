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
    /// 明文密钥的 SHA-256 摘要,用于 /v1 Bearer 鉴权的 O(1) 查找;
    /// 历史数据为 NULL,启动时回填。
    #[sea_orm(nullable)]
    pub key_hash: Option<String>,
    /// 是否启用。
    #[sea_orm(default_value = "1")]
    pub enable: bool,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

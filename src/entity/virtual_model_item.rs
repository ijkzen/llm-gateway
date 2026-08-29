use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// VirtualModelItem:虚拟模型名下的成员条目，指向一个供应商模型。
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "virtual_model_item")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = true)]
    pub virtual_model_item_id: i32,
    /// 所属虚拟模型 id(逻辑外键;虚拟模型删除时应用层级联硬删)。
    pub virtual_model_id: i32,
    /// 成员供应商模型 id(逻辑外键 → provider_model.model_id)。
    /// 一个供应商模型最多归属一个虚拟模型(全局唯一索引,见 migration 6)。
    pub model_id: i32,
    /// 是否启用;实际可用性还受所属供应商 enable 影响。
    #[sea_orm(default_value = "1")]
    pub enable: bool,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

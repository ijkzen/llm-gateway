use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// 统计快照的生成元信息（键值）：最近生成所用的时区（设置表时区变更检测，
/// 不一致即触发全量重算）等。
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "snapshot_meta")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub key: String,
    pub value: String,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

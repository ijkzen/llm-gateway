use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// Provider 用量数据库缓存：定时任务（usage_refresh，每 5 分钟）写入，
/// 读接口按 10 分钟新鲜度直出（`src/usage/persist.rs`）。
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "provider_usage_cache")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = true)]
    pub id: i32,
    /// 供应商 id；唯一（每个供应商最多一行）。
    #[sea_orm(unique)]
    pub provider_id: i32,
    /// 序列化后的 UsageData（camelCase）。
    pub usage_json: String,
    /// 抓取完成时间，新鲜度判定依据（超过 10 分钟视为过期）。
    pub fetched_at: DateTimeUtc,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
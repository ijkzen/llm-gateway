use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// 定时任务执行过程中的一条日志。
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "cron_job_logs")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = true)]
    pub id: i64,
    /// 所属执行批次（cron_job_runs.run_id）。
    pub run_id: String,
    /// 单次执行内递增的序号，用于排序与增量去重。
    pub seq: i32,
    /// info / warn / error
    pub level: String,
    pub message: String,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

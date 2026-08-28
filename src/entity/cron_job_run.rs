use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// 一次定时任务执行（run）的元信息，日志按 run 组织。
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "cron_job_runs")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = true)]
    pub id: i64,
    /// 每次执行唯一标识（uuid），用于实时流订阅与日志归属。
    #[sea_orm(unique)]
    pub run_id: String,
    pub job_name: String,
    /// running / success / failed
    pub status: String,
    pub started_at: DateTimeUtc,
    pub ended_at: Option<DateTimeUtc>,
    /// 本次执行落库的日志条数（截断提示不计入）。
    pub log_count: i32,
    /// 是否因达到单次日志条数上限而被截断。
    pub truncated: bool,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

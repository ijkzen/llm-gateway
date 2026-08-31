pub mod log_capture;
pub mod log_repository;
pub mod parser;
pub mod repository;
pub mod scheduler;
pub mod seed;
#[cfg(test)]
mod test_utils;
pub mod worker;

use std::pin::Pin;
use std::sync::Arc;

use sea_orm::DatabaseConnection;
use thiserror::Error;

use crate::app_settings::AppSettings;

#[derive(Clone)]
pub struct JobContext {
    pub db: DatabaseConnection,
    /// 语言/时区设置缓存：handler 可用其按设置语言输出日志、按设置时区计算时间。
    pub settings: AppSettings,
}

pub type JobError = Box<dyn std::error::Error + Send + Sync>;

pub type JobHandler = Arc<
    dyn Fn(JobContext) -> Pin<Box<dyn Future<Output = Result<(), JobError>> + Send>> + Send + Sync,
>;

#[derive(Clone, Debug)]
pub struct JobInfo {
    pub name: String,
    pub title: String,
    pub description: String,
    pub expression: String,
    pub enabled: bool,
    pub last_run_at: Option<chrono::DateTime<chrono::Utc>>,
    pub next_run_at: Option<chrono::DateTime<chrono::Utc>>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
    pub group: String,
    pub frequency_secs: i64,
}

#[derive(Debug, Error)]
pub enum SchedulerError {
    #[error("parse error: {0}")]
    ParseError(String),
    #[error("job scheduler error: {0}")]
    JobScheduler(#[from] tokio_cron_scheduler::JobSchedulerError),
    #[error("db error: {0}")]
    Db(#[from] sea_orm::DbErr),
    #[error("job not found: {0}")]
    JobNotFound(String),
    #[error("compute next run error: {0}")]
    ComputeNextRun(String),
    #[error("worker channel closed for job: {0}")]
    WorkerChannelClosed(String),
}

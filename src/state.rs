use sea_orm::DatabaseConnection;
use tokio::sync::broadcast;

use crate::cron::log_capture::JobLogEvent;
use crate::cron::scheduler::SchedulerRuntime;

#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    pub scheduler: SchedulerRuntime,
    /// 任务日志事件广播通道，SSE 端点订阅后按任务名过滤推送。
    pub log_tx: broadcast::Sender<JobLogEvent>,
}

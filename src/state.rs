use std::sync::Arc;

use sea_orm::DatabaseConnection;
use tokio::sync::broadcast;

use crate::app_settings::AppSettings;
use crate::availability::FailureCounter;
use crate::cron::log_capture::JobLogEvent;
use crate::cron::scheduler::SchedulerRuntime;
use crate::proxy::LbState;
use crate::proxy::failure_recheck::RecheckGate;
use crate::proxy::pool::UpstreamPool;
use crate::usage::mem_cache::UsageMemCache;

#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    pub scheduler: SchedulerRuntime,
    /// 任务日志事件广播通道（载荷 Arc 化，订阅者只克隆指针），SSE 端点
    /// 订阅后按任务名过滤推送。
    pub log_tx: broadcast::Sender<Arc<JobLogEvent>>,
    /// 虚拟模型 RoundRobin 负载均衡的轮转计数。
    pub lb_state: LbState,
    /// 连续失败计数（内存，provider 粒度）：失败 +1、成功清零，达阈值熔断。
    pub failure_counter: FailureCounter,
    /// 失败复查节流（60s 时间窗，provider 粒度）。
    pub recheck_gate: RecheckGate,
    /// /v1 上游连接池（按 host 隔离，空闲 10 分钟释放）。
    pub upstream_pool: UpstreamPool,
    /// 用量选路内存缓存（10 分钟 TTL + provider 单飞抓取，P3）。
    pub usage_mem: UsageMemCache,
    /// 语言/时区设置缓存（设置页更新后热刷新）。
    pub settings: AppSettings,
}

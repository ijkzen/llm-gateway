pub mod app_settings;
pub mod auth;
pub mod config;
pub mod cron;
pub mod crypto;
pub mod db;
pub mod entity;
pub mod i18n;
pub mod logs_cleanup;
pub mod middleware;
pub mod provider_model;
pub mod provider_repo;
pub mod provider_template;
pub mod proxy;
pub mod response;
pub mod routes;
pub mod state;
pub mod static_assets;
pub mod usage;

use std::sync::Arc;

use tokio::sync::broadcast;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::{Config, RuntimeEnv};
use crate::cron::JobContext;
use crate::cron::log_capture::JobLogLayer;
use crate::cron::log_repository::{CronJobLogRepository, SeaOrmCronJobLogRepository};
use crate::cron::repository::SeaOrmCronJobRepository;
use crate::cron::scheduler::SchedulerRuntime;
use crate::cron::worker::JobWorker;
use crate::state::AppState;

const LOG_RETENTION_DAYS: u64 = 30;
const SHUTDOWN_TIMEOUT_SECS: u64 = 10;
/// 任务日志事件广播容量：单次执行 2000 条上限下足够容纳并发任务的瞬时积压。
const JOB_LOG_BROADCAST_CAPACITY: usize = 8192;

struct AppContext {
    #[allow(dead_code)]
    log_guard: tracing_appender::non_blocking::WorkerGuard,
    state: AppState,
    worker_handle: crate::cron::worker::WorkerHandle,
}

async fn setup_logging(
    env: &RuntimeEnv,
    log_tx: broadcast::Sender<crate::cron::log_capture::JobLogEvent>,
) -> anyhow::Result<tracing_appender::non_blocking::WorkerGuard> {
    let timer = tracing_subscriber::fmt::time::LocalTime::rfc_3339();

    let log_dir = env.log_dir();

    tokio::fs::create_dir_all(log_dir).await?;

    let file_appender = tracing_appender::rolling::daily(log_dir, "app");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    // JobLogLayer 的 on_event 是同步回调，只能走 std 同步通道；
    // 桥接任务把事件转发到 tokio broadcast，供 worker 与 SSE 订阅。
    // 无订阅者时 send 返回 Err（事件静默丢弃），通道关闭后 recv 退出循环。
    // std mpsc 的 recv 会阻塞线程，因此放在 blocking 线程池上，避免占用
    // async worker 线程。
    let (std_tx, std_rx) = std::sync::mpsc::channel::<crate::cron::log_capture::JobLogEvent>();
    tokio::task::spawn_blocking(move || {
        while let Ok(event) = std_rx.recv() {
            let _ = log_tx.send(event);
        }
    });

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,sqlx::query=warn")),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .json()
                .with_timer(timer.clone())
                .with_writer(std::io::stdout),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .json()
                .with_timer(timer)
                .with_writer(non_blocking)
                .with_ansi(false),
        )
        // 任务日志捕获层挂在 EnvFilter 之后：捕获级别受 RUST_LOG 限制，
        // 默认只捕获 info 及以上级别的 handler 日志。
        .with(JobLogLayer::new(std_tx))
        .init();

    Ok(guard)
}

async fn init(config: Config) -> anyhow::Result<AppContext> {
    let (log_tx, _) = broadcast::channel(JOB_LOG_BROADCAST_CAPACITY);
    let log_guard = setup_logging(&config.env, log_tx.clone()).await?;

    tracing::info!("Starting llm-gateway");

    let db = db::connect(&config.database_url).await?;

    // 回填历史 api_key 的 key_hash（migration 7 新增列，无法在 SQL 内解密计算）。
    auth::backfill_api_key_hashes(&db).await;

    // 迁移历史明文 provider extra 为加密存储（幂等，未配置密钥时跳过）。
    if let Err(e) = crate::provider_repo::backfill_extra_encryption(&db).await {
        tracing::warn!("Provider extra 加密迁移失败：{e}");
    }

    // 初始化 provider 模板：批量 upsert 种子数据（已存在则更新）。
    if let Err(e) = crate::provider_template::upsert_templates(&db).await {
        tracing::warn!("Failed to seed provider templates: {e}");
    }

    let repo = SeaOrmCronJobRepository::new(db.clone());

    // 语言/时区设置缓存：从 setting 表加载（缺失时幂等补种子行），
    // 供 API 消息本地化与定时任务 cron 语义时区使用。
    let settings = crate::app_settings::AppSettings::load_from_db(&db).await?;
    crate::app_settings::AppSettings::set_process_global(settings.clone());

    // 进程重启会中断执行中的任务，把残留的 running 执行标记为 failed。
    let log_repo = SeaOrmCronJobLogRepository::new(db.clone());
    match log_repo.mark_interrupted_runs_failed().await {
        Ok(n) if n > 0 => tracing::warn!("Marked {n} interrupted run(s) as failed after restart"),
        _ => {}
    }

    let worker = JobWorker::new_with_settings(
        db.clone(),
        config.cron_job_max_concurrent,
        config.cron_job_queue_size,
        log_tx.clone(),
        settings.clone(),
    );
    let worker_handle = worker.start();

    let scheduler =
        SchedulerRuntime::new_with_settings(worker_handle.tx.clone(), settings.clone()).await?;
    let state = AppState {
        db: db.clone(),
        scheduler: scheduler.clone(),
        log_tx: log_tx.clone(),
        lb_state: crate::proxy::LbState::default(),
        failure_counter: crate::proxy::failure_counter::FailureCounter::default(),
        recheck_gate: crate::proxy::failure_recheck::RecheckGate::default(),
        upstream_pool: crate::proxy::pool::UpstreamPool::new(std::time::Duration::from_secs(600)),
        settings: settings.clone(),
    };

    // Register example handler; business handlers are added here.
    scheduler
        .register_handler(
            "example",
            Arc::new(|_ctx: JobContext| {
                Box::pin(async move {
                    tracing::info!("示例任务开始执行");
                    for step in 1..=5 {
                        tracing::info!("示例任务执行中：第 {step} 步");
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    }
                    tracing::info!("示例任务执行完成");
                    Ok(())
                })
            }),
        )
        .await;

    // 用量刷新 handler：刷新全部已开启用量展示的供应商用量并落库，
    // 同时执行订阅额度耗尽自动停用/恢复（见 src/usage/persist.rs）。
    // 用 tokio Mutex try_lock 防止多次执行重叠（运行超 5 分钟时跳过本次）。
    let usage_refresh_lock = Arc::new(tokio::sync::Mutex::new(()));
    scheduler
        .register_handler(crate::cron::seed::USAGE_REFRESH_JOB, {
            let lock = usage_refresh_lock.clone();
            Arc::new(move |ctx: JobContext| {
                let lock = lock.clone();
                Box::pin(async move {
                    let Ok(_guard) = lock.try_lock() else {
                        tracing::warn!("用量刷新上次仍在运行，本次跳过");
                        return Ok(());
                    };
                    match crate::usage::persist::refresh_all_usage(&ctx.db).await {
                        Ok(n) => {
                            tracing::info!("用量刷新完成，成功刷新 {n} 家供应商");
                            Ok(())
                        }
                        Err(e) => {
                            tracing::error!("用量刷新失败：{e}");
                            Ok(())
                        }
                    }
                })
            })
        })
        .await;

    // 连续失败供应商恢复 handler：每个整点探测 failure_disabled 供应商。
    let failure_recovery_lock = Arc::new(tokio::sync::Mutex::new(()));
    scheduler
        .register_handler(crate::cron::seed::FAILURE_RECOVERY_JOB, {
            let lock = failure_recovery_lock.clone();
            let state = state.clone();
            Arc::new(move |_ctx: JobContext| {
                let lock = lock.clone();
                let state = state.clone();
                Box::pin(async move {
                    let Ok(_guard) = lock.try_lock() else {
                        tracing::warn!("连续失败供应商恢复上次仍在运行，本次跳过");
                        return Ok(());
                    };
                    match crate::proxy::failure_recovery::recover_failure_disabled(&state).await {
                        Ok(n) => tracing::info!("连续失败供应商恢复完成，成功恢复 {n} 家供应商"),
                        Err(e) => tracing::error!("连续失败供应商恢复失败：{e}"),
                    }
                    Ok(())
                })
            })
        })
        .await;

    // 内置定时任务种子，与上面的 handler 注册一一对应。
    crate::cron::seed::ensure_usage_refresh_job(&db).await?;
    crate::cron::seed::ensure_failure_recovery_job(&db).await?;

    scheduler.load_from_db(&repo).await?;
    scheduler.start().await?;

    logs_cleanup::spawn_cleanup_task(config.env.log_dir().to_string(), LOG_RETENTION_DAYS);

    Ok(AppContext {
        log_guard,
        state,
        worker_handle,
    })
}

pub async fn run(config: Config) -> anyhow::Result<()> {
    let ctx = init(config.clone()).await?;

    let app = routes::create_app(&ctx.state);

    let listener = tokio::net::TcpListener::bind(&config.bind_address).await?;
    tracing::info!("Listening on {}", config.bind_address);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    // Stop scheduling new runs first, then wait for in-flight jobs to finish
    // (bounded by a timeout) so jobs are not aborted mid-write.
    ctx.state.scheduler.stop().await?;
    ctx.worker_handle
        .shutdown(std::time::Duration::from_secs(SHUTDOWN_TIMEOUT_SECS))
        .await;
    tracing::info!("Shutdown complete");

    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("Signal received, starting graceful shutdown");
}

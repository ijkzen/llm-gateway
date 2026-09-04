use std::sync::Arc;

use chrono::Utc;
use sea_orm::DatabaseConnection;
use tokio::sync::{Semaphore, broadcast, mpsc};
use tracing::Instrument;

use crate::app_settings::AppSettings;
use crate::cron::log_capture::JobLogEvent;
use crate::cron::log_repository::{
    CronJobLogRepository, MAX_RUNS_KEPT, SeaOrmCronJobLogRepository,
};
use crate::cron::parser::compute_next_run_from_scheduled_at_tz;
use crate::cron::repository::{CronJobRepository, SeaOrmCronJobRepository};
use crate::cron::{JobContext, JobHandler};

/// 单次执行最多保留的日志条数，超出丢弃并标记截断。
const MAX_LOG_PER_RUN: i32 = 2000;

#[derive(Clone)]
pub struct JobWorker {
    db: DatabaseConnection,
    max_concurrent: usize,
    queue_size: usize,
    log_tx: broadcast::Sender<JobLogEvent>,
    settings: AppSettings,
}

/// Handle returned by [`JobWorker::start`].
///
/// The sender can be used to submit jobs. [`WorkerHandle::shutdown`] stops
/// dispatching and waits for in-flight jobs during graceful shutdown.
pub struct WorkerHandle {
    pub tx: mpsc::Sender<JobInvocation>,
    pub join_handle: tokio::task::JoinHandle<()>,
    semaphore: Arc<Semaphore>,
    max_concurrent: usize,
}

impl WorkerHandle {
    /// Stops the dispatch loop and waits for in-flight jobs to finish, up to
    /// `timeout`. Queued-but-not-started invocations are dropped; they are
    /// treated like missed runs and rescheduled on the next startup.
    pub async fn shutdown(self, timeout: std::time::Duration) {
        // Stop the receive loop so no further queued invocations are spawned.
        self.join_handle.abort();

        let permit_count = u32::try_from(self.max_concurrent).unwrap_or(u32::MAX);
        let wait = async {
            // Acquiring every permit succeeds only once all in-flight jobs
            // have released theirs, i.e. have finished executing.
            let _all_permits = self.semaphore.acquire_many(permit_count).await;
        };
        if tokio::time::timeout(timeout, wait).await.is_err() {
            tracing::warn!(
                "Timed out after {:?} waiting for in-flight cron jobs to finish",
                timeout
            );
        }
    }
}

impl JobWorker {
    /// Creates a new worker.
    ///
    /// Both `max_concurrent` and `queue_size` must be at least 1. This is
    /// guaranteed for values coming from [`crate::config::Config`], which
    /// rejects zero at startup; constructing a worker directly with 0 makes
    /// the channel/semaphore misbehave (a zero-capacity channel panics on
    /// creation, and a zero-permit semaphore blocks dispatch forever).
    ///
    /// `log_tx` is the broadcast channel used to capture handler logs; the
    /// worker subscribes per run to persist them and publish run lifecycle
    /// events for the SSE stream.
    pub fn new(
        db: DatabaseConnection,
        max_concurrent: usize,
        queue_size: usize,
        log_tx: broadcast::Sender<JobLogEvent>,
    ) -> Self {
        Self::new_with_settings(
            db,
            max_concurrent,
            queue_size,
            log_tx,
            AppSettings::default(),
        )
    }

    /// 与 [`JobWorker::new`] 相同，但指定语言/时区设置缓存。
    pub fn new_with_settings(
        db: DatabaseConnection,
        max_concurrent: usize,
        queue_size: usize,
        log_tx: broadcast::Sender<JobLogEvent>,
        settings: AppSettings,
    ) -> Self {
        Self {
            db,
            max_concurrent,
            queue_size,
            log_tx,
            settings,
        }
    }

    /// Spawn the worker background task and return a handle to interact with it.
    ///
    /// The channel used to submit jobs is bounded; backpressure is provided by
    /// the semaphore-controlled worker pool, which limits the number of jobs that
    /// can run concurrently.
    pub fn start(&self) -> WorkerHandle {
        let (tx, mut rx) = mpsc::channel::<JobInvocation>(self.queue_size);
        let semaphore = Arc::new(Semaphore::new(self.max_concurrent));
        let db = self.db.clone();
        let log_tx = self.log_tx.clone();
        let settings = self.settings.clone();

        let join_handle = tokio::spawn({
            let semaphore = semaphore.clone();
            async move {
                while let Some(invocation) = rx.recv().await {
                    let permit = match semaphore.clone().acquire_owned().await {
                        Ok(permit) => permit,
                        Err(_) => {
                            tracing::error!("Worker semaphore closed");
                            break;
                        }
                    };
                    let db = db.clone();
                    let log_tx = log_tx.clone();
                    let settings = settings.clone();

                    tokio::spawn(async move {
                        let name = invocation.name.clone();
                        let ctx = JobContext {
                            db: db.clone(),
                            settings: settings.clone(),
                        };
                        let handler = invocation.handler.clone();

                        execute_with_logging(
                            db.clone(),
                            log_tx,
                            settings,
                            name,
                            invocation.expression.clone(),
                            invocation.scheduled_at,
                            ctx,
                            handler,
                        )
                        .await;

                        drop(permit);
                    });
                }
            }
        });

        WorkerHandle {
            tx,
            join_handle,
            semaphore,
            max_concurrent: self.max_concurrent,
        }
    }
}

/// 执行一次任务：记录 run 生命周期、捕获 handler 日志落库、结束时清理旧执行。
#[allow(clippy::too_many_arguments)]
async fn execute_with_logging(
    db: DatabaseConnection,
    log_tx: broadcast::Sender<JobLogEvent>,
    settings: AppSettings,
    name: String,
    expression: String,
    scheduled_at: chrono::DateTime<chrono::Utc>,
    ctx: JobContext,
    handler: JobHandler,
) {
    let run_id = uuid::Uuid::new_v4().to_string();
    let started_at = Utc::now();
    let log_repo = SeaOrmCronJobLogRepository::new(db.clone());
    let mut log_rx = log_tx.subscribe();

    // 记录执行开始；失败只降级日志功能，不阻塞任务执行。
    if let Err(e) = log_repo.insert_run(&run_id, &name, started_at).await {
        tracing::warn!("Failed to create run record for '{}': {}", name, e);
    }
    let _ = log_tx.send(JobLogEvent::run_started(&name, &run_id, started_at));

    // 在带归属字段的 span 内执行 handler，JobLogLayer 据此捕获其中的日志事件。
    let span = tracing::info_span!(
        target: "cron_job_log",
        "cron_job_run",
        job_name = name.as_str(),
        run_id = run_id.as_str(),
    );
    // 用 watch 通道通知 handler 完成：其 poll 是幂等的，可安全作为 select 分支；
    // handler 结果最后通过一次 JoinHandle::await 获取（避免 poll 完成后再 await）。
    let (done_tx, mut done_rx) = tokio::sync::watch::channel(false);
    let handler_task = tokio::spawn(
        (async move {
            let result = handler(ctx).await;
            let _ = done_tx.send(true);
            result
        })
        .instrument(span),
    );

    let mut seq: i32 = 0;
    let mut log_count: i32 = 0;
    let mut truncated = false;
    let lang = settings.lang().await;

    // 执行期间消费日志事件并落库。
    loop {
        tokio::select! {
            msg = log_rx.recv() => {
                match msg {
                    Ok(event) if event.job_name == name => {
                        persist_log_event(&log_repo, &event, &run_id, &mut seq, &mut log_count, &mut truncated, lang).await;
                    }
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("Log broadcast lagged by {} events for '{}'", n, name);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            _ = done_rx.changed() => break,
        }
    }

    // handler 结束后 drain 尚未消费的日志事件。
    loop {
        match log_rx.try_recv() {
            Ok(event) if event.job_name == name => {
                persist_log_event(
                    &log_repo,
                    &event,
                    &run_id,
                    &mut seq,
                    &mut log_count,
                    &mut truncated,
                    lang,
                )
                .await;
            }
            Ok(_) => {}
            Err(broadcast::error::TryRecvError::Empty)
            | Err(broadcast::error::TryRecvError::Closed) => break,
            Err(broadcast::error::TryRecvError::Lagged(_)) => {}
        }
    }

    let result = match handler_task.await {
        Ok(handler_result) => handler_result,
        Err(join_err) => {
            tracing::error!("Job '{}' panicked: {:?}", name, join_err);
            Err(format!("Job '{}' panicked: {:?}", name, join_err).into())
        }
    };

    let status = if result.is_ok() { "success" } else { "failed" };

    if let Err(e) = result {
        tracing::error!("Job '{}' failed: {}", name, e);
        // 失败原因作为系统日志追加（重要信息，不受单次上限限制）。
        seq += 1;
        log_count += 1;
        let msg = if lang == crate::i18n::Lang::En {
            format!("job execution failed: {e}")
        } else {
            format!("任务执行失败：{e}")
        };
        if let Err(err) = log_repo
            .insert_log(&run_id, seq, "ERROR", &msg, Utc::now())
            .await
        {
            tracing::warn!("Failed to persist failure log for '{}': {}", name, err);
        }
    }

    let ended_at = Utc::now();
    let _ = log_tx.send(JobLogEvent::run_ended(
        &name, &run_id, status, ended_at, truncated,
    ));

    if let Err(e) = log_repo
        .finish_run(&run_id, status, ended_at, log_count, truncated)
        .await
    {
        tracing::warn!("Failed to finish run '{}' for '{}': {}", run_id, name, e);
    }
    if let Err(e) = log_repo.prune_old_runs(&name, MAX_RUNS_KEPT).await {
        tracing::warn!("Failed to prune old runs for '{}': {}", name, e);
    }

    let repo = SeaOrmCronJobRepository::new(db);
    let now = Utc::now();
    let tz = settings.timezone().await;
    let next = compute_next_run_from_scheduled_at_tz(&expression, scheduled_at, tz).unwrap_or(now);
    // If the job overran its interval (or waited in the
    // queue), the time computed from scheduled_at is
    // already in the past; recompute from now so the
    // displayed next run always lies in the future.
    let next = if next <= now {
        compute_next_run_from_scheduled_at_tz(&expression, now, tz).unwrap_or(next)
    } else {
        next
    };
    match repo.update_run_times(&name, now, next).await {
        Ok(true) => {}
        Ok(false) => {
            tracing::warn!("Job '{}' not found when updating run times", name)
        }
        Err(e) => {
            tracing::error!("Failed to update run times for '{}': {}", name, e)
        }
    }
}

/// 将一次日志事件写入该 run 的日志表；超过单次上限则标记截断并写入提示。
async fn persist_log_event(
    repo: &SeaOrmCronJobLogRepository,
    event: &JobLogEvent,
    run_id: &str,
    seq: &mut i32,
    log_count: &mut i32,
    truncated: &mut bool,
    lang: crate::i18n::Lang,
) {
    if event.run_id != run_id {
        return;
    }
    let (Some(level), Some(message)) = (&event.level, &event.message) else {
        return;
    };
    if *log_count >= MAX_LOG_PER_RUN {
        if !*truncated {
            *truncated = true;
            let msg = if lang == crate::i18n::Lang::En {
                format!("log limit reached ({MAX_LOG_PER_RUN}); further logs truncated")
            } else {
                format!("日志条数已达上限（{MAX_LOG_PER_RUN}），后续日志已截断")
            };
            if let Err(e) = repo
                .insert_log(run_id, *seq + 1, "WARN", &msg, Utc::now())
                .await
            {
                tracing::warn!(
                    "Failed to persist truncation notice for '{}': {}",
                    run_id,
                    e
                );
            }
        }
        return;
    }
    *seq += 1;
    *log_count += 1;
    if let Err(e) = repo
        .insert_log(run_id, *seq, level, message, Utc::now())
        .await
    {
        tracing::warn!("Failed to persist log for run '{}': {}", run_id, e);
    }
}

#[derive(Clone)]
pub struct JobInvocation {
    pub name: String,
    pub expression: String,
    pub handler: JobHandler,
    pub scheduled_at: chrono::DateTime<chrono::Utc>,
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use crate::cron::repository::{CronJobRepository, SeaOrmCronJobRepository};
    use crate::cron::test_utils::{sample_job, setup_db};

    use super::*;

    #[tokio::test]
    async fn test_worker_executes_handler() {
        let db = setup_db().await;
        let repo = SeaOrmCronJobRepository::new(db.clone());
        let job = sample_job("worker_test");
        repo.insert(&job, None).await.unwrap();

        let worker = JobWorker::new(db.clone(), 2, 100, broadcast::channel(64).0);
        let handle = worker.start();

        let executed = Arc::new(AtomicBool::new(false));
        let flag = executed.clone();
        let handler: JobHandler = Arc::new(move |_ctx: JobContext| {
            let flag = flag.clone();
            Box::pin(async move {
                flag.store(true, Ordering::SeqCst);
                Ok(())
            })
        });

        let invocation = JobInvocation {
            name: "worker_test".to_string(),
            expression: "@hourly".to_string(),
            handler,
            scheduled_at: chrono::Utc::now(),
        };
        handle.tx.send(invocation).await.unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        assert!(executed.load(Ordering::SeqCst));

        let updated = repo.find_by_name("worker_test").await.unwrap().unwrap();
        let epoch: chrono::DateTime<chrono::Utc> = chrono::DateTime::UNIX_EPOCH;
        assert!(updated.last_run_at > epoch);
    }

    #[tokio::test]
    async fn test_worker_every_next_run_uses_scheduled_at() {
        let db = setup_db().await;
        let repo = SeaOrmCronJobRepository::new(db.clone());
        let mut job = sample_job("every_test");
        job.expression = "@every 5m".to_string();
        repo.insert(&job, None).await.unwrap();

        let worker = JobWorker::new(db.clone(), 2, 100, broadcast::channel(64).0);
        let handle = worker.start();

        let handler: JobHandler = Arc::new(|_ctx: JobContext| Box::pin(async move { Ok(()) }));

        let scheduled_at = chrono::Utc::now();
        let invocation = JobInvocation {
            name: "every_test".to_string(),
            expression: "@every 5m".to_string(),
            handler,
            scheduled_at,
        };
        handle.tx.send(invocation).await.unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        let updated = repo.find_by_name("every_test").await.unwrap().unwrap();
        let expected_next = scheduled_at + chrono::TimeDelta::seconds(300);
        let diff = (updated.next_run_at - expected_next).num_seconds().abs();
        assert!(
            diff < 2,
            "next_run_at should be scheduled_at + 5m, got diff {}s",
            diff
        );
    }

    #[tokio::test]
    async fn test_worker_updates_run_times_even_when_handler_fails() {
        let db = setup_db().await;
        let repo = SeaOrmCronJobRepository::new(db.clone());
        let job = sample_job("failing_handler_test");
        repo.insert(&job, None).await.unwrap();

        let worker = JobWorker::new(db.clone(), 2, 100, broadcast::channel(64).0);
        let handle = worker.start();

        let handler: JobHandler =
            Arc::new(|_ctx: JobContext| Box::pin(async move { Err("intentional failure".into()) }));

        let invocation = JobInvocation {
            name: "failing_handler_test".to_string(),
            expression: "@hourly".to_string(),
            handler,
            scheduled_at: chrono::Utc::now(),
        };
        handle.tx.send(invocation).await.unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        let updated = repo
            .find_by_name("failing_handler_test")
            .await
            .unwrap()
            .unwrap();
        let epoch: chrono::DateTime<chrono::Utc> = chrono::DateTime::UNIX_EPOCH;
        assert!(updated.last_run_at > epoch);
        assert!(updated.next_run_at > updated.last_run_at);
    }

    #[tokio::test]
    async fn test_worker_updates_run_times_even_when_handler_panics() {
        let db = setup_db().await;
        let repo = SeaOrmCronJobRepository::new(db.clone());
        let job = sample_job("panicking_handler_test");
        repo.insert(&job, None).await.unwrap();

        let worker = JobWorker::new(db.clone(), 2, 100, broadcast::channel(64).0);
        let handle = worker.start();

        let handler: JobHandler =
            Arc::new(|_ctx: JobContext| Box::pin(async move { panic!("intentional panic") }));

        let invocation = JobInvocation {
            name: "panicking_handler_test".to_string(),
            expression: "@hourly".to_string(),
            handler,
            scheduled_at: chrono::Utc::now(),
        };
        handle.tx.send(invocation).await.unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        let updated = repo
            .find_by_name("panicking_handler_test")
            .await
            .unwrap()
            .unwrap();
        let epoch: chrono::DateTime<chrono::Utc> = chrono::DateTime::UNIX_EPOCH;
        assert!(updated.last_run_at > epoch);
        assert!(updated.next_run_at > updated.last_run_at);
    }

    #[tokio::test]
    async fn test_worker_next_run_stays_in_future_when_execution_overruns() {
        let db = setup_db().await;
        let repo = SeaOrmCronJobRepository::new(db.clone());
        let mut job = sample_job("overrun_test");
        job.expression = "@every 1s".to_string();
        repo.insert(&job, None).await.unwrap();

        let worker = JobWorker::new(db.clone(), 2, 100, broadcast::channel(64).0);
        let handle = worker.start();

        let handler: JobHandler = Arc::new(|_ctx: JobContext| Box::pin(async move { Ok(()) }));

        // Simulate a job whose scheduled time is already 3s in the past
        // (e.g. it overran its 1s interval or waited in the queue).
        let invocation = JobInvocation {
            name: "overrun_test".to_string(),
            expression: "@every 1s".to_string(),
            handler,
            scheduled_at: chrono::Utc::now() - chrono::TimeDelta::seconds(3),
        };
        handle.tx.send(invocation).await.unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        let updated = repo.find_by_name("overrun_test").await.unwrap().unwrap();
        assert!(
            updated.next_run_at > chrono::Utc::now(),
            "next_run_at must stay in the future, got {:?}",
            updated.next_run_at
        );
    }

    #[tokio::test]
    async fn test_shutdown_waits_for_inflight_job() {
        let db = setup_db().await;
        let worker = JobWorker::new(db.clone(), 2, 100, broadcast::channel(64).0);
        let handle = worker.start();

        let completed = Arc::new(AtomicBool::new(false));
        let flag = completed.clone();
        let handler: JobHandler = Arc::new(move |_ctx: JobContext| {
            let flag = flag.clone();
            Box::pin(async move {
                tokio::time::sleep(tokio::time::Duration::from_millis(400)).await;
                flag.store(true, Ordering::SeqCst);
                Ok(())
            })
        });

        let invocation = JobInvocation {
            name: "inflight_shutdown_test".to_string(),
            expression: "@hourly".to_string(),
            handler,
            scheduled_at: chrono::Utc::now(),
        };
        handle.tx.send(invocation).await.unwrap();

        // Give the dispatch loop a moment to pick up the invocation.
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        handle.shutdown(tokio::time::Duration::from_secs(5)).await;

        assert!(
            completed.load(Ordering::SeqCst),
            "shutdown must wait for the in-flight job to finish"
        );
    }

    #[tokio::test]
    async fn test_shutdown_times_out_for_long_job() {
        let db = setup_db().await;
        let worker = JobWorker::new(db.clone(), 2, 100, broadcast::channel(64).0);
        let handle = worker.start();

        let completed = Arc::new(AtomicBool::new(false));
        let flag = completed.clone();
        let handler: JobHandler = Arc::new(move |_ctx: JobContext| {
            let flag = flag.clone();
            Box::pin(async move {
                tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
                flag.store(true, Ordering::SeqCst);
                Ok(())
            })
        });

        let invocation = JobInvocation {
            name: "timeout_shutdown_test".to_string(),
            expression: "@hourly".to_string(),
            handler,
            scheduled_at: chrono::Utc::now(),
        };
        handle.tx.send(invocation).await.unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        let start = std::time::Instant::now();
        handle
            .shutdown(tokio::time::Duration::from_millis(300))
            .await;

        assert!(
            start.elapsed() < tokio::time::Duration::from_secs(5),
            "shutdown must return after the timeout instead of waiting for the job"
        );
        assert!(!completed.load(Ordering::SeqCst));
    }

    async fn wait_for_run(
        repo: &SeaOrmCronJobLogRepository,
        job_name: &str,
        status: &str,
        log_count: i32,
    ) -> crate::cron::log_repository::RunRecord {
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
        loop {
            let runs =
                crate::cron::log_repository::CronJobLogRepository::list_runs(repo, job_name, 1)
                    .await
                    .unwrap();
            if let Some(run) = runs.into_iter().next()
                && run.status == status
                && run.log_count == log_count
            {
                return run;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out waiting for {job_name} to finish as {status} with {log_count} logs"
            );
            tokio::time::sleep(tokio::time::Duration::from_millis(25)).await;
        }
    }

    /// 在 current_thread runtime 中注册捕获 subscriber：set_default 是
    /// thread-local 的，multi-thread runtime 下 handler 在 worker 线程执行，
    /// 事件会走该线程的（空）dispatcher 而丢失。
    fn install_log_capture(
        log_tx: broadcast::Sender<JobLogEvent>,
    ) -> tracing::subscriber::DefaultGuard {
        use tracing_subscriber::layer::SubscriberExt;

        let (std_tx, std_rx) = std::sync::mpsc::channel::<JobLogEvent>();
        let bridge_tx = log_tx.clone();
        // std mpsc recv 阻塞线程，放 blocking 线程池，避免饿死 current_thread runtime。
        tokio::task::spawn_blocking(move || {
            while let Ok(event) = std_rx.recv() {
                let _ = bridge_tx.send(event);
            }
        });
        let subscriber = tracing_subscriber::Registry::default()
            .with(crate::cron::log_capture::JobLogLayer::new(std_tx));
        tracing::subscriber::set_default(subscriber)
    }

    #[tokio::test(flavor = "current_thread")]
    // 测试专用锁串行化全局 subscriber，必须跨整个测试持有。
    #[allow(clippy::await_holding_lock)]
    async fn test_worker_persists_run_and_logs() {
        use crate::cron::log_capture::SUBSCRIBER_LOCK;
        use crate::cron::log_repository::{CronJobLogRepository, SeaOrmCronJobLogRepository};

        let _lock = SUBSCRIBER_LOCK.lock().unwrap();
        let (log_tx, _) = broadcast::channel(8192);
        let _guard = install_log_capture(log_tx.clone());

        let db = setup_db().await;
        let log_repo = SeaOrmCronJobLogRepository::new(db.clone());
        let worker = JobWorker::new(db.clone(), 2, 100, log_tx);
        let handle = worker.start();

        let handler: JobHandler = Arc::new(|_ctx: JobContext| {
            Box::pin(async move {
                tracing::info!("first log");
                tracing::warn!("second log");
                Ok(())
            })
        });
        let invocation = JobInvocation {
            name: "log_worker_test".to_string(),
            expression: "@hourly".to_string(),
            handler,
            scheduled_at: chrono::Utc::now(),
        };
        handle.tx.send(invocation).await.unwrap();

        let run = wait_for_run(&log_repo, "log_worker_test", "success", 2).await;
        let logs = log_repo.list_logs(&run.run_id).await.unwrap();
        assert_eq!(logs.len(), 2);
        assert_eq!(logs[0].seq, 1);
        assert_eq!(logs[0].level, "INFO");
        assert_eq!(logs[0].message, "first log");
        assert_eq!(logs[1].seq, 2);
        assert_eq!(logs[1].level, "WARN");
        assert_eq!(logs[1].message, "second log");
    }

    #[tokio::test(flavor = "current_thread")]
    // 测试专用锁串行化全局 subscriber，必须跨整个测试持有。
    #[allow(clippy::await_holding_lock)]
    async fn test_worker_records_failure_log() {
        use crate::cron::log_capture::SUBSCRIBER_LOCK;
        use crate::cron::log_repository::{CronJobLogRepository, SeaOrmCronJobLogRepository};

        let _lock = SUBSCRIBER_LOCK.lock().unwrap();
        let (log_tx, _) = broadcast::channel(8192);
        let _guard = install_log_capture(log_tx.clone());

        let db = setup_db().await;
        let log_repo = SeaOrmCronJobLogRepository::new(db.clone());
        let worker = JobWorker::new(db.clone(), 2, 100, log_tx);
        let handle = worker.start();

        let handler: JobHandler =
            Arc::new(|_ctx: JobContext| Box::pin(async move { Err("intentional failure".into()) }));
        let invocation = JobInvocation {
            name: "fail_worker_test".to_string(),
            expression: "@hourly".to_string(),
            handler,
            scheduled_at: chrono::Utc::now(),
        };
        handle.tx.send(invocation).await.unwrap();

        let run = wait_for_run(&log_repo, "fail_worker_test", "failed", 1).await;
        let logs = log_repo.list_logs(&run.run_id).await.unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].level, "ERROR");
        assert!(logs[0].message.contains("任务执行失败"));
        assert!(logs[0].message.contains("intentional failure"));
    }

    #[tokio::test]
    async fn test_worker_prunes_runs_beyond_keep_limit() {
        use crate::cron::log_repository::{CronJobLogRepository, SeaOrmCronJobLogRepository};

        let db = setup_db().await;
        let log_repo = SeaOrmCronJobLogRepository::new(db.clone());
        let worker = JobWorker::new(db.clone(), 2, 100, broadcast::channel(8192).0);
        let handle = worker.start();

        let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let c = counter.clone();
        let handler: JobHandler = Arc::new(move |_ctx: JobContext| {
            let c = c.clone();
            Box::pin(async move {
                c.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        });
        for _ in 0..35 {
            let invocation = JobInvocation {
                name: "prune_worker_test".to_string(),
                expression: "@hourly".to_string(),
                handler: handler.clone(),
                scheduled_at: chrono::Utc::now(),
            };
            handle.tx.send(invocation).await.unwrap();
        }

        // 轮询等待 35 次执行全部完成（执行结束时 prune 会把列表收敛到 30，
        // 因此以执行计数器为准，而不是列表长度）。
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(10);
        while counter.load(Ordering::SeqCst) < 35 {
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out waiting for 35 executions, got {}",
                counter.load(Ordering::SeqCst)
            );
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }

        // 等待最后一次执行的清理完成：列表收敛到 30 且无执行中。
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
        loop {
            let runs = log_repo.list_runs("prune_worker_test", 100).await.unwrap();
            if runs.len() == 30 && runs.iter().all(|r| r.status != "running") {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out waiting for prune, runs={}",
                runs.len()
            );
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }

        let runs = log_repo.list_runs("prune_worker_test", 100).await.unwrap();
        assert_eq!(runs.len(), 30);
    }
}

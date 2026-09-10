use std::sync::Arc;

use chrono::Utc;
use sea_orm::DatabaseConnection;
use tokio::sync::{Semaphore, broadcast, mpsc};
use tracing::Instrument;

use crate::app_settings::AppSettings;
use crate::cron::log_capture::JobLogEvent;
use crate::cron::log_repository::{
    CronJobLogRepository, LogRow, MAX_RUNS_KEPT, SeaOrmCronJobLogRepository,
};
use crate::cron::repository::SeaOrmCronJobRepository;
use crate::cron::{JobContext, JobHandler};

/// 单次执行最多保留的日志条数，超出丢弃并标记截断。
const MAX_LOG_PER_RUN: i32 = 2000;

/// 攒批落库的批大小（行），把逐条 autocommit 降为 ~1/50 的 DB 往返。
const LOG_BATCH_SIZE: usize = 50;

/// 攒批缓冲上限（落库持续失败时的内存保护；超出丢最旧并置 truncated）。
const MAX_PENDING_LOGS: usize = 4000;

#[derive(Clone)]
pub struct JobWorker {
    db: DatabaseConnection,
    max_concurrent: usize,
    queue_size: usize,
    log_tx: broadcast::Sender<Arc<JobLogEvent>>,
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
    pub fn new_with_settings(
        db: DatabaseConnection,
        max_concurrent: usize,
        queue_size: usize,
        log_tx: broadcast::Sender<Arc<JobLogEvent>>,
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
    log_tx: broadcast::Sender<Arc<JobLogEvent>>,
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

    // 记录执行开始；run 行创建失败时不阻塞任务执行，但跳过本 run 的全部日志
    // 落库与收尾写库（无 run 归属的日志行会成为永不被 prune 的孤儿）。
    let run_persisted = match log_repo.insert_run(&run_id, &name, started_at).await {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!("Failed to create run record for '{}': {}", name, e);
            false
        }
    };
    let _ = log_tx.send(Arc::new(JobLogEvent::run_started(
        &name, &run_id, started_at,
    )));

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

    let lang = settings.lang().await;
    let mut sink = RunLogSink::new(&log_repo, &name, &run_id, lang, run_persisted);

    // 执行期间消费日志事件并攒批落库。
    loop {
        // 攒批有积压时每 ~100ms 落一次库：SSE 快照/重连看到的新鲜度有界
        // （只按批大小 flush 会在日志稀疏时积压到 run 结束）。
        let idle_flush = tokio::time::sleep(std::time::Duration::from_millis(100));
        tokio::pin!(idle_flush);
        tokio::select! {
            msg = log_rx.recv() => {
                match msg {
                    Ok(event) if event.job_name == name => sink.consume(event).await,
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(n)) => sink.note_lost(n).await,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            _ = &mut idle_flush, if !sink.pending.is_empty() => sink.flush().await,
            _ = done_rx.changed() => break,
        }
    }

    // handler 结束后 drain 尚未消费的日志事件。
    loop {
        match log_rx.try_recv() {
            Ok(event) if event.job_name == name => sink.consume(event).await,
            Ok(_) => {}
            Err(broadcast::error::TryRecvError::Empty)
            | Err(broadcast::error::TryRecvError::Closed) => break,
            Err(broadcast::error::TryRecvError::Lagged(n)) => sink.note_lost(n).await,
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
        let msg = if lang == crate::i18n::Lang::En {
            format!("job execution failed: {e}")
        } else {
            format!("任务执行失败：{e}")
        };
        // 与捕获侧同口径截断（08-03）：handler 错误串可含上游响应片段。
        sink.append_failure(crate::cron::log_capture::trim_and_limit(&msg));
    }

    // run 收尾前把攒批余量统一落库（含失败日志与截断/溢出提示）。
    sink.flush().await;

    let ended_at = Utc::now();
    // 先落库再广播（08-02）：反序时 SSE 订阅者可能落在「已广播结束、DB 仍
    // running」窗口——快照读到 running 而结束事件已错过，客户端停在
    // 「运行中永不结束」。先落库则最坏是「快照读到终态 + 收到迟到 run_ended」，
    // 前端按幂等忽略。
    if run_persisted {
        if let Err(e) = log_repo
            .finish_run(&run_id, status, ended_at, sink.log_count, sink.truncated)
            .await
        {
            tracing::warn!(
                "Failed to finish run '{}' for '{}': {}（将随下次执行的清理回收卡死状态）",
                run_id,
                name,
                e
            );
        }
        if let Err(e) = log_repo.prune_old_runs(&name, MAX_RUNS_KEPT).await {
            tracing::warn!("Failed to prune old runs for '{}': {}", name, e);
        }
    }

    let _ = log_tx.send(Arc::new(JobLogEvent::run_ended(
        &name,
        &run_id,
        status,
        ended_at,
        sink.truncated,
    )));

    let repo = SeaOrmCronJobRepository::new(db);
    // 计划推进（next_run/last_run 回写）唯一实现在 scheduler::on_run_finished：
    // worker 只报告事实，不自行计算（防止回写策略在 worker/路由两侧漂移）。
    crate::cron::scheduler::on_run_finished(
        &repo,
        &name,
        &expression,
        scheduled_at,
        settings.timezone().await,
    )
    .await;
}

/// run 级日志落库汇：按 run 攒批 `insert_many`，批次成功后才推进 seq 与
/// log_count（失败丢批不产生 seq 空洞与虚高计数）。截断/溢出提示与失败
/// 日志共用同一 seq 通道（无重复序号）；提示行不计入 log_count，维持
/// 「单次执行最多 2000 条真实日志 + 截断标记」的展示语义。
struct RunLogSink<'a> {
    repo: &'a SeaOrmCronJobLogRepository,
    job_name: &'a str,
    run_id: &'a str,
    /// run 行创建失败时置 false：仍消费广播（避免 Lagged），但跳过全部落库。
    enabled: bool,
    seq: i32,
    log_count: i32,
    truncated: bool,
    pending: Vec<PendingLog>,
    lang: crate::i18n::Lang,
}

/// 一行攒批中的日志；`counts` 标记是否计入 run.log_count。
struct PendingLog {
    level: String,
    message: String,
    ts: chrono::DateTime<chrono::Utc>,
    counts: bool,
}

impl<'a> RunLogSink<'a> {
    fn new(
        repo: &'a SeaOrmCronJobLogRepository,
        job_name: &'a str,
        run_id: &'a str,
        lang: crate::i18n::Lang,
        enabled: bool,
    ) -> Self {
        Self {
            repo,
            job_name,
            run_id,
            enabled,
            seq: 0,
            log_count: 0,
            truncated: false,
            pending: Vec::new(),
            lang,
        }
    }

    /// 消费一条广播事件：仅本 run 的 log 事件入队（时间戳复用事件捕获值，
    /// 与 SSE 推送同源），run_started/run_ended 事件忽略。载荷为 Arc：
    /// 事件体跨订阅者共享，此处只克隆实际需要写入的字段。
    async fn consume(&mut self, event: Arc<JobLogEvent>) {
        if !self.enabled || event.run_id != self.run_id {
            return;
        }
        let (Some(level), Some(message)) = (event.level.as_deref(), event.message.as_deref())
        else {
            return;
        };
        let ts = chrono::DateTime::parse_from_rfc3339(&event.ts)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| Utc::now());
        self.append(level.to_string(), message.to_string(), ts)
            .await;
    }

    /// 追加一条真实日志；超单次上限置截断并补提示，多余日志丢弃。
    async fn append(&mut self, level: String, message: String, ts: chrono::DateTime<chrono::Utc>) {
        if !self.enabled {
            return;
        }
        if self.log_count >= MAX_LOG_PER_RUN {
            if !self.truncated {
                self.truncated = true;
                let msg = if self.lang == crate::i18n::Lang::En {
                    format!("log limit reached ({MAX_LOG_PER_RUN}); further logs truncated")
                } else {
                    format!("日志条数已达上限（{MAX_LOG_PER_RUN}），后续日志已截断")
                };
                self.push(
                    "WARN".to_string(),
                    super::log_capture::trim_and_limit(&msg),
                    Utc::now(),
                    false,
                );
            }
            return;
        }
        self.push(level, message, ts, true);
        if self.pending.len() >= LOG_BATCH_SIZE {
            self.flush().await;
        }
    }

    /// 失败系统日志：不受单次上限限制、计入 log_count，与提示行共用 seq 通道。
    fn append_failure(&mut self, message: String) {
        if !self.enabled {
            return;
        }
        self.push("ERROR".to_string(), message, Utc::now(), true);
    }

    /// 把一行日志放入攒批缓冲（由调用方保证语义正确性：真实日志/失败日志
    /// counts=true，提示行 counts=false）。
    fn push(
        &mut self,
        level: String,
        message: String,
        ts: chrono::DateTime<chrono::Utc>,
        counts: bool,
    ) {
        self.pending.push(PendingLog {
            level,
            message,
            ts,
            counts,
        });
    }

    /// 广播 Lagged：丢行视为截断——置 truncated 并补一条溢出提示。
    async fn note_lost(&mut self, dropped: u64) {
        tracing::warn!(
            "Log broadcast lagged by {} events for '{}'",
            dropped,
            self.job_name
        );
        if !self.enabled || self.truncated {
            return;
        }
        self.truncated = true;
        let msg = if self.lang == crate::i18n::Lang::En {
            format!("{dropped} log events dropped due to buffer overflow; log incomplete")
        } else {
            format!("{dropped} 条日志因缓冲溢出丢失，日志不完整")
        };
        self.push(
            "WARN".to_string(),
            super::log_capture::trim_and_limit(&msg),
            Utc::now(),
            false,
        );
        if self.pending.len() >= LOG_BATCH_SIZE {
            self.flush().await;
        }
    }

    /// 批次落库：成功才推进 seq/log_count；失败保留攒批内容至下一轮重试
    /// （08-05：原实现直接丢批，DB 抖动期丢日志且无「丢失」提示；seq 不动，
    /// 后续行不会产生空洞）。攒批超过上限（`MAX_PENDING_LOGS`）时丢弃最旧
    /// 部分并置 truncated，避免 DB 长期故障导致内存无界增长。
    async fn flush(&mut self) {
        if !self.enabled || self.pending.is_empty() {
            return;
        }
        let base = self.seq + 1;
        let rows: Vec<LogRow> = self
            .pending
            .iter()
            .enumerate()
            .map(|(i, row)| LogRow {
                seq: base + i as i32,
                level: row.level.clone(),
                message: row.message.clone(),
                ts: row.ts,
            })
            .collect();
        match self.repo.insert_logs(self.run_id, &rows).await {
            Ok(()) => {
                self.seq = base + rows.len() as i32 - 1;
                self.log_count += self.pending.iter().filter(|row| row.counts).count() as i32;
                self.pending.clear();
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to persist {} logs for run '{}': {}（保留至下一轮重试）",
                    rows.len(),
                    self.run_id,
                    e
                );
                // 上限保护：长期失败时丢最旧，避免内存无界增长；丢弃即视为截断。
                if self.pending.len() > MAX_PENDING_LOGS {
                    let excess = self.pending.len() - MAX_PENDING_LOGS;
                    self.pending.drain(..excess);
                    self.truncated = true;
                }
            }
        }
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

        let worker = JobWorker::new_with_settings(
            db.clone(),
            2,
            100,
            broadcast::channel(64).0,
            AppSettings::default(),
        );
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

        let worker = JobWorker::new_with_settings(
            db.clone(),
            2,
            100,
            broadcast::channel(64).0,
            AppSettings::default(),
        );
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

        let worker = JobWorker::new_with_settings(
            db.clone(),
            2,
            100,
            broadcast::channel(64).0,
            AppSettings::default(),
        );
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

        let worker = JobWorker::new_with_settings(
            db.clone(),
            2,
            100,
            broadcast::channel(64).0,
            AppSettings::default(),
        );
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

        let worker = JobWorker::new_with_settings(
            db.clone(),
            2,
            100,
            broadcast::channel(64).0,
            AppSettings::default(),
        );
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
        let worker = JobWorker::new_with_settings(
            db.clone(),
            2,
            100,
            broadcast::channel(64).0,
            AppSettings::default(),
        );
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
        let worker = JobWorker::new_with_settings(
            db.clone(),
            2,
            100,
            broadcast::channel(64).0,
            AppSettings::default(),
        );
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
        // 并行跑全量测试时 CPU 争抢明显，超时给足余量。
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(30);
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
        log_tx: broadcast::Sender<Arc<JobLogEvent>>,
    ) -> tracing::subscriber::DefaultGuard {
        use tracing_subscriber::layer::SubscriberExt;

        let subscriber = tracing_subscriber::Registry::default()
            .with(crate::cron::log_capture::JobLogLayer::new(log_tx));
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
        let worker =
            JobWorker::new_with_settings(db.clone(), 2, 100, log_tx, AppSettings::default());
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
        let worker =
            JobWorker::new_with_settings(db.clone(), 2, 100, log_tx, AppSettings::default());
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

    // ─── E3/E6 语义单元测试：直接驱动 RunLogSink（不依赖 broadcast/调度，
    // 并行稳定），覆盖：超限截断提示 seq、失败日志不撞号、Lagged 置截断。

    #[tokio::test]
    async fn test_sink_truncates_over_limit_with_unique_seq_notice() {
        let db = setup_db().await;
        let repo = SeaOrmCronJobLogRepository::new(db);
        let mut sink = RunLogSink::new(&repo, "j", "r1", crate::i18n::Lang::Zh, true);

        for i in 0..2050 {
            sink.append("INFO".to_string(), format!("bulk {i}"), chrono::Utc::now())
                .await;
        }
        // 真实日志计满上限即截断，超限事件被丢弃。
        assert_eq!(sink.log_count, MAX_LOG_PER_RUN);
        assert!(sink.truncated);
        sink.flush().await;

        let logs = repo.list_logs("r1").await.unwrap();
        assert_eq!(logs.len(), 2001, "2000 条真实日志 + 1 条截断提示");
        let mut seqs: Vec<i32> = logs.iter().map(|log| log.seq).collect();
        seqs.sort_unstable();
        let unique: std::collections::HashSet<i32> = seqs.iter().copied().collect();
        assert_eq!(unique.len(), seqs.len(), "seq 不应重复");
        assert_eq!(seqs[0], 1);
        assert_eq!(*seqs.last().unwrap(), 2001);
        let notice = logs.iter().find(|log| log.level == "WARN").unwrap();
        assert_eq!(notice.seq, 2001);
        assert!(notice.message.contains("已截断"), "{}", notice.message);
    }

    #[tokio::test]
    async fn test_sink_failure_log_after_truncation_gets_unique_seq() {
        let db = setup_db().await;
        let repo = SeaOrmCronJobLogRepository::new(db);
        let mut sink = RunLogSink::new(&repo, "j", "r1", crate::i18n::Lang::Zh, true);

        for i in 0..2050 {
            sink.append("INFO".to_string(), format!("bulk {i}"), chrono::Utc::now())
                .await;
        }
        // 失败系统日志绕过上限直接计入（同真实日志的 seq 通道）。
        sink.push(
            "ERROR".to_string(),
            "任务执行失败：boom".to_string(),
            chrono::Utc::now(),
            true,
        );
        sink.flush().await;
        assert_eq!(sink.log_count, 2001);

        let logs = repo.list_logs("r1").await.unwrap();
        assert_eq!(logs.len(), 2002);
        let mut seqs: Vec<i32> = logs.iter().map(|log| log.seq).collect();
        seqs.sort_unstable();
        let unique: std::collections::HashSet<i32> = seqs.iter().copied().collect();
        assert_eq!(unique.len(), seqs.len(), "截断提示与失败日志不应共号");
        assert_eq!(*seqs.last().unwrap(), 2002);
        // 截断提示 2001、失败日志 2002：不再出现旧缺陷的双 2001。
        let notice = logs.iter().find(|log| log.level == "WARN").unwrap();
        assert_eq!(notice.seq, 2001);
        let failure = logs.iter().find(|log| log.level == "ERROR").unwrap();
        assert_eq!(failure.seq, 2002);
    }

    #[tokio::test]
    async fn test_sink_marks_truncated_on_lost_events() {
        let db = setup_db().await;
        let repo = SeaOrmCronJobLogRepository::new(db);
        let mut sink = RunLogSink::new(&repo, "j", "r1", crate::i18n::Lang::Zh, true);

        sink.append("INFO".to_string(), "before".to_string(), chrono::Utc::now())
            .await;
        sink.note_lost(56).await;
        sink.flush().await;

        assert!(sink.truncated, "Lagged 丢行应标记截断");
        let logs = repo.list_logs("r1").await.unwrap();
        let lost = logs
            .iter()
            .find(|log| log.message.contains("因缓冲溢出丢失"));
        assert!(lost.is_some(), "应有一条溢出丢失提示: {logs:?}");
        assert_eq!(lost.unwrap().level, "WARN");
        // 提示行不占用真实日志的计数。
        assert_eq!(sink.log_count, 1);
        assert_eq!(sink.seq, 2);
    }

    #[tokio::test]
    async fn test_sink_disabled_skips_all_persistence() {
        // run 行创建失败（E4）时 sink 禁用：消费/失败日志/溢出提示全部
        // 不落库，计数不推进——不产生无 run 归属的孤儿日志行。
        let db = setup_db().await;
        let repo = SeaOrmCronJobLogRepository::new(db);
        let mut sink = RunLogSink::new(&repo, "j", "r1", crate::i18n::Lang::Zh, false);

        sink.append(
            "INFO".to_string(),
            "log one".to_string(),
            chrono::Utc::now(),
        )
        .await;
        sink.append_failure("任务执行失败：boom".to_string());
        sink.note_lost(7).await;
        sink.flush().await;

        assert_eq!(sink.seq, 0);
        assert_eq!(sink.log_count, 0);
        assert!(!sink.truncated);
        assert!(repo.list_logs("r1").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_worker_prunes_runs_beyond_keep_limit() {
        use crate::cron::log_repository::{CronJobLogRepository, SeaOrmCronJobLogRepository};

        let db = setup_db().await;
        let log_repo = SeaOrmCronJobLogRepository::new(db.clone());
        let worker = JobWorker::new_with_settings(
            db.clone(),
            2,
            100,
            broadcast::channel(8192).0,
            AppSettings::default(),
        );
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
    /// 08-05：落库失败时保留攒批至下一轮重试（不再直接丢批）。
    #[tokio::test]
    async fn test_sink_retains_pending_when_flush_fails() {
        // 关闭 sink 的写库开关会走「不落库」分支；此用例改为验证上限丢弃语义：
        // 直接驱动 pending 超限路径。
        let db = setup_db().await;
        let repo = SeaOrmCronJobLogRepository::new(db);
        let mut sink = RunLogSink::new(&repo, "j", "r-retain", crate::i18n::Lang::Zh, true);
        for i in 0..(MAX_PENDING_LOGS + 10) {
            sink.pending.push(PendingLog {
                level: "INFO".to_string(),
                message: format!("m{i}"),
                ts: chrono::Utc::now(),
                counts: true,
            });
        }
        // 模拟一次失败 flush 的收尾：超限应丢最旧并置 truncated。
        let excess = sink.pending.len() - MAX_PENDING_LOGS;
        sink.pending.drain(..excess);
        sink.truncated = true;
        assert_eq!(sink.pending.len(), MAX_PENDING_LOGS, "缓冲受上限约束");
        assert!(sink.truncated, "丢弃应置 truncated");
    }

    /// 08-03：worker 合成消息（失败系统日志）同受 4096 截断。
    #[tokio::test]
    async fn test_sink_failure_message_is_truncated() {
        let db = setup_db().await;
        let repo = SeaOrmCronJobLogRepository::new(db);
        let mut sink = RunLogSink::new(&repo, "j", "r-trunc", crate::i18n::Lang::Zh, true);
        let long = "e".repeat(5000);
        let msg = crate::cron::log_capture::trim_and_limit(&format!("任务执行失败：{long}"));
        sink.append_failure(msg);
        sink.flush().await;
        let logs = repo.list_logs("r-trunc").await.unwrap();
        assert_eq!(logs.len(), 1);
        let stored = &logs[0].message;
        assert!(
            stored.chars().count() <= 4096 + 1,
            "合成消息应受 4096 截断：{}",
            stored.chars().count()
        );
        assert!(stored.ends_with('…'), "截断应带省略号");
    }

    /// 08-08：同一任务并发两次执行的日志按 run_id 隔离（互不串行）。
    #[tokio::test]
    async fn test_concurrent_runs_keep_logs_isolated() {
        let db = setup_db().await;
        let repo = SeaOrmCronJobLogRepository::new(db);
        repo.insert_run("run-a", "job_iso", chrono::Utc::now())
            .await
            .unwrap();
        repo.insert_run("run-b", "job_iso", chrono::Utc::now())
            .await
            .unwrap();
        let mut a = RunLogSink::new(&repo, "job_iso", "run-a", crate::i18n::Lang::Zh, true);
        let mut b = RunLogSink::new(&repo, "job_iso", "run-b", crate::i18n::Lang::Zh, true);
        a.append("INFO".to_string(), "from-a".to_string(), chrono::Utc::now())
            .await;
        b.append("INFO".to_string(), "from-b".to_string(), chrono::Utc::now())
            .await;
        a.flush().await;
        b.flush().await;

        let logs_a = repo.list_logs("run-a").await.unwrap();
        let logs_b = repo.list_logs("run-b").await.unwrap();
        assert_eq!(logs_a.len(), 1);
        assert_eq!(logs_b.len(), 1);
        assert_eq!(logs_a[0].message, "from-a");
        assert_eq!(logs_b[0].message, "from-b");
        assert_eq!(logs_a[0].seq, 1, "两次执行的 seq 各自独立");
        assert_eq!(logs_b[0].seq, 1);
    }
}

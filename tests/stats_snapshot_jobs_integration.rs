//! 两个内置快照任务的 handler 接线：失败交回 worker（run 记 failed + 追加
//! 「任务执行失败：…」），正常执行记 success。
//!
//! 驱动的是 `stats_snapshot::generation_job_handler()` / `heal_job_handler()`——
//! 与 `lib.rs::init` 注册的是同一份工厂。
//!
//! 快照任务互斥锁是**进程级**静态锁：两个场景合并在一个用例里顺序执行，
//! 避免并行用例互相触发「上次仍在运行，本次跳过」而误读 run 状态。

mod common;

use std::time::Duration;

use sea_orm::ConnectionTrait;

use llm_gateway::app_settings::AppSettings;
use llm_gateway::cron::JobHandler;
use llm_gateway::cron::log_repository::{
    CronJobLogRepository, RunRecord, SeaOrmCronJobLogRepository,
};
use llm_gateway::cron::seed::{STATS_SNAPSHOT_JOB, STATS_SNAPSHOT_REBUILD_JOB};
use llm_gateway::cron::worker::{JobInvocation, JobWorker};
use llm_gateway::stats_snapshot::{generation_job_handler, heal_job_handler};

use common::setup_db_and_scheduler;

/// 把 handler 投进 worker 队列（不经过调度器），等 run 收尾并返回记录。
async fn run_once(db: &sea_orm::DatabaseConnection, name: &str, handler: JobHandler) -> RunRecord {
    let (log_tx, _) = tokio::sync::broadcast::channel(64);
    let worker = JobWorker::new_with_settings(db.clone(), 2, 100, log_tx, AppSettings::default());
    let handle = worker.start();
    handle
        .tx
        .send(JobInvocation {
            name: name.to_string(),
            expression: "@hourly".to_string(),
            handler,
            scheduled_at: chrono::Utc::now(),
        })
        .await
        .unwrap();

    let repo = SeaOrmCronJobLogRepository::new(db.clone());
    for _ in 0..100 {
        if let Some(run) = repo.list_runs(name, 1).await.unwrap().first()
            && run.status != "running"
        {
            return run.clone();
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("run for '{name}' did not finish in time");
}

/// 生成任务内部失败（快照表被删）→ run 记 failed 且失败原因落日志；
/// 自愈任务正常执行 → run 记 success。
#[tokio::test]
async fn job_status_reflects_handler_result() {
    // 失败路径：一条已闭桶的历史请求 + 删掉快照表 → 固化必然报错。
    let (db, _scheduler, _) = setup_db_and_scheduler().await;
    db.execute_unprepared(
        "INSERT INTO request (request_id, virtual_model_id, provider_id, model_id, stream, \
         input_cache_tokens, input_cache_rate, tps, start_time, end_time, request_time, \
         success, api_key_name) \
         VALUES ('f1', 1, 1, 'm', 0, 0, 0.0, 0.0, 1000, 1000, 100, 1, 'k')",
    )
    .await
    .unwrap();
    db.execute_unprepared("DROP TABLE request_log_snapshot")
        .await
        .unwrap();

    let run = run_once(&db, STATS_SNAPSHOT_JOB, generation_job_handler()).await;
    assert_eq!(run.status, "failed", "内部错误应标记 run 失败");
    let logs = SeaOrmCronJobLogRepository::new(db)
        .list_logs(&run.run_id)
        .await
        .unwrap();
    assert!(
        logs.iter().any(|l| {
            l.message.contains("任务执行失败") && l.message.contains("request_log_snapshot")
        }),
        "失败原因应落日志：{:?}",
        logs.iter().map(|l| &l.message).collect::<Vec<_>>()
    );

    // 成功路径：先跑一轮生成把快照初始化，自愈再正常收尾。
    let (db, _scheduler, _) = setup_db_and_scheduler().await;
    let gen_run = run_once(&db, STATS_SNAPSHOT_JOB, generation_job_handler()).await;
    assert_eq!(gen_run.status, "success", "干净库上生成应成功");
    let run = run_once(&db, STATS_SNAPSHOT_REBUILD_JOB, heal_job_handler()).await;
    assert_eq!(run.status, "success", "正常执行不得误标失败");
}

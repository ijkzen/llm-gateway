//! `models_refresh` 内置任务接线：种子幂等 + 表达式 + 调度器可见（ADR-0025）。

mod common;

use std::sync::Arc;

use llm_gateway::cron::JobContext;
use llm_gateway::cron::repository::SeaOrmCronJobRepository;

#[tokio::test]
async fn models_refresh_job_seed_is_scheduled() {
    let (db, scheduler, _log_tx) = common::setup_db_and_scheduler().await;
    scheduler
        .register_handler(
            llm_gateway::cron::seed::MODELS_REFRESH_JOB,
            Arc::new(|_ctx: JobContext| Box::pin(async move { Ok(()) })),
        )
        .await;
    // 幂等：重复确保只留一行。
    llm_gateway::cron::seed::ensure_models_refresh_job(&db)
        .await
        .unwrap();
    llm_gateway::cron::seed::ensure_models_refresh_job(&db)
        .await
        .unwrap();

    let repo = SeaOrmCronJobRepository::new(db.clone());
    scheduler.load_from_db(&repo).await.unwrap();
    scheduler.start().await.unwrap();

    assert!(
        scheduler
            .has_job(llm_gateway::cron::seed::MODELS_REFRESH_JOB)
            .await
    );
    let jobs = scheduler.list_jobs().await;
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].expression, "0 0 4 * * *");
    assert_eq!(jobs[0].group, "system");
    assert!(jobs[0].enabled);
}

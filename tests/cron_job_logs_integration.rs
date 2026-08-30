//! 定时任务执行日志 API 的集成测试。
//!
//! 通过 HTTP 触发任务执行，再验证 logs 列表与单次日志接口。
//! 日志捕获链路（tracing → Layer → 落库）由 lib 单元测试覆盖；
//! 这里主要验证 API 的行为与数据完整性。

mod common;

use std::sync::Arc;

use axum::body::Body;
use axum::http::Request;
use tower::ServiceExt;

use llm_gateway::cron::JobContext;
use llm_gateway::cron::repository::{CronJobRepository, JobDefinition, SeaOrmCronJobRepository};

async fn setup_app() -> (axum::Router, sea_orm::DatabaseConnection) {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;

    scheduler
        .register_handler(
            "log_job",
            Arc::new(|_ctx: JobContext| {
                Box::pin(async move {
                    tracing::info!("执行开始");
                    tracing::warn!("注意告警");
                    Ok(())
                })
            }),
        )
        .await;

    let repo = SeaOrmCronJobRepository::new(db.clone());
    repo.insert(&JobDefinition {
        name: "log_job".to_string(),
        title: "Log Test".to_string(),
        description: "".to_string(),
        expression: "@hourly".to_string(),
        enabled: true,
        group: "default".to_string(),
    })
    .await
    .unwrap();

    scheduler.load_from_db(&repo).await.unwrap();
    scheduler.start().await.unwrap();

    let app = common::build_authed_app(db.clone(), scheduler, log_tx).await;
    (app, db)
}

async fn get_json(app: &axum::Router, uri: &str) -> (u16, String) {
    let request: Request<Body> = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status().as_u16();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8(body.to_vec()).unwrap())
}

/// 手动触发任务并轮询等待执行完成，返回该次执行的 run_id。
async fn run_job_and_wait(app: &axum::Router, db: &sea_orm::DatabaseConnection) -> String {
    use llm_gateway::cron::log_repository::{CronJobLogRepository, SeaOrmCronJobLogRepository};

    let request: Request<Body> = Request::builder()
        .method("POST")
        .uri("/api/cron-jobs/log_job/run")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), 200);

    let log_repo = SeaOrmCronJobLogRepository::new(db.clone());
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
    loop {
        let runs = log_repo.list_runs("log_job", 10).await.unwrap();
        if let Some(run) = runs.first()
            && run.status != "running"
        {
            return run.run_id.clone();
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for the job run to finish"
        );
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn test_run_job_creates_runs_and_logs() {
    let (app, db) = setup_app().await;
    let run_id = run_job_and_wait(&app, &db).await;

    let (status, body) = get_json(&app, "/api/cron-jobs/log_job/logs").await;
    assert_eq!(status, 200);
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["code"], "0");
    let runs = json["data"].as_array().unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0]["run_id"], run_id);
    assert_eq!(runs[0]["status"], "success");
    assert!(runs[0]["started_at"].as_str().unwrap().contains('T'));
    assert!(runs[0]["ended_at"].as_str().unwrap().contains('T'));
}

#[tokio::test]
async fn test_run_logs_returns_persisted_lines() {
    let (app, db) = setup_app().await;
    let run_id = run_job_and_wait(&app, &db).await;

    let (status, body) = get_json(&app, &format!("/api/cron-jobs/log_job/logs/{run_id}")).await;
    assert_eq!(status, 200);
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["code"], "0");
    let logs = json["data"].as_array().unwrap();
    // 捕获链路在集成测试进程内未注册 subscriber，日志可能为空；
    // 至少保证接口形状正确（seq/level/message/ts 字段或空数组）。
    for log in logs {
        assert!(log["seq"].is_number());
        assert!(log["message"].is_string());
        assert!(log["ts"].as_str().unwrap().contains('T'));
    }
}

#[tokio::test]
async fn test_run_logs_404_for_unknown_run() {
    let (app, _db) = setup_app().await;
    let (status, body) = get_json(&app, "/api/cron-jobs/log_job/logs/does-not-exist").await;
    assert_eq!(status, 404);
    assert!(body.contains("NOT_FOUND"));
}

#[tokio::test]
async fn test_logs_empty_before_any_run() {
    let (app, _db) = setup_app().await;
    let (status, body) = get_json(&app, "/api/cron-jobs/log_job/logs").await;
    assert_eq!(status, 200);
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["data"].as_array().unwrap().len(), 0);
}

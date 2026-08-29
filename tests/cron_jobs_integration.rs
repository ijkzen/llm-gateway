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
            "test_job",
            Arc::new(|_ctx: JobContext| Box::pin(async move { Ok(()) })),
        )
        .await;

    let repo = SeaOrmCronJobRepository::new(db.clone());
    repo.insert(&JobDefinition {
        name: "test_job".to_string(),
        title: "Test".to_string(),
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

#[tokio::test]
async fn test_update_job_rejects_empty_title() {
    let (app, _db) = setup_app().await;
    let request: Request<Body> = Request::builder()
        .method("PUT")
        .uri("/api/cron-jobs/test_job")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"title":""}"#))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), 400);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_str = String::from_utf8(body.to_vec()).unwrap();
    assert!(body_str.contains("INVALID_INPUT"));
    assert!(body_str.contains("标题不能为空"));
}

#[tokio::test]
async fn test_update_job_rejects_empty_expression() {
    let (app, _db) = setup_app().await;
    let request: Request<Body> = Request::builder()
        .method("PUT")
        .uri("/api/cron-jobs/test_job")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"expression":""}"#))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), 400);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_str = String::from_utf8(body.to_vec()).unwrap();
    assert!(body_str.contains("INVALID_INPUT"));
    assert!(body_str.contains("表达式不能为空"));
}

#[tokio::test]
async fn test_update_job_updates_group() {
    let (app, db) = setup_app().await;
    let request: Request<Body> = Request::builder()
        .method("PUT")
        .uri("/api/cron-jobs/test_job")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"group":"new-group"}"#))
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), 200);

    let repo = SeaOrmCronJobRepository::new(db);
    let model = repo.find_by_name("test_job").await.unwrap().unwrap();
    assert_eq!(model.group, "new-group");

    let request: Request<Body> = Request::builder()
        .method("GET")
        .uri("/api/cron-jobs")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), 200);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_str = String::from_utf8(body.to_vec()).unwrap();
    assert!(body_str.contains("new-group"));
}

#[tokio::test]
async fn test_delete_job_succeeds() {
    let (app, db) = setup_app().await;
    let request: Request<Body> = Request::builder()
        .method("DELETE")
        .uri("/api/cron-jobs/test_job")
        .body(Body::empty())
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), 200);

    let repo = SeaOrmCronJobRepository::new(db);
    assert!(repo.find_by_name("test_job").await.unwrap().is_none());
    let model = repo
        .find_by_name_including_deleted("test_job")
        .await
        .unwrap()
        .unwrap();
    assert!(model.is_deleted);

    let request: Request<Body> = Request::builder()
        .method("GET")
        .uri("/api/cron-jobs")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), 200);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_str = String::from_utf8(body.to_vec()).unwrap();
    assert!(!body_str.contains("test_job"));
}

#[tokio::test]
async fn test_update_job_updates_all_fields_atomically() {
    let (app, db) = setup_app().await;
    let request: Request<Body> = Request::builder()
        .method("PUT")
        .uri("/api/cron-jobs/test_job")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"title":"New Title","description":"New Desc","expression":"@daily","enabled":false,"group":"new-group"}"#,
        ))
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), 200);

    let repo = SeaOrmCronJobRepository::new(db);
    let model = repo.find_by_name("test_job").await.unwrap().unwrap();
    assert_eq!(model.title, "New Title");
    assert_eq!(model.description, "New Desc");
    assert_eq!(model.expression, "@daily");
    assert!(!model.enabled);
    assert_eq!(model.group, "new-group");

    let request: Request<Body> = Request::builder()
        .method("GET")
        .uri("/api/cron-jobs")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), 200);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_str = String::from_utf8(body.to_vec()).unwrap();
    assert!(body_str.contains("New Title"));
    assert!(body_str.contains("new-group"));
    assert!(body_str.contains("@daily"));
}

#[tokio::test]
async fn test_run_job_succeeds() {
    let (app, _db) = setup_app().await;
    let request: Request<Body> = Request::builder()
        .method("POST")
        .uri("/api/cron-jobs/test_job/run")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), 200);
}

#[tokio::test]
async fn test_update_job_rejects_unloaded_job_without_touching_db() {
    let (app, db) = setup_app().await;

    // Insert a job whose handler is not registered; it is skipped at load
    // time and therefore unknown to the scheduler.
    let repo = SeaOrmCronJobRepository::new(db.clone());
    repo.insert(&JobDefinition {
        name: "unloaded_job".to_string(),
        title: "Unloaded".to_string(),
        description: "".to_string(),
        expression: "@hourly".to_string(),
        enabled: true,
        group: "default".to_string(),
    })
    .await
    .unwrap();

    let request: Request<Body> = Request::builder()
        .method("PUT")
        .uri("/api/cron-jobs/unloaded_job")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"title":"Changed"}"#))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), 400);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_str = String::from_utf8(body.to_vec()).unwrap();
    assert!(body_str.contains("INVALID_INPUT"));

    // Regression: the DB row must NOT have been modified by the failed update.
    let model = repo.find_by_name("unloaded_job").await.unwrap().unwrap();
    assert_eq!(model.title, "Unloaded");
}

#[tokio::test]
async fn test_update_job_returns_404_for_missing_job_with_invalid_expression() {
    let (app, _db) = setup_app().await;
    let request: Request<Body> = Request::builder()
        .method("PUT")
        .uri("/api/cron-jobs/missing_job")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"expression":"not-a-cron"}"#))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), 404);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_str = String::from_utf8(body.to_vec()).unwrap();
    assert!(body_str.contains("NOT_FOUND"));
}

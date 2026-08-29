//! Shared bootstrap helpers for the HTTP integration tests.

use axum::extract::Request;
use axum::middleware::Next;
use axum::http::HeaderValue;
use axum::response::Response;
use sea_orm::{ActiveModelTrait, DatabaseConnection, Set};

use llm_gateway::auth::hash_token;
use llm_gateway::cron::log_capture::JobLogEvent;
use llm_gateway::cron::scheduler::SchedulerRuntime;
use llm_gateway::cron::worker::JobWorker;
use llm_gateway::db;
use llm_gateway::entity::{api_key, session, user};
use llm_gateway::routes;
use llm_gateway::state::AppState;

/// 集成测试默认用户（Admin / Password）与固定会话令牌。
const TEST_USERNAME: &str = "Admin";
const TEST_PASSWORD: &str = "Password";
const TEST_SESSION_TOKEN: &str = "itest-session-token-0123456789abcdef";
const TEST_COOKIE: &str = "lg_session=itest-session-token-0123456789abcdef";
const TEST_API_KEY_PLAIN: &str = "lg-itest-api-key-0000000000000";
const TEST_BEARER: &str = "Bearer lg-itest-api-key-0000000000000";

/// Creates an in-memory database, starts a job worker, and builds the
/// scheduler on top of it. The scheduler is returned unstarted so each test
/// can register handlers and seed data before calling `start()`.
pub async fn setup_db_and_scheduler(
) -> (DatabaseConnection, SchedulerRuntime, tokio::sync::broadcast::Sender<JobLogEvent>) {
    let db = db::connect("sqlite::memory:").await.unwrap();

    let (log_tx, _) = tokio::sync::broadcast::channel::<JobLogEvent>(64);
    let worker = JobWorker::new(db.clone(), 2, 100, log_tx.clone());
    let handle = worker.start();

    let scheduler = SchedulerRuntime::new(handle.tx.clone()).await.unwrap();
    (db, scheduler, log_tx)
}

/// Builds the Axum app with the given database and scheduler as state.
///
/// 未经认证包装，供 auth 集成测试直接验证 401 行为。
pub fn build_app(
    db: DatabaseConnection,
    scheduler: SchedulerRuntime,
    log_tx: tokio::sync::broadcast::Sender<JobLogEvent>,
) -> axum::Router {
    let state = AppState {
        db,
        scheduler,
        log_tx,
        lb_state: llm_gateway::proxy::LbState::default(),
    };
    routes::create_app(&state)
}

/// 在测试库中种入默认用户、固定会话与测试 API Key。
async fn seed_default_auth(db: &DatabaseConnection) {
    let now = chrono::Utc::now();
    let user_id = user::ActiveModel {
        username: Set(TEST_USERNAME.to_string()),
        password_hash: Set(llm_gateway::auth::hash_password(TEST_PASSWORD).unwrap()),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap()
    .id;

    session::ActiveModel {
        id: Set(hash_token(TEST_SESSION_TOKEN)),
        user_id: Set(user_id),
        created_at: Set(now),
        expires_at: Set(now + chrono::Duration::days(365)),
    }
    .insert(db)
    .await
    .unwrap();

    api_key::ActiveModel {
        name: Set("itest-key".to_string()),
        key: Set(llm_gateway::crypto::encrypt(TEST_API_KEY_PLAIN)),
        key_hash: Set(Some(hash_token(TEST_API_KEY_PLAIN))),
        enable: Set(true),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap();
}

/// 测试专用请求头注入：/api/* 注入会话 Cookie，/v1/* 注入 Bearer。
/// 认证中间件仍然完整执行，仅模拟"已持有凭证的客户端"。
async fn inject_test_auth(mut req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();
    let headers = req.headers_mut();
    if path.starts_with("/api/") && !path.starts_with("/api/auth/") {
        headers.insert(axum::http::header::COOKIE, HeaderValue::from_static(TEST_COOKIE));
    } else if path.starts_with("/v1/") {
        headers.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_static(TEST_BEARER),
        );
    }
    next.run(req).await
}

/// 与 build_app 相同，但种入默认凭证并给请求自动注入认证头。
/// 大多数集成测试只关心业务行为，用该入口省去逐请求携带凭证。
pub async fn build_authed_app(
    db: DatabaseConnection,
    scheduler: SchedulerRuntime,
    log_tx: tokio::sync::broadcast::Sender<JobLogEvent>,
) -> axum::Router {
    seed_default_auth(&db).await;
    build_app(db, scheduler, log_tx)
        .layer(axum::middleware::from_fn(inject_test_auth))
}


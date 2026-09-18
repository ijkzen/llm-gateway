//! 飞书通知渠道集成测试（`.scratch/feishu-notification/`）。
//!
//! 配置读写类用例经 `build_authed_app` 注入会话凭证即可；涉及真实发送/注册
//! 的用例由本地 mock 飞书服务器经 `LLM_GATEWAY_FEISHU_BASE_URL` 重定向。

mod common;

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::IntoResponse;
use sea_orm::{ActiveModelTrait, DatabaseConnection, Set};
use serde_json::{Value, json};
use tower::ServiceExt;

use llm_gateway::entity::{notification_channel, provider};
use llm_gateway::usage::types::{QuotaWindow, UsageData, UsageKind, WindowKind};

/// 飞书发送用例串行锁：重定向经进程级环境变量实现（`temp_env`），并发跑会互相
/// 覆盖基址（同仓库 SUBSCRIBER_LOCK 对全局状态的处置）。
static SEND_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// 本地 mock 飞书：按路径区分端点，记录收到的请求体。
#[derive(Clone, Default)]
struct FeishuMock {
    token_bodies: Arc<Mutex<Vec<Value>>>,
    message_bodies: Arc<Mutex<Vec<Value>>>,
    message_queries: Arc<Mutex<Vec<String>>>,
    /// 非 0 时消息端点返回该业务错误码（HTTP 仍 200，同飞书实际行为）。
    message_error_code: Arc<Mutex<Option<i64>>>,
    /// 注册端点收到的原始 form body（按顺序）。
    registration_bodies: Arc<Mutex<Vec<String>>>,
    /// 注册端点 poll 的脚本化响应队列（出队即用，空则返回 authorization_pending）。
    registration_polls: Arc<Mutex<std::collections::VecDeque<Value>>>,
}

impl FeishuMock {
    fn token_bodies(&self) -> Vec<Value> {
        self.token_bodies.lock().unwrap().clone()
    }
    fn message_bodies(&self) -> Vec<Value> {
        self.message_bodies.lock().unwrap().clone()
    }
    fn message_queries(&self) -> Vec<String> {
        self.message_queries.lock().unwrap().clone()
    }
    /// 真实打到飞书注册端点的 poll 次数（用于验证 interval 节流）。
    fn poll_count(&self) -> usize {
        self.registration_bodies
            .lock()
            .unwrap()
            .iter()
            .filter(|body| body.contains("action=poll"))
            .count()
    }
    fn queue_poll(&self, response: Value) {
        self.registration_polls.lock().unwrap().push_back(response);
    }
}

/// 注册端点：`action=begin` 返回二维码内容；`action=poll` 按脚本队列出队，
/// 队列空则返回 `authorization_pending`（HTTP 400，同飞书实际行为）。
fn registration_route(mock: FeishuMock) -> axum::routing::MethodRouter {
    axum::routing::post(move |request: Request<Body>| {
        let state = mock.clone();
        async move {
            let raw = axum::body::to_bytes(request.into_body(), usize::MAX)
                .await
                .unwrap();
            let body = String::from_utf8_lossy(&raw).to_string();
            let is_poll = body.contains("action=poll");
            state.registration_bodies.lock().unwrap().push(body);

            if !is_poll {
                return axum::Json(json!({
                    "device_code": "dc-mock-1",
                    "user_code": "ABCD-1234",
                    "verification_uri": "https://open.feishu.cn/page/launcher",
                    "verification_uri_complete": "https://open.feishu.cn/page/launcher?user_code=ABCD-1234",
                    "expires_in": 3600,
                    "interval": 1,
                }))
                .into_response();
            }

            let payload = state
                .registration_polls
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| json!({"error": "authorization_pending"}));
            // 飞书把 authorization_pending 等以 HTTP 400 返回——客户端必须解析 body。
            let status = if payload.get("error").is_some() {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::OK
            };
            (status, axum::Json(payload)).into_response()
        }
    })
}

/// 启动 mock 飞书并返回（基址，mock 状态）。
async fn spawn_feishu_mock() -> (String, FeishuMock) {
    let mock = FeishuMock::default();

    let token_state = mock.clone();
    let token_route = axum::routing::post(move |request: Request<Body>| {
        let state = token_state.clone();
        async move {
            let body = axum::body::to_bytes(request.into_body(), usize::MAX)
                .await
                .unwrap();
            state
                .token_bodies
                .lock()
                .unwrap()
                .push(serde_json::from_slice(&body).unwrap_or(Value::Null));
            axum::Json(json!({
                "code": 0,
                "msg": "ok",
                "tenant_access_token": "t-mock-token",
                "expire": 7200,
            }))
            .into_response()
        }
    });

    let message_state = mock.clone();
    let message_route = axum::routing::post(move |request: Request<Body>| {
        let state = message_state.clone();
        async move {
            state
                .message_queries
                .lock()
                .unwrap()
                .push(request.uri().query().unwrap_or_default().to_string());
            let body = axum::body::to_bytes(request.into_body(), usize::MAX)
                .await
                .unwrap();
            state
                .message_bodies
                .lock()
                .unwrap()
                .push(serde_json::from_slice(&body).unwrap_or(Value::Null));
            let error_code = *state.message_error_code.lock().unwrap();
            match error_code {
                Some(code) => {
                    axum::Json(json!({"code": code, "msg": "mock error"})).into_response()
                }
                None => axum::Json(json!({
                    "code": 0,
                    "msg": "success",
                    "data": {"message_id": "om_mock_1"},
                }))
                .into_response(),
            }
        }
    });

    let app = Router::new()
        .route(
            "/open-apis/auth/v3/tenant_access_token/internal",
            token_route,
        )
        .route("/open-apis/im/v1/messages", message_route)
        .route(
            "/oauth/v1/app/registration",
            registration_route(mock.clone()),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{addr}"), mock)
}

/// 把配置保存成「指向 mock 的可用配置」。
async fn save_config(app: &Router, receiver: &str, secret: &str) {
    let (status, body) = call(
        app,
        "PUT",
        URI,
        Some(json!({
            "appId": "cli_mock",
            "appSecret": secret,
            "receiverType": "open_id",
            "receiver": receiver,
            "enable": true,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "保存配置应成功：{body}");
}

const TEST_URI: &str = "/api/notification/feishu/test";

async fn setup_app() -> (Router, DatabaseConnection) {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    scheduler.start().await.unwrap();
    let app = common::build_authed_app(db.clone(), scheduler, log_tx).await;
    (app, db)
}

/// 发一次请求，返回（状态码，响应体 JSON）。
async fn call(app: &Router, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    let request = match body {
        Some(payload) => builder
            .header("content-type", "application/json")
            .body(Body::from(payload.to_string()))
            .unwrap(),
        None => {
            builder = builder.header("content-type", "application/json");
            builder.body(Body::empty()).unwrap()
        }
    };

    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let parsed = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, parsed)
}

const URI: &str = "/api/notification/feishu";

// ---------- 测试发送（mock 飞书） ----------

// ---------- 扫码创建（mock 飞书） ----------

const REGISTER_URI: &str = "/api/notification/feishu/register";

// ---------- 自动转换触发通知 ----------

/// 种子一个订阅制供应商（用量已开启，billing_mode=1）。
async fn seed_subscription_provider(db: &DatabaseConnection, name: &str) -> provider::Model {
    let now = chrono::Utc::now();
    provider::ActiveModel {
        name: Set(name.to_string()),
        enable: Set(true),
        base_url: Set("https://open.bigmodel.cn/v1".to_string()),
        api_key: Set(llm_gateway::crypto::encrypt("sk-x")),
        custom_header: Set("{}".to_string()),
        protocol_type: Set(0),
        billing_mode: Set(1),
        extra: Set(r#"{"usage": true, "usage_type": 1}"#.to_string()),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap()
}

/// 全部窗口剩余为 0 的订阅用量 → 判定「额度耗尽」。
fn exhausted_quota(provider_id: i32) -> UsageData {
    UsageData {
        provider_id,
        fetched_at: chrono::Utc::now(),
        kind: UsageKind::Quota,
        plan: Some("pro".to_string()),
        windows: vec![
            QuotaWindow::from_remaining_percent(WindowKind::FiveHour, 0.0, None),
            QuotaWindow::from_remaining_percent(WindowKind::Weekly, 0.0, None),
            QuotaWindow::from_remaining_percent(WindowKind::Monthly, 0.0, None),
        ],
        balances: vec![],
    }
}

/// 余量充足的订阅用量 → 判定可用（用于触发额度恢复）。
fn healthy_quota(provider_id: i32) -> UsageData {
    UsageData {
        provider_id,
        fetched_at: chrono::Utc::now(),
        kind: UsageKind::Quota,
        plan: Some("pro".to_string()),
        windows: vec![
            QuotaWindow::from_remaining_percent(WindowKind::FiveHour, 80.0, None),
            QuotaWindow::from_remaining_percent(WindowKind::Weekly, 60.0, None),
            QuotaWindow::from_remaining_percent(WindowKind::Monthly, 40.0, None),
        ],
        balances: vec![],
    }
}

/// 有界轮询等待：通知是 `tokio::spawn` 出去的，用有界等待代替固定 sleep
/// （固定 sleep 会随机器负载假失败）。
async fn wait_for_messages(mock: &FeishuMock, expected: usize) {
    for _ in 0..250 {
        if mock.message_bodies().len() >= expected {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}

// ---------- 备份集成 ----------

const EXPORT_URI: &str = "/api/backup/export";
const IMPORT_URI: &str = "/api/backup/import";

/// 最小可用备份文件；`channels` 为 None 时**不带** notificationChannels 字段
/// （模拟早于本特性的导出文件）。
fn minimal_backup(channels: Option<Value>) -> Value {
    let mut file = json!({
        "version": 1,
        "exportedAt": "2026-09-18T00:00:00Z",
        "providers": [],
        "virtualModels": [],
        "apiKeys": [],
        "settings": [],
    });
    if let Some(channels) = channels {
        file["notificationChannels"] = channels;
    }
    file
}

async fn import_backup(app: &Router, file: Value) -> (StatusCode, Value) {
    let request = Request::builder()
        .method("POST")
        .uri(IMPORT_URI)
        .header("content-type", "application/json")
        .body(Body::from(file.to_string()))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[path = "notification_integration/backup.rs"]
mod backup;
#[path = "notification_integration/config.rs"]
mod config;
#[path = "notification_integration/register.rs"]
mod register;
#[path = "notification_integration/send.rs"]
mod send;
#[path = "notification_integration/trigger.rs"]
mod trigger;

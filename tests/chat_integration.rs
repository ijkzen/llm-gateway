//! /api/chat/completions 管理后台聊天直连端点集成测试：
//! 会话鉴权、供应商/模型校验、SSE 流式（含思考内容）、request 表落库。
#![allow(clippy::await_holding_lock)]

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode as HttpStatus};
use axum::response::IntoResponse;
use axum::routing::post;
use sea_orm::{ActiveModelTrait, EntityTrait, Set};
use serde_json::{Value, json};
use tower::ServiceExt;

use llm_gateway::entity::request;
use llm_gateway::entity::{provider, provider_model};

const TEST_COOKIE: &str = "lg_session=itest-session-token-0123456789abcdef";

// ---------- mock 上游 ----------

type Captured = Arc<Mutex<Vec<Value>>>;

fn capture() -> Captured {
    Arc::new(Mutex::new(Vec::new()))
}

/// 启动含 OpenAI chat 与 Anthropic messages 路径的 mock 上游：流式返回思考增量 + 正文增量。
async fn spawn_mock(captured: Captured) -> String {
    let captured_messages = captured.clone();
    let app = Router::new()
        .route(
            "/v1/chat/completions",
            post(move |request: Request<Body>| {
                let captured = captured.clone();
                async move {
                    let body = axum::body::to_bytes(request.into_body(), usize::MAX)
                        .await
                        .unwrap();
                    let parsed: Value =
                        serde_json::from_str(&String::from_utf8_lossy(&body)).unwrap();
                    captured.lock().unwrap().push(parsed);
                    let payload = [
                        json!({"id":"chatcmpl-c1","object":"chat.completion.chunk","model":"m-1","choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}).to_string(),
                        json!({"id":"chatcmpl-c1","object":"chat.completion.chunk","model":"m-1","choices":[{"index":0,"delta":{"reasoning_content":"想一想"},"finish_reason":null}]}).to_string(),
                        json!({"id":"chatcmpl-c1","object":"chat.completion.chunk","model":"m-1","choices":[{"index":0,"delta":{"content":"你好"},"finish_reason":null}]}).to_string(),
                        json!({"id":"chatcmpl-c1","object":"chat.completion.chunk","model":"m-1","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}).to_string(),
                        json!({"id":"chatcmpl-c1","object":"chat.completion.chunk","model":"m-1","choices":[],"usage":{"prompt_tokens":5,"completion_tokens":3,"total_tokens":8}}).to_string(),
                    ]
                    .iter()
                    .map(|event| format!("data: {event}\n\n"))
                    .collect::<String>()
                        + "data: [DONE]\n\n";
                    (
                        HttpStatus::OK,
                        [("content-type", "text/event-stream")],
                        payload,
                    )
                        .into_response()
                }
            }),
        )
        .route(
            "/v1/messages",
            post(move |request: Request<Body>| {
                let captured = captured_messages.clone();
                async move {
                    let body = axum::body::to_bytes(request.into_body(), usize::MAX)
                        .await
                        .unwrap();
                    let parsed: Value =
                        serde_json::from_str(&String::from_utf8_lossy(&body)).unwrap();
                    captured.lock().unwrap().push(parsed);
                    let payload = [
                        json!({"type":"message_start","message":{"id":"msg-c1","usage":{"input_tokens":5}}}).to_string(),
                        json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking"}}).to_string(),
                        json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"想一想"}}).to_string(),
                        json!({"type":"content_block_start","index":1,"content_block":{"type":"text"}}).to_string(),
                        json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"你好"}}).to_string(),
                        json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":3}}).to_string(),
                        json!({"type":"message_stop"}).to_string(),
                    ]
                    .iter()
                    .map(|event| format!("data: {event}\n\n"))
                    .collect::<String>();
                    (
                        HttpStatus::OK,
                        [("content-type", "text/event-stream")],
                        payload,
                    )
                        .into_response()
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

// ---------- 种子与请求辅助 ----------

async fn seed_provider_named(
    db: &sea_orm::DatabaseConnection,
    name: &str,
    base_url: &str,
    enable: bool,
    protocol_type: i32,
) -> i32 {
    let active = provider::ActiveModel {
        name: Set(name.to_string()),
        enable: Set(enable),
        base_url: Set(base_url.to_string()),
        api_key: Set(llm_gateway::crypto::encrypt("sk-mock")),
        custom_header: Set("{}".to_string()),
        protocol_type: Set(protocol_type),
        billing_mode: Set(0),
        extra: Set("{}".to_string()),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    active.insert(db).await.unwrap().id
}

async fn seed_provider(db: &sea_orm::DatabaseConnection, base_url: &str, enable: bool) -> i32 {
    seed_provider_named(db, "p-chat", base_url, enable, 0).await
}

async fn seed_provider_model(
    db: &sea_orm::DatabaseConnection,
    provider_id: i32,
    remote_id: &str,
) -> i32 {
    let active = provider_model::ActiveModel {
        provider_id: Set(provider_id),
        provider_model_id: Set(remote_id.to_string()),
        context_length: Set(128000),
        max_output_tokens: Set(4096),
        reasoning: Set(true),
        tool_use: Set(false),
        image_understand: Set(false),
        video_understand: Set(false),
        protocol_type: Set(None),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    active.insert(db).await.unwrap().model_id
}

async fn setup_app() -> (axum::Router, sea_orm::DatabaseConnection) {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    scheduler.start().await.unwrap();
    let app = common::build_authed_app(db.clone(), scheduler, log_tx).await;
    (app, db)
}

async fn send_chat(app: &axum::Router, body: Value) -> (u16, String, String) {
    let request = Request::builder()
        .method("POST")
        .uri("/api/chat/completions")
        .header("content-type", "application/json")
        .header("cookie", TEST_COOKIE)
        .body(Body::from(body.to_string()))
        .unwrap();
    let (status, content_type, text) = {
        let response = app.clone().oneshot(request).await.unwrap();
        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (
            status,
            content_type,
            String::from_utf8_lossy(&bytes).to_string(),
        )
    };
    (status, content_type, text)
}

fn chat_body(provider_id: i32, model_id: i32) -> Value {
    json!({
        "providerId": provider_id,
        "modelId": model_id,
        "messages": [{"role": "user", "content": "你好"}],
    })
}

/// 等待 request 表出现记录（落库为异步任务）。
async fn wait_for_records(
    db: &sea_orm::DatabaseConnection,
    expected: usize,
) -> Vec<request::Model> {
    for _ in 0..40 {
        if let Ok(rows) = request::Entity::find().all(db).await
            && rows.len() >= expected
        {
            return rows;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    request::Entity::find().all(db).await.unwrap()
}

// ---------- 用例 ----------

#[tokio::test]
async fn chat_requires_session() {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    scheduler.start().await.unwrap();
    let app = common::build_app(db, scheduler, log_tx);
    let request = Request::builder()
        .method("POST")
        .uri("/api/chat/completions")
        .header("content-type", "application/json")
        .body(Body::from(chat_body(1, 1).to_string()))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status().as_u16(), 401);
}

#[tokio::test]
async fn chat_stream_direct_with_reasoning_and_record() {
    let captured = capture();
    let base = spawn_mock(captured.clone()).await;
    let (app, db) = setup_app().await;
    let provider_id = seed_provider(&db, &base, true).await;
    let model_id = seed_provider_model(&db, provider_id, "m-1").await;

    let (status, content_type, text) = send_chat(&app, chat_body(provider_id, model_id)).await;
    assert_eq!(status, 200, "{text}");
    assert!(content_type.contains("text/event-stream"), "{content_type}");
    // 思考增量与正文增量均透传；结束帧 finish_reason。
    assert!(text.contains("reasoning_content"), "{text}");
    assert!(text.contains("想一想"), "{text}");
    assert!(text.contains("\"content\":\"你好\""), "{text}");
    assert!(text.contains("finish_reason\":\"stop"), "{text}");

    // 上游收到的 model 为供应商模型 ID，stream 强制为 true；
    // OpenAI Compat 为透传协议，不注入 reasoning_effort。
    let upstream_bodies = captured.lock().unwrap();
    assert_eq!(upstream_bodies.len(), 1);
    assert_eq!(upstream_bodies[0]["model"], json!("m-1"));
    assert_eq!(upstream_bodies[0]["stream"], json!(true));
    assert!(upstream_bodies[0].get("reasoning_effort").is_none());

    let rows = wait_for_records(&db, 1).await;
    assert_eq!(rows.len(), 1);
    assert!(rows[0].success);
    assert_eq!(rows[0].provider_id, provider_id);
    assert_eq!(rows[0].model_id, "m-1");
    assert!(rows[0].stream);
}

#[tokio::test]
async fn chat_rejects_unknown_provider_or_model() {
    let (app, db) = setup_app().await;
    let provider_id = seed_provider(&db, "http://127.0.0.1:1", true).await;

    let (status, _, text) = send_chat(&app, chat_body(99999, 1)).await;
    assert_eq!(status, 404, "{text}");
    let (status, _, text) = send_chat(&app, chat_body(provider_id, 99999)).await;
    assert_eq!(status, 404, "{text}");
    // 供应商对不上模型归属同样 404。
    let other_provider = seed_provider_named(&db, "p-chat-2", "http://127.0.0.1:1", true, 0).await;
    let model_id = seed_provider_model(&db, provider_id, "m-1").await;
    let (status, _, text) = send_chat(&app, chat_body(other_provider, model_id)).await;
    assert_eq!(status, 404, "{text}");
}

#[tokio::test]
async fn chat_rejects_disabled_provider() {
    let (app, db) = setup_app().await;
    let provider_id = seed_provider(&db, "http://127.0.0.1:1", false).await;
    let model_id = seed_provider_model(&db, provider_id, "m-1").await;

    let (status, _, text) = send_chat(&app, chat_body(provider_id, model_id)).await;
    assert_eq!(status, 400, "{text}");
}

#[tokio::test]
async fn chat_rejects_empty_messages() {
    let (app, db) = setup_app().await;
    let provider_id = seed_provider(&db, "http://127.0.0.1:1", true).await;
    let model_id = seed_provider_model(&db, provider_id, "m-1").await;

    let (status, _, text) = send_chat(
        &app,
        json!({"providerId": provider_id, "modelId": model_id, "messages": []}),
    )
    .await;
    assert_eq!(status, 400, "{text}");
}

#[tokio::test]
async fn chat_anthropic_reasoning_model_requests_thinking() {
    let captured = capture();
    let base = spawn_mock(captured.clone()).await;
    let (app, db) = setup_app().await;
    let provider_id = seed_provider_named(&db, "p-anthropic", &base, true, 2).await;
    let model_id = seed_provider_model(&db, provider_id, "m-1").await;

    let (status, content_type, text) = send_chat(&app, chat_body(provider_id, model_id)).await;
    assert_eq!(status, 200, "{text}");
    assert!(content_type.contains("text/event-stream"), "{content_type}");

    // 请求体已按 reasoning 能力注入 thinking 预算（reasoning_effort=medium → Anthropic thinking）。
    let upstream_bodies = captured.lock().unwrap();
    assert_eq!(upstream_bodies.len(), 1);
    assert_eq!(upstream_bodies[0]["thinking"]["type"], json!("enabled"));
    assert!(
        upstream_bodies[0]["thinking"]["budget_tokens"]
            .as_i64()
            .unwrap()
            > 0
    );

    // 上游 thinking 增量被归一为 reasoning_content 透传给前端。
    assert!(text.contains("reasoning_content"), "{text}");
    assert!(text.contains("想一想"), "{text}");
    assert!(text.contains("\"content\":\"你好\""), "{text}");
    drop(upstream_bodies);
    let rows = wait_for_records(&db, 1).await;
    assert!(rows[0].success);
}

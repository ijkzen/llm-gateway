//! /v1 转发集成测试：本地 mock 上游覆盖各协议转换、failover 与 request 表落库。
//! mock 上游用 Mutex 记录请求体，锁跨 await 持有是测试有意为之；assert_eq 布尔比较改 assert 更清晰。
#![allow(clippy::await_holding_lock, clippy::bool_assert_comparison)]

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode as HttpStatus};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde_json::{Value, json};
use tower::ServiceExt;

use llm_gateway::entity::request;
use llm_gateway::entity::setting;
use llm_gateway::entity::{provider, provider_model, virtual_model, virtual_model_item};

#[path = "proxy_integration/failover.rs"]
mod failover;
#[path = "proxy_integration/native_messages.rs"]
mod native_messages;
#[path = "proxy_integration/native_responses.rs"]
mod native_responses;
#[path = "proxy_integration/outbound_headers.rs"]
mod outbound_headers;
#[path = "proxy_integration/protocol.rs"]
mod protocol;
#[path = "proxy_integration/responses_live.rs"]
mod responses_live;
#[path = "proxy_integration/upstream_abort.rs"]
mod upstream_abort;

const TEST_BEARER: &str = "Bearer lg-itest-api-key-0000000000000";

// ---------- mock 上游 ----------

/// 最近一次请求体捕获器。
type Captured = Arc<Mutex<Vec<Value>>>;

fn capture() -> Captured {
    Arc::new(Mutex::new(Vec::new()))
}

fn record_capture(captured: &Captured, body: &str) {
    if let Ok(value) = serde_json::from_str::<Value>(body) {
        captured.lock().unwrap().push(value);
    }
}

fn sse(events: &[String]) -> Response {
    let payload = events
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

/// 捕获器：请求体 + 请求头（供 header 透传/剥离断言）。
type CapturedHeaders = Arc<Mutex<Vec<HeaderMap>>>;

fn capture_headers() -> CapturedHeaders {
    Arc::new(Mutex::new(Vec::new()))
}

/// 启动 mock 上游，返回 base_url（http://127.0.0.1:port）。
/// `captured` 记录请求体（JSON），`captured_headers` 记录每个请求的 HeaderMap
/// （同名多值会保留多行，供重复断言）。
async fn spawn_mock_with_headers(captured: Captured, captured_headers: CapturedHeaders) -> String {
    let captured_chat = captured.clone();
    let captured_messages = captured.clone();
    let captured_responses = captured.clone();
    let captured_gemini = captured.clone();
    let captured_stream_gemini = captured.clone();
    let headers_chat = captured_headers.clone();
    let headers_messages = captured_headers.clone();
    let headers_responses = captured_headers.clone();
    let headers_gemini = captured_headers.clone();
    let headers_stream_gemini = captured_headers.clone();

    let app = Router::new()
        .route(
            "/v1/chat/completions",
            post(move |request: Request<Body>| {
                let captured = captured_chat.clone();
                let captured_headers = headers_chat.clone();
                async move {
                    let headers = request.headers().clone();
                    captured_headers.lock().unwrap().push(headers);
                    let body = axum::body::to_bytes(request.into_body(), usize::MAX)
                        .await
                        .unwrap();
                    let body = String::from_utf8_lossy(&body).to_string();
                    record_capture(&captured, &body);
                    let parsed: Value = serde_json::from_str(&body).unwrap();
                    if parsed["stream"] == json!(true) {
                        sse(&[
                            json!({"id":"chatcmpl-m1","object":"chat.completion.chunk","model":"m1","choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}).to_string(),
                            json!({"id":"chatcmpl-m1","object":"chat.completion.chunk","model":"m1","choices":[{"index":0,"delta":{"content":"你好"},"finish_reason":null}]}).to_string(),
                            json!({"id":"chatcmpl-m1","object":"chat.completion.chunk","model":"m1","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}).to_string(),
                            json!({"id":"chatcmpl-m1","object":"chat.completion.chunk","model":"m1","choices":[],"usage":{"prompt_tokens":11,"completion_tokens":3,"total_tokens":14,"prompt_tokens_details":{"cached_tokens":4}}}).to_string(),
                        ])
                    } else {
                        Json(json!({
                            "id": "chatcmpl-m1",
                            "object": "chat.completion",
                            "model": "m1",
                            "choices": [{"index": 0, "message": {"role": "assistant", "content": "你好"}, "finish_reason": "stop"}],
                            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15, "prompt_tokens_details": {"cached_tokens": 4}}
                        }))
                        .into_response()
                    }
                }
            }),
        )
        .route(
            "/v1/messages",
            post(move |request: Request<Body>| {
                let captured = captured_messages.clone();
                let captured_headers = headers_messages.clone();
                async move {
                    let headers = request.headers().clone();
                    captured_headers.lock().unwrap().push(headers);
                    let body = axum::body::to_bytes(request.into_body(), usize::MAX)
                        .await
                        .unwrap();
                    let body = String::from_utf8_lossy(&body).to_string();
                    record_capture(&captured, &body);
                    let parsed: Value = serde_json::from_str(&body).unwrap();
                    // 触发器：返回带 signature 的 thinking 块（reasoning_details 往返用）。
                    if parsed.pointer("/messages/0/content/0/text")
                        == Some(&json!("think-signature"))
                    {
                        return Json(json!({
                            "id": "msg_sig",
                            "content": [
                                {"type": "thinking", "thinking": "想一想", "signature": "sig-abc"},
                                {"type": "text", "text": "你好"}
                            ],
                            "stop_reason": "end_turn",
                            "usage": {"input_tokens": 10, "output_tokens": 5}
                        }))
                        .into_response();
                    }
                    // 回归触发器：流中夹带畸形事件（非 JSON 的 data 行），
                    // 转换失败必须按失败记账（不得假成功）。
                    if parsed.pointer("/messages/0/content/0/text")
                        == Some(&json!("malformed-stream"))
                    {
                        let mut payload = String::new();
                        for event in [
                            json!({"type":"message_start","message":{"id":"msg_1","usage":{"input_tokens":10}}}).to_string(),
                            json!({"type":"content_block_start","index":0,"content_block":{"type":"text"}}).to_string(),
                        ] {
                            payload.push_str(&format!("data: {event}\n\n"));
                        }
                        payload.push_str("data: {broken\n\n");
                        return (
                            HttpStatus::OK,
                            [("content-type", "text/event-stream")],
                            payload,
                        )
                            .into_response();
                    }
                    // 回归触发器：200 SSE 流内错误事件（带内 error 帧）——
                    // 客户端必须收到 error 帧 + [DONE]，不得假成功（03-01）。
                    if parsed.pointer("/messages/0/content/0/text")
                        == Some(&json!("inband-error"))
                    {
                        let payload = [
                            json!({"type":"message_start","message":{"id":"msg_1","usage":{"input_tokens":10}}}).to_string(),
                            json!({"type":"error","error":{"type":"overloaded_error","message":"上游过载"}}).to_string(),
                        ]
                        .into_iter()
                        .fold(String::new(), |mut acc, event| {
                            acc.push_str(&format!("data: {event}\n\n"));
                            acc
                        });
                        return (
                            HttpStatus::OK,
                            [("content-type", "text/event-stream")],
                            payload,
                        )
                            .into_response();
                    }
                    if parsed["stream"] == json!(true) {
                        sse(&[
                            json!({"type":"message_start","message":{"id":"msg_1","usage":{"input_tokens":10,"cache_read_input_tokens":3,"cache_creation_input_tokens":2}}}).to_string(),
                            json!({"type":"content_block_start","index":0,"content_block":{"type":"text"}}).to_string(),
                            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"你好"}}).to_string(),
                            json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":5}}).to_string(),
                            json!({"type":"message_stop"}).to_string(),
                        ])
                    } else {
                        Json(json!({
                            "id": "msg_1",
                            "content": [{"type": "text", "text": "你好"}],
                            "stop_reason": "end_turn",
                            "usage": {"input_tokens": 10, "output_tokens": 5, "cache_read_input_tokens": 3, "cache_creation_input_tokens": 2}
                        }))
                        .into_response()
                    }
                }
            }),
        )
        .route(
            "/v1/responses",
            post(move |request: Request<Body>| {
                let captured = captured_responses.clone();
                let captured_headers = headers_responses.clone();
                async move {
                    let headers = request.headers().clone();
                    captured_headers.lock().unwrap().push(headers);
                    let body = axum::body::to_bytes(request.into_body(), usize::MAX)
                        .await
                        .unwrap();
                    let body = String::from_utf8_lossy(&body).to_string();
                    record_capture(&captured, &body);
                    let parsed: Value = serde_json::from_str(&body).unwrap();
                    // 触发器：返回带 encrypted_content 的 reasoning item（往返用）。
                    if parsed.pointer("/input/0/content/0/text") == Some(&json!("encrypted-only"))
                    {
                        return sse(&[
                            json!({"type":"response.created","response":{"id":"resp_enc","model":"enc"}}).to_string(),
                            json!({"type":"response.completed","response":{"status":"completed","output":[{"type":"reasoning","id":"rs_enc","summary":[{"type":"summary_text","text":"推理"}],"encrypted_content":"gAAA-enc"}],"usage":{"input_tokens":5,"output_tokens":3}}}).to_string(),
                        ]);
                    }
                    if parsed.pointer("/input/0/content/0/text") == Some(&json!("final-only")) {
                        return sse(&[
                            json!({"type":"response.created","response":{"id":"resp_final","model":"final-only"}}).to_string(),
                            json!({"type":"response.completed","response":{"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"最终内容"},{"type":"reasoning","summary":[{"type":"summary_text","text":"最终推理"}]}]}],"usage":{"input_tokens":12,"output_tokens":6,"input_tokens_details":{"cached_tokens":5}}}}).to_string(),
                        ]);
                    }
                    // 回归触发器：200 SSE 流内 response.failed 事件（带内错误）——
                    // 客户端必须收到 error 帧 + [DONE]，不得假成功（03-07）。
                    if parsed.pointer("/input/0/content/0/text") == Some(&json!("inband-error")) {
                        return sse(&[
                            json!({"type":"response.created","response":{"id":"resp_err","model":"gpt-x"}}).to_string(),
                            json!({"type":"response.failed","response":{"status":"failed","error":{"message":"上游过载"}}}).to_string(),
                        ]);
                    }
                    sse(&[
                        json!({"type":"response.created","response":{"id":"resp_1","model":"gpt-x"}}).to_string(),
                        json!({"type":"response.output_text.delta","delta":"你好"}).to_string(),
                        json!({"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":12,"output_tokens":6,"input_tokens_details":{"cached_tokens":5}}}}).to_string(),
                    ])
                }
            }),
        )
        .route(
            "/v1beta/models/m-1:generateContent",
            post(move |request: Request<Body>| {
                let captured = captured_gemini.clone();
                let captured_headers = headers_gemini.clone();
                async move {
                    let headers = request.headers().clone();
                    captured_headers.lock().unwrap().push(headers);
                    let body = axum::body::to_bytes(request.into_body(), usize::MAX)
                        .await
                        .unwrap();
                    let body = String::from_utf8_lossy(&body).to_string();
                    record_capture(&captured, &body);
                    let parsed: Value = serde_json::from_str(&body).unwrap();
                    // 触发器：返回带 thoughtSignature 的 functionCall part（往返用）。
                    if parsed.pointer("/contents/0/parts/0/text")
                        == Some(&json!("tool-call-please"))
                    {
                        return Json(json!({
                            "candidates": [{"content": {"parts": [{"functionCall": {"name": "get_weather", "args": {"city": "sf"}}, "thoughtSignature": "sig-gem"}], "role": "model"}, "finishReason": "STOP"}],
                            "usageMetadata": {"promptTokenCount": 10, "candidatesTokenCount": 4, "totalTokenCount": 14}
                        }))
                        .into_response();
                    }
                    Json(json!({
                        "candidates": [{"content": {"parts": [{"text": "你好"}], "role": "model"}, "finishReason": "STOP"}],
                        "usageMetadata": {"promptTokenCount": 10, "candidatesTokenCount": 4, "thoughtsTokenCount": 2, "cachedContentTokenCount": 6, "totalTokenCount": 16}
                    }))
                    .into_response()
                }
            }),
        )
        .route(
            "/v1beta/models/m-1:streamGenerateContent",
            post(move |request: Request<Body>| {
                let _ = captured_stream_gemini;
                let captured_headers = headers_stream_gemini.clone();
                async move {
                    let headers = request.headers().clone();
                    captured_headers.lock().unwrap().push(headers);
                    let body = axum::body::to_bytes(request.into_body(), usize::MAX)
                        .await
                        .unwrap();
                    let parsed: Value = serde_json::from_slice(&body).unwrap();
                    // 回归触发器：200 SSE 流内 {"error":…} 事件（带内错误）——
                    // 客户端必须收到 error 帧 + [DONE]，不得假成功（03-07）。
                    if parsed.pointer("/contents/0/parts/0/text") == Some(&json!("inband-error")) {
                        return sse(&[
                            json!({"candidates":[{"content":{"parts":[{"text":"部分"}],"role":"model"}}],"modelVersion":"gemini-x"}).to_string(),
                            json!({"error":{"code":503,"message":"上游过载","status":"UNAVAILABLE"}}).to_string(),
                        ]);
                    }
                    sse(&[
                        json!({"candidates":[{"content":{"parts":[{"text":"你好"}],"role":"model"}}],"modelVersion":"gemini-x"}).to_string(),
                        json!({"candidates":[{"content":{"parts":[],"role":"model"},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":10,"candidatesTokenCount":4,"thoughtsTokenCount":2,"cachedContentTokenCount":6}}).to_string(),
                    ])
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

/// 不带 header 捕获的兼容封装（大多数既有用例仍用 body 捕获）。
async fn spawn_mock(captured: Captured) -> String {
    spawn_mock_with_headers(captured, capture_headers()).await
}

// ---------- 测试基建 ----------

async fn seed_provider(
    db: &sea_orm::DatabaseConnection,
    name: &str,
    base_url: &str,
    protocol_type: i32,
    billing_mode: i32,
) -> i32 {
    let active = provider::ActiveModel {
        name: Set(name.to_string()),
        enable: Set(true),
        base_url: Set(base_url.to_string()),
        api_key: Set(llm_gateway::crypto::encrypt("sk-mock")),
        custom_header: Set("{}".to_string()),
        protocol_type: Set(protocol_type),
        billing_mode: Set(billing_mode),
        extra: Set("{}".to_string()),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    active.insert(db).await.unwrap().id
}

async fn seed_provider_model(
    db: &sea_orm::DatabaseConnection,
    provider_id: i32,
    remote_id: &str,
) -> i32 {
    seed_provider_model_with_protocol(db, provider_id, remote_id, None).await
}

async fn seed_provider_model_with_protocol(
    db: &sea_orm::DatabaseConnection,
    provider_id: i32,
    remote_id: &str,
    protocol_type: Option<i32>,
) -> i32 {
    let active = provider_model::ActiveModel {
        provider_id: Set(provider_id),
        provider_model_id: Set(remote_id.to_string()),
        context_length: Set(128000),
        max_output_tokens: Set(4096),
        reasoning: Set(false),
        tool_use: Set(true),
        image_understand: Set(false),
        video_understand: Set(false),
        protocol_type: Set(protocol_type),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    active.insert(db).await.unwrap().model_id
}

async fn send_chat(app: &axum::Router, body: Value) -> (u16, String) {
    let request = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .header("authorization", TEST_BEARER)
        .body(Body::from(body.to_string()))
        .unwrap();
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
    let text = String::from_utf8_lossy(&bytes).to_string();
    let _ = content_type;
    (status, text)
}

/// 等待 request 表出现记录：订阅落库事件做同步（事件先于订阅到达的行由
/// 首查存量覆盖，事件唤醒后重查），超时兜底与旧轮询预算耗尽同行为。
async fn wait_for_records(
    db: &sea_orm::DatabaseConnection,
    expected: usize,
) -> Vec<request::Model> {
    let mut rx = llm_gateway::proxy::metrics::subscribe_request_writes();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let rows = request::Entity::find().all(db).await.unwrap();
        if rows.len() >= expected {
            return rows;
        }
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return rows;
        }
        // 任意落库事件（含其他并发测试的）都触发一次重查；Lagged 同样重查。
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Ok(())) | Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => {}
            Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) | Err(_) => return rows,
        }
    }
}

fn chat_body(model: &str, stream: bool) -> Value {
    json!({
        "model": model,
        "stream": stream,
        "messages": [{"role": "user", "content": "hi"}],
        "max_tokens": 128,
    })
}

/// 组合助手：一个 OpenAI 成员 + 虚拟模型 vm-x（策略可调）。
async fn common_setup_with_member(
    base_url: &str,
    protocol_type: i32,
    load_balancing_strategy: i32,
    billing_mode: i32,
) -> (axum::Router, sea_orm::DatabaseConnection) {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    scheduler.start().await.unwrap();
    let app = common::build_authed_app(db.clone(), scheduler, log_tx).await;
    let provider_id = seed_provider(&db, "p-1", base_url, protocol_type, billing_mode).await;
    let model_id = seed_provider_model(&db, provider_id, "m-1").await;
    let vm = virtual_model::ActiveModel {
        display_id: Set("vm-x".to_string()),
        enable: Set(true),
        load_balancing_strategy: Set(load_balancing_strategy),
        fallback_strategy: Set(1),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    let vm = vm.insert(&db).await.unwrap();
    virtual_model_item::ActiveModel {
        virtual_model_id: Set(vm.virtual_model_id),
        model_id: Set(model_id),
        enable: Set(true),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    (app, db)
}

/// 组合助手：供应商 + 模型 + 虚拟模型（模型可单独指定协议，None=跟随供应商）。
async fn common_setup_with_model_protocol(
    base_url: &str,
    provider_protocol: i32,
    model_protocol: Option<i32>,
) -> (axum::Router, sea_orm::DatabaseConnection) {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    scheduler.start().await.unwrap();
    let app = common::build_authed_app(db.clone(), scheduler, log_tx).await;
    let provider_id = seed_provider(&db, "p-1", base_url, provider_protocol, 0).await;
    let model_id = seed_provider_model_with_protocol(&db, provider_id, "m-1", model_protocol).await;
    let vm = virtual_model::ActiveModel {
        display_id: Set("vm-x".to_string()),
        enable: Set(true),
        load_balancing_strategy: Set(0),
        fallback_strategy: Set(1),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    let vm = vm.insert(&db).await.unwrap();
    virtual_model_item::ActiveModel {
        virtual_model_id: Set(vm.virtual_model_id),
        model_id: Set(model_id),
        enable: Set(true),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    (app, db)
}

/// 种入「用量缓存显示订阅窗口已耗尽」的订阅制供应商 + 模型 + 虚拟模型。
async fn seed_exhausted_subscription(
    db: &sea_orm::DatabaseConnection,
    base_url: &str,
    name: &str,
    protocol_type: i32,
    display_id: &str,
    interface_type: i32,
) {
    let provider = seed_provider(db, name, base_url, protocol_type, 1).await;
    let model = seed_provider_model(db, provider, "m-x").await;
    let now = chrono::Utc::now();
    llm_gateway::usage::persist::write_usage_cache(
        db,
        &llm_gateway::usage::types::UsageData {
            provider_id: provider,
            fetched_at: now,
            kind: llm_gateway::usage::types::UsageKind::Quota,
            plan: None,
            windows: vec![
                llm_gateway::usage::types::QuotaWindow::from_remaining_percent(
                    llm_gateway::usage::types::WindowKind::FiveHour,
                    0.0,
                    None,
                ),
            ],
            balances: vec![],
        },
    )
    .await
    .unwrap();
    let vm = virtual_model::ActiveModel {
        display_id: Set(display_id.to_string()),
        enable: Set(true),
        interface_type: Set(interface_type),
        load_balancing_strategy: Set(0),
        fallback_strategy: Set(1),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };
    let vm = vm.insert(db).await.unwrap();
    virtual_model_item::ActiveModel {
        virtual_model_id: Set(vm.virtual_model_id),
        model_id: Set(model),
        enable: Set(true),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap();
}

// ─── 上游出站头：透传 / 剥离 / 覆盖集成测试 ──────────────────────────────────

/// seed provider，可指定 custom_header（默认 "{}"）。
async fn seed_provider_with_custom_header(
    db: &sea_orm::DatabaseConnection,
    name: &str,
    base_url: &str,
    protocol_type: i32,
    billing_mode: i32,
    custom_header: &str,
) -> i32 {
    let active = provider::ActiveModel {
        name: Set(name.to_string()),
        enable: Set(true),
        base_url: Set(base_url.to_string()),
        api_key: Set(llm_gateway::crypto::encrypt("sk-mock")),
        custom_header: Set(custom_header.to_string()),
        protocol_type: Set(protocol_type),
        billing_mode: Set(billing_mode),
        extra: Set("{}".to_string()),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    active.insert(db).await.unwrap().id
}

/// 带额外请求头发送 /v1/chat/completions（鉴权头固定 TEST_BEARER）。
async fn send_chat_with_headers(
    app: &axum::Router,
    body: Value,
    extra_headers: &[(&str, &str)],
) -> (u16, String) {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .header("authorization", TEST_BEARER);
    for (k, v) in extra_headers {
        builder = builder.header(*k, *v);
    }
    let request = builder.body(Body::from(body.to_string())).unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status().as_u16();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&bytes).to_string())
}

/// 直接手建「1 个 OpenAI 成员 + vm-x」，允许 custom_header（不经 seed_provider 默认）。
async fn setup_member_with_custom_header(
    base_url: &str,
    protocol_type: i32,
    custom_header: &str,
) -> (axum::Router, sea_orm::DatabaseConnection) {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    scheduler.start().await.unwrap();
    let app = common::build_authed_app(db.clone(), scheduler, log_tx).await;
    let provider_id =
        seed_provider_with_custom_header(&db, "p-1", base_url, protocol_type, 0, custom_header)
            .await;
    let model_id = seed_provider_model(&db, provider_id, "m-1").await;
    let vm = virtual_model::ActiveModel {
        display_id: Set("vm-x".to_string()),
        enable: Set(true),
        load_balancing_strategy: Set(0),
        fallback_strategy: Set(1),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    let vm = vm.insert(&db).await.unwrap();
    virtual_model_item::ActiveModel {
        virtual_model_id: Set(vm.virtual_model_id),
        model_id: Set(model_id),
        enable: Set(true),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    (app, db)
}

/// 从捕获的上游 HeaderMap 中取某名首个值（小写匹配）。
fn header_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

/// 断言某名在捕获的上游头中只出现一次。
fn header_count(headers: &HeaderMap, name: &str) -> usize {
    headers.get_all(name).iter().count()
}

// ---------- /v1/messages 原生透传 ----------

/// 组合助手（原生透传）：供应商 + 模型 + 指定接口类型的虚拟模型。
async fn common_setup_native(
    base_url: &str,
    provider_protocol: i32,
    interface_type: i32,
    display_id: &str,
) -> (axum::Router, sea_orm::DatabaseConnection) {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    scheduler.start().await.unwrap();
    let app = common::build_authed_app(db.clone(), scheduler, log_tx).await;
    seed_native_fixture(&db, base_url, provider_protocol, interface_type, display_id).await;
    (app, db)
}

/// 在给定 db 上种入供应商 + 模型 + 指定接口类型的虚拟模型。
async fn seed_native_fixture(
    db: &sea_orm::DatabaseConnection,
    base_url: &str,
    provider_protocol: i32,
    interface_type: i32,
    display_id: &str,
) {
    let provider_id = seed_provider(db, "p-native", base_url, provider_protocol, 0).await;
    let model_id = seed_provider_model(db, provider_id, "m-1").await;
    let vm = virtual_model::ActiveModel {
        display_id: Set(display_id.to_string()),
        enable: Set(true),
        interface_type: Set(interface_type),
        load_balancing_strategy: Set(0),
        fallback_strategy: Set(1),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    let vm = vm.insert(db).await.unwrap();
    virtual_model_item::ActiveModel {
        virtual_model_id: Set(vm.virtual_model_id),
        model_id: Set(model_id),
        enable: Set(true),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap();
}

/// 发送 /v1 原生请求（可带额外头；鉴权默认 Bearer）。
async fn send_native(
    app: &axum::Router,
    uri: &str,
    body: Value,
    extra_headers: &[(&str, &str)],
) -> (u16, String, String) {
    let mut builder = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .header("authorization", TEST_BEARER);
    for (name, value) in extra_headers {
        builder = builder.header(*name, *value);
    }
    let request = builder.body(Body::from(body.to_string())).unwrap();
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
        String::from_utf8_lossy(&bytes).to_string(),
        content_type,
    )
}

fn messages_body(model: &str, stream: bool) -> Value {
    json!({
        "model": model,
        "stream": stream,
        "max_tokens": 99,
        "system": "be nice",
        "messages": [{"role": "user", "content": "hi"}],
    })
}

// ---------- /v1/responses 原生透传 ----------

fn responses_body(model: &str) -> Value {
    json!({
        "model": model,
        "stream": true,
        "input": [{"role": "user", "content": [{"type": "input_text", "text": "hi"}]}],
    })
}

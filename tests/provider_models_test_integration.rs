//! 供应商模型「测试」端点集成测试：mock 上游覆盖四协议成功落库、
//! 上游错误返回信封错误并落库、连接失败等场景。
//! mock 上游用 Mutex 记录请求体，锁跨 await 持有是测试有意为之；assert_eq 布尔比较改 assert 更清晰。
#![allow(clippy::await_holding_lock, clippy::bool_assert_comparison)]

mod common;

use std::sync::{Arc, Mutex};

use axum::{
    Json, Router,
    body::Body,
    http::{Request, StatusCode},
    response::IntoResponse,
    routing::post,
};
use sea_orm::{ActiveModelTrait, EntityTrait, Set};
use serde_json::{Value, json};
use tower::ServiceExt;

use llm_gateway::crypto;
use llm_gateway::entity::provider;
use llm_gateway::entity::provider_model;
use llm_gateway::entity::request;

type Captured = Arc<Mutex<Vec<Value>>>;

fn capture() -> Captured {
    Arc::new(Mutex::new(Vec::new()))
}

fn record_capture(captured: &Captured, body: &str) {
    if let Ok(value) = serde_json::from_str::<Value>(body) {
        captured.lock().unwrap().push(value);
    }
}

/// 启动一个 mock 上游：`/v1/chat/completions`（OpenAI）、`/v1/messages`
/// （Anthropic）、`/v1/responses`（Responses，SSE）、`/v1beta/models/m-1:generateContent`
/// （Gemini 非流式）。返回 base_url。
async fn spawn_mock(captured: Captured) -> String {
    let chat_captured = captured.clone();
    let messages_captured = captured.clone();
    let responses_captured = captured.clone();
    let gemini_captured = captured.clone();

    let app = Router::new()
        .route(
            "/v1/chat/completions",
            post(move |body: String| {
                let captured = chat_captured.clone();
                async move {
                    record_capture(&captured, &body);
                    Json(json!({
                        "id": "chatcmpl-test",
                        "object": "chat.completion",
                        "model": "m-1",
                        "choices": [{"index": 0, "message": {"role": "assistant", "content": "你好"}, "finish_reason": "stop"}],
                        "usage": {"prompt_tokens": 3, "completion_tokens": 2, "total_tokens": 5}
                    }))
                    .into_response()
                }
            }),
        )
        .route(
            "/v1/messages",
            post(move |body: String| {
                let captured = messages_captured.clone();
                async move {
                    record_capture(&captured, &body);
                    Json(json!({
                        "id": "msg_test",
                        "content": [{"type": "text", "text": "你好"}],
                        "stop_reason": "end_turn",
                        "usage": {"input_tokens": 3, "output_tokens": 2}
                    }))
                    .into_response()
                }
            }),
        )
        .route(
            "/v1/responses",
            post(move |body: String| {
                let captured = responses_captured.clone();
                async move {
                    record_capture(&captured, &body);
                    let payload = concat!(
                        "data: ",
                        r#"{"type":"response.created","response":{"id":"resp_1","model":"m-1"}}"#,
                        "\n\n",
                        "data: ",
                        r#"{"type":"response.output_text.delta","delta":"你好"}"#,
                        "\n\n",
                        "data: ",
                        r#"{"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":3,"output_tokens":2}}}"#,
                        "\n\n",
                        "data: [DONE]\n\n",
                    );
                    (
                        StatusCode::OK,
                        [("content-type", "text/event-stream")],
                        payload,
                    )
                        .into_response()
                }
            }),
        )
        .route(
            "/v1beta/models/m-1:generateContent",
            post(move |body: String| {
                let captured = gemini_captured.clone();
                async move {
                    record_capture(&captured, &body);
                    Json(json!({
                        "candidates": [{"content": {"parts": [{"text": "你好"}], "role": "model"}, "finishReason": "STOP"}],
                        "usageMetadata": {"promptTokenCount": 3, "candidatesTokenCount": 2, "totalTokenCount": 5}
                    }))
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

async fn setup_app() -> (axum::Router, sea_orm::DatabaseConnection) {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    scheduler.start().await.unwrap();
    let app = common::build_authed_app(db.clone(), scheduler, log_tx).await;
    (app, db)
}

/// 种入 provider + provider_model（默认 OpenAI 协议），返回 (provider_id, model_id)。
async fn seed_provider_and_model(
    db: &sea_orm::DatabaseConnection,
    base_url: &str,
    protocol_type: i32,
) -> (i32, i32) {
    let provider_id = provider::ActiveModel {
        name: Set(format!("p-test-{protocol_type}")),
        enable: Set(true),
        base_url: Set(base_url.to_string()),
        api_key: Set(crypto::encrypt("sk-test")),
        custom_header: Set("{}".to_string()),
        protocol_type: Set(protocol_type),
        billing_mode: Set(0),
        extra: Set("{}".to_string()),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap()
    .id;

    let model_id = provider_model::ActiveModel {
        provider_id: Set(provider_id),
        provider_model_id: Set("m-1".to_string()),
        context_length: Set(128000),
        max_output_tokens: Set(4096),
        reasoning: Set(false),
        tool_use: Set(true),
        image_understand: Set(false),
        video_understand: Set(false),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap()
    .model_id;

    (provider_id, model_id)
}

async fn call_test(app: &axum::Router, provider_id: i32, model_id: i32) -> (u16, Value) {
    let request = Request::builder()
        .method("POST")
        .uri(format!(
            "/api/providers/{provider_id}/models/{model_id}/test"
        ))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status().as_u16();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let parsed: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, parsed)
}

/// 等待 request 表出现 n 条记录（落库为异步任务）。
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
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    request::Entity::find().all(db).await.unwrap()
}

#[tokio::test]
async fn test_openai_model_success_records_request() {
    let captured = capture();
    let base = spawn_mock(captured.clone()).await;
    let (app, db) = setup_app().await;
    let (provider_id, model_id) = seed_provider_and_model(&db, &base, 0).await;

    let (status, body) = call_test(&app, provider_id, model_id).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["code"], "0");
    assert_eq!(body["data"]["ok"], true);

    // 上游收到固定提示词「你好」+ max_tokens 映射。
    let upstream_bodies = captured.lock().unwrap();
    assert_eq!(upstream_bodies[0]["messages"][0]["content"], "你好");
    assert_eq!(upstream_bodies[0]["model"], "m-1");
    assert_eq!(upstream_bodies[0]["stream"], false);
    assert_eq!(upstream_bodies[0]["max_tokens"], 4096);
    drop(upstream_bodies);

    let rows = wait_for_records(&db, 1).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].success, true);
    assert_eq!(rows[0].provider_id, provider_id);
    assert_eq!(rows[0].model_id, "m-1");
    assert_eq!(rows[0].virtual_model_id, 0, "测试流量不归属任何虚拟模型");
    assert_eq!(rows[0].api_key_name, "test");
    assert_eq!(rows[0].input_tokens, Some(3));
    assert_eq!(rows[0].output_tokens, Some(2));
    assert!(rows[0].fail_reason.is_none());
}

#[tokio::test]
async fn test_anthropic_model_success() {
    let captured = capture();
    let base = spawn_mock(captured.clone()).await;
    let (app, db) = setup_app().await;
    let (provider_id, model_id) = seed_provider_and_model(&db, &base, 2).await;

    let (status, body) = call_test(&app, provider_id, model_id).await;
    assert_eq!(status, 200, "{body}");

    let upstream_bodies = captured.lock().unwrap();
    assert_eq!(upstream_bodies[0]["messages"][0]["role"], "user");
    assert_eq!(upstream_bodies[0]["max_tokens"], 4096);
    drop(upstream_bodies);

    let rows = wait_for_records(&db, 1).await;
    assert_eq!(rows[0].success, true);
    assert_eq!(rows[0].input_tokens, Some(3));
    assert_eq!(rows[0].output_tokens, Some(2));
}

#[tokio::test]
async fn test_gemini_model_success() {
    let captured = capture();
    let base = spawn_mock(captured.clone()).await;
    let (app, db) = setup_app().await;
    let (provider_id, model_id) = seed_provider_and_model(&db, &base, 3).await;

    let (status, body) = call_test(&app, provider_id, model_id).await;
    assert_eq!(status, 200, "{body}");

    let upstream_bodies = captured.lock().unwrap();
    assert_eq!(
        upstream_bodies[0]["generationConfig"]["maxOutputTokens"],
        4096
    );
    drop(upstream_bodies);

    let rows = wait_for_records(&db, 1).await;
    assert_eq!(rows[0].success, true);
    assert_eq!(rows[0].input_tokens, Some(3));
    assert_eq!(rows[0].output_tokens, Some(2));
}

#[tokio::test]
async fn test_responses_model_success_parses_sse_usage() {
    let captured = capture();
    let base = spawn_mock(captured.clone()).await;
    let (app, db) = setup_app().await;
    let (provider_id, model_id) = seed_provider_and_model(&db, &base, 1).await;

    let (status, body) = call_test(&app, provider_id, model_id).await;
    assert_eq!(status, 200, "{body}");

    let upstream_bodies = captured.lock().unwrap();
    assert_eq!(upstream_bodies[0]["stream"], true, "Responses 上游强制流式");
    drop(upstream_bodies);

    let rows = wait_for_records(&db, 1).await;
    assert_eq!(rows[0].success, true);
    assert_eq!(rows[0].input_tokens, Some(3));
    assert_eq!(rows[0].output_tokens, Some(2));
}

#[tokio::test]
async fn test_upstream_http_error_returns_envelope_and_records_failure() {
    let fail_router = Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            (
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({"error": {"message": "rate limited"}})),
            )
                .into_response()
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let fail_addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, fail_router).await.unwrap();
    });
    let base = format!("http://{fail_addr}");

    let (app, db) = setup_app().await;
    let (provider_id, model_id) = seed_provider_and_model(&db, &base, 0).await;

    let (status, body) = call_test(&app, provider_id, model_id).await;
    assert_eq!(status, 502, "{body}");
    assert_ne!(body["code"], "0");
    let msg = body["msg"].as_str().unwrap_or("");
    assert!(msg.contains("429"), "失败消息应含 HTTP 状态码：{msg}");
    assert!(msg.contains("rate limited"), "失败消息应含上游原文：{msg}");

    let rows = wait_for_records(&db, 1).await;
    assert_eq!(rows[0].success, false);
    assert!(
        rows[0]
            .fail_reason
            .as_deref()
            .unwrap_or("")
            .contains("rate limited")
    );
}

#[tokio::test]
async fn test_connection_failure_records_failure() {
    // 占用一个端口后立即释放，得到几乎必然连不上的地址。
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let base = format!("http://{addr}");

    let (app, db) = setup_app().await;
    let (provider_id, model_id) = seed_provider_and_model(&db, &base, 0).await;

    let (status, body) = call_test(&app, provider_id, model_id).await;
    assert_eq!(status, 502, "{body}");
    assert_ne!(body["code"], "0");

    let rows = wait_for_records(&db, 1).await;
    assert_eq!(rows[0].success, false);
    assert!(
        rows[0]
            .fail_reason
            .as_deref()
            .unwrap_or("")
            .contains("连接")
    );
}

#[tokio::test]
async fn test_no_api_key_returns_400_without_record() {
    let base = spawn_mock(capture()).await;
    let (app, db) = setup_app().await;
    let provider_id = provider::ActiveModel {
        name: Set("p-no-key".to_string()),
        enable: Set(true),
        base_url: Set(base.to_string()),
        api_key: Set(String::new()),
        custom_header: Set("{}".to_string()),
        protocol_type: Set(0),
        billing_mode: Set(0),
        extra: Set("{}".to_string()),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap()
    .id;
    let model_id = provider_model::ActiveModel {
        provider_id: Set(provider_id),
        provider_model_id: Set("m-1".to_string()),
        context_length: Set(128000),
        max_output_tokens: Set(4096),
        reasoning: Set(false),
        tool_use: Set(false),
        image_understand: Set(false),
        video_understand: Set(false),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap()
    .model_id;

    let (status, body) = call_test(&app, provider_id, model_id).await;
    assert_eq!(status, 400, "{body}");
    assert!(body["msg"].as_str().unwrap_or("").contains("API Key"));

    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let rows = request::Entity::find().all(&db).await.unwrap();
    assert!(rows.is_empty(), "未配置 API Key 不发请求、不落表");
}

#[tokio::test]
async fn test_missing_model_returns_404() {
    let base = spawn_mock(capture()).await;
    let (app, db) = setup_app().await;
    let (provider_id, _) = seed_provider_and_model(&db, &base, 0).await;

    let (status, body) = call_test(&app, provider_id, 9999).await;
    assert_eq!(status, 404, "{body}");
    assert_ne!(body["code"], "0");
}

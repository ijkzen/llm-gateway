//! /v1 转发集成测试：本地 mock 上游覆盖各协议转换、failover 与 request 表落库。

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode as HttpStatus};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde_json::{Value, json};
use tower::ServiceExt;

use llm_gateway::entity::request;
use llm_gateway::entity::{provider, provider_model, virtual_model, virtual_model_item};

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

/// 启动 mock 上游，返回 base_url（http://127.0.0.1:port）。
async fn spawn_mock(captured: Captured) -> String {
    let captured_chat = captured.clone();
    let captured_messages = captured.clone();
    let captured_responses = captured.clone();
    let captured_gemini = captured.clone();
    let captured_stream_gemini = captured.clone();

    let app = Router::new()
        .route(
            "/v1/chat/completions",
            post(move |body: String| {
                let captured = captured_chat.clone();
                async move {
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
            post(move |body: String| {
                let captured = captured_messages;
                async move {
                    record_capture(&captured, &body);
                    let parsed: Value = serde_json::from_str(&body).unwrap();
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
            post(move |body: String| {
                let captured = captured_responses;
                async move {
                    record_capture(&captured, &body);
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
            post(move |body: String| {
                let captured = captured_gemini;
                async move {
                    record_capture(&captured, &body);
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
            post(move |body: String| {
                let _ = captured_stream_gemini;
                async move {
                    let _ = body;
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
        status: Set(0),
        protocol_type: Set(protocol_type),
        billing_mode: Set(billing_mode),
        extra: Set("{}".to_string()),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    active.insert(db).await.unwrap().id
}

async fn seed_provider_model(db: &sea_orm::DatabaseConnection, provider_id: i32, remote_id: &str) -> i32 {
    let active = provider_model::ActiveModel {
        provider_id: Set(provider_id),
        provider_model_id: Set(remote_id.to_string()),
        context_length: Set(128000),
        max_output_tokens: Set(4096),
        reasoning: Set(false),
        tool_use: Set(true),
        image_understand: Set(false),
        video_understand: Set(false),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    active.insert(db).await.unwrap().model_id
}

/// 建虚拟模型并绑定成员，返回 (router, db)。
async fn setup_virtual_model(
    display_id: &str,
    load_balancing_strategy: i32,
    fallback_strategy: i32,
    members: &[i32],
) -> (axum::Router, sea_orm::DatabaseConnection) {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    scheduler.start().await.unwrap();
    let app = common::build_authed_app(db.clone(), scheduler, log_tx).await;

    let vm = virtual_model::ActiveModel {
        display_id: Set(display_id.to_string()),
        enable: Set(true),
        load_balancing_strategy: Set(load_balancing_strategy),
        fallback_strategy: Set(fallback_strategy),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    let vm = vm.insert(&db).await.unwrap();
    for model_id in members {
        let item = virtual_model_item::ActiveModel {
            virtual_model_id: Set(vm.virtual_model_id),
            model_id: Set(*model_id),
            enable: Set(true),
            created_at: Set(chrono::Utc::now()),
            updated_at: Set(chrono::Utc::now()),
            ..Default::default()
        };
        item.insert(&db).await.unwrap();
    }
    (app, db)
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
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let text = String::from_utf8_lossy(&bytes).to_string();
    let _ = content_type;
    (status, text)
}

/// 等待 request 表出现记录（落库为异步任务）。
async fn wait_for_records(db: &sea_orm::DatabaseConnection, expected: usize) -> Vec<request::Model> {
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

fn chat_body(model: &str, stream: bool) -> Value {
    json!({
        "model": model,
        "stream": stream,
        "messages": [{"role": "user", "content": "hi"}],
        "max_tokens": 128,
    })
}

// ---------- 用例 ----------

#[tokio::test]
async fn openai_passthrough_non_stream_and_record() {
    let captured = capture();
    let base = spawn_mock(captured).await;
    let (app, db) = common_setup_with_member(&base, 0, 0, 0).await;

    let (status, text) = send_chat(&app, chat_body("vm-x", false)).await;
    assert_eq!(status, 200, "{text}");
    let body: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(body["object"], "chat.completion");
    assert_eq!(body["choices"][0]["message"]["content"], "你好");
    assert_eq!(body["usage"]["prompt_tokens"], 10);

    let rows = wait_for_records(&db, 1).await;
    assert_eq!(rows.len(), 1);
    let record = &rows[0];
    assert_eq!(record.success, true);
    assert_eq!(record.stream, false);
    assert_eq!(record.input_tokens, Some(10));
    assert_eq!(record.output_tokens, Some(5));
    assert_eq!(record.input_cache_tokens, 4);
    assert_eq!(record.total_tokens, Some(15));
    assert_eq!(record.api_key_name, "itest-key");
    assert_eq!(record.request_time, record.end_time - record.start_time);
    assert!(record.fail_reason.is_none());
}

#[tokio::test]
async fn openai_passthrough_stream_with_usage_injection() {
    let captured = capture();
    let base = spawn_mock(captured.clone()).await;
    let (app, db) = common_setup_with_member(&base, 0, 0, 0).await;

    let (status, text) = send_chat(&app, chat_body("vm-x", true)).await;
    assert_eq!(status, 200, "{text}");
    assert!(text.contains("data: [DONE]"));
    assert!(text.contains("你好"));

    // 上游请求被注入 stream_options.include_usage。
    let upstream_bodies = captured.lock().unwrap();
    assert_eq!(upstream_bodies[0]["stream_options"]["include_usage"], true);

    let rows = wait_for_records(&db, 1).await;
    let record = &rows[0];
    assert_eq!(record.stream, true);
    assert_eq!(record.input_tokens, Some(11));
    assert_eq!(record.output_tokens, Some(3));
    assert!(record.ttft.is_some(), "流式应有 ttft");
}

#[tokio::test]
async fn anthropic_non_stream_converts_and_merges_cache_tokens() {
    let captured = capture();
    let base = spawn_mock(captured.clone()).await;
    let (app, db) = common_setup_with_member(&base, 2, 0, 0).await;

    let (status, text) = send_chat(&app, chat_body("vm-x", false)).await;
    assert_eq!(status, 200, "{text}");
    let body: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(body["object"], "chat.completion");
    assert_eq!(body["choices"][0]["message"]["content"], "你好");
    assert_eq!(body["choices"][0]["finish_reason"], "stop");

    // 上游请求为 Anthropic 形状。
    let upstream_bodies = captured.lock().unwrap();
    assert_eq!(upstream_bodies[0]["max_tokens"], 128);
    assert_eq!(upstream_bodies[0]["messages"][0]["role"], "user");

    let rows = wait_for_records(&db, 1).await;
    let record = &rows[0];
    // input = 10 + cache_read 3 + cache_creation 2 = 15（含缓存总输入）。
    assert_eq!(record.input_tokens, Some(15));
    assert_eq!(record.input_cache_tokens, 5);
    assert_eq!(record.output_tokens, Some(5));
}

#[tokio::test]
async fn anthropic_stream_converts_to_openai_chunks() {
    let base = spawn_mock(capture()).await;
    let (app, db) = common_setup_with_member(&base, 2, 0, 0).await;

    let (status, text) = send_chat(&app, chat_body("vm-x", true)).await;
    assert_eq!(status, 200, "{text}");
    assert!(text.contains("\"role\":\"assistant\""));
    assert!(text.contains("你好"));
    assert!(text.contains("\"finish_reason\":\"stop\""));
    assert!(text.contains("data: [DONE]"));

    let rows = wait_for_records(&db, 1).await;
    let record = &rows[0];
    assert_eq!(record.stream, true);
    assert_eq!(record.input_tokens, Some(15));
    assert!(record.ttft.is_some());
}

#[tokio::test]
async fn responses_forced_stream_aggregates_for_non_stream_client() {
    let captured = capture();
    let base = spawn_mock(captured.clone()).await;
    let (app, db) = common_setup_with_member(&base, 1, 0, 0).await;

    let (status, text) = send_chat(&app, chat_body("vm-x", false)).await;
    assert_eq!(status, 200, "{text}");
    let body: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(body["choices"][0]["message"]["content"], "你好");
    assert_eq!(body["usage"]["prompt_tokens"], 12);

    // 上游被强制 stream: true，且 max_tokens → max_output_tokens。
    let upstream_bodies = captured.lock().unwrap();
    assert_eq!(upstream_bodies[0]["stream"], true);
    assert_eq!(upstream_bodies[0]["max_output_tokens"], 128);
    assert!(upstream_bodies[0].get("max_tokens").is_none());

    let rows = wait_for_records(&db, 1).await;
    let record = &rows[0];
    assert_eq!(record.input_tokens, Some(12));
    assert_eq!(record.input_cache_tokens, 5);
    assert_eq!(record.output_tokens, Some(6));
}

#[tokio::test]
async fn gemini_non_stream_converts() {
    let captured = capture();
    let base = spawn_mock(captured.clone()).await;
    let (app, db) = common_setup_with_member(&base, 3, 0, 0).await;

    let (status, text) = send_chat(&app, chat_body("vm-x", false)).await;
    assert_eq!(status, 200, "{text}");
    let body: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(body["choices"][0]["message"]["content"], "你好");
    assert_eq!(body["choices"][0]["finish_reason"], "stop");

    let upstream_bodies = captured.lock().unwrap();
    assert_eq!(upstream_bodies[0]["generationConfig"]["maxOutputTokens"], 128);
    assert_eq!(upstream_bodies[0]["contents"][0]["role"], "user");

    let rows = wait_for_records(&db, 1).await;
    let record = &rows[0];
    assert_eq!(record.input_tokens, Some(10));
    assert_eq!(record.input_cache_tokens, 6);
    // 输出 = candidates 4 + thoughts 2（含思考）。
    assert_eq!(record.output_tokens, Some(6));
}

#[tokio::test]
async fn failover_retries_next_member_on_429() {
    // 成员 A：返回 429；成员 B：OpenAI 成功。
    let fail_router = Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            (
                HttpStatus::TOO_MANY_REQUESTS,
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
    let fail_base = format!("http://{fail_addr}");

    let ok_base = spawn_mock(capture()).await;

    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    scheduler.start().await.unwrap();
    let app = common::build_authed_app(db.clone(), scheduler, log_tx).await;
    let provider_a = seed_provider(&db, "p-a", &fail_base, 0, 0).await;
    let model_a = seed_provider_model(&db, provider_a, "m-a").await;
    let provider_b = seed_provider(&db, "p-b", &ok_base, 0, 0).await;
    let model_b = seed_provider_model(&db, provider_b, "m-b").await;

    let vm = virtual_model::ActiveModel {
        display_id: Set("vm-fo".to_string()),
        enable: Set(true),
        load_balancing_strategy: Set(3),
        fallback_strategy: Set(1), // RetryEnabledMembers
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    let vm = vm.insert(&db).await.unwrap();
    for model_id in [model_a, model_b] {
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
    }

    let (status, text) = send_chat(&app, chat_body("vm-fo", false)).await;
    assert_eq!(status, 200, "应 failover 到成员 B：{text}");

    let rows = wait_for_records(&db, 1).await;
    let record = &rows[0];
    assert_eq!(record.success, true);
    assert_eq!(record.provider_id, provider_b, "记录最终成功的成员");
    assert_eq!(record.model_id, "m-b");
}

#[tokio::test]
async fn fail_directly_returns_upstream_error_and_records() {
    let fail_router = Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            (
                HttpStatus::TOO_MANY_REQUESTS,
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

    let (app, db) = common_setup_with_member(&base, 0, 0, 0).await;
    // fallback 策略 0（FailDirectly）：把虚拟模型改成 fallback 0。
    let vm = virtual_model::Entity::find()
        .filter(virtual_model::Column::DisplayId.eq("vm-x"))
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    let mut active: virtual_model::ActiveModel = vm.into();
    active.fallback_strategy = Set(0);
    active.update(&db).await.unwrap();

    let (status, text) = send_chat(&app, chat_body("vm-x", false)).await;
    assert_eq!(status, 429);
    let body: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(body["error"]["message"], "rate limited");
    assert_eq!(body["error"]["type"], "invalid_request_error");

    let rows = wait_for_records(&db, 1).await;
    let record = &rows[0];
    assert_eq!(record.success, false);
    assert!(record.fail_reason.as_deref().unwrap_or("").contains("rate limited"));
}

#[tokio::test]
async fn unknown_model_is_404_without_record() {
    let base = spawn_mock(capture()).await;
    let (app, db) = common_setup_with_member(&base, 0, 0, 0).await;

    let (status, text) = send_chat(&app, chat_body("no-such-model", false)).await;
    assert_eq!(status, 404);
    let body: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(body["error"]["code"], "model_not_found");

    tokio::time::sleep(Duration::from_millis(300)).await;
    let rows = request::Entity::find().all(&db).await.unwrap();
    assert!(rows.is_empty(), "路由未命中不落表");
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

#[tokio::test]
async fn subscription_first_ranks_by_remaining_five_hour_usage() {
    use llm_gateway::usage::persist::write_usage_cache;
    use llm_gateway::usage::types::{UsageData, UsageKind, WindowKind};

    let captured_a = capture();
    let captured_b = capture();
    let base_a = spawn_mock(captured_a.clone()).await;
    let base_b = spawn_mock(captured_b.clone()).await;

    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    let provider_a = seed_provider(&db, "订阅-A-高5h余量", &base_a, 0, 1).await;
    let provider_b = seed_provider(&db, "订阅-B-低5h余量", &base_b, 0, 1).await;
    let model_a = seed_provider_model(&db, provider_a, "m-a").await;
    let model_b = seed_provider_model(&db, provider_b, "m-b").await;

    // 预置 10 分钟内的用量数据库缓存：A 的 5h 剩余（80%）高于 B（20%）。
    for (pid, remaining) in [(provider_a, 80.0), (provider_b, 20.0)] {
        let data = UsageData {
            provider_id: pid,
            fetched_at: chrono::Utc::now(),
            kind: UsageKind::Quota,
            plan: None,
            windows: vec![
                llm_gateway::usage::types::QuotaWindow::from_remaining_percent(
                    WindowKind::FiveHour,
                    remaining,
                    None,
                ),
                llm_gateway::usage::types::QuotaWindow::from_remaining_percent(
                    WindowKind::Weekly,
                    50.0,
                    None,
                ),
                llm_gateway::usage::types::QuotaWindow::unavailable(WindowKind::Monthly),
            ],
            balances: vec![],
        };
        write_usage_cache(&db, &data).await.unwrap();
    }

    scheduler.start().await.unwrap();
    let app = common::build_authed_app(db.clone(), scheduler, log_tx).await;

    let vm = virtual_model::ActiveModel {
        display_id: Set("vm-lb-q".to_string()),
        enable: Set(true),
        load_balancing_strategy: Set(0),
        fallback_strategy: Set(1),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    let vm = vm.insert(&db).await.unwrap();
    for model_id in [model_a, model_b] {
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
    }

    let (status, text) = send_chat(
        &app,
        json!({"model": "vm-lb-q", "messages": [{"role": "user", "content": "hi"}]}),
    )
    .await;
    assert_eq!(status, 200, "转发失败：{text}");
    assert_eq!(
        captured_a.lock().unwrap().len(),
        1,
        "订阅制优先应选择 5h 剩余更高的供应商 A"
    );
    assert_eq!(captured_b.lock().unwrap().len(), 0, "供应商 B 不应被选到");
}

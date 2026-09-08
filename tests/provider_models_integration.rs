mod common;

use axum::body::Body;
use axum::http::Request;
use axum::response::IntoResponse;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, Set};
use serde_json::{Value, json};
use tower::ServiceExt;

use llm_gateway::crypto;
use llm_gateway::entity::provider;
use llm_gateway::entity::provider_model;
use llm_gateway::entity::virtual_model_item;

#[path = "provider_models_integration/crud.rs"]
mod crud;
#[path = "provider_models_integration/forward.rs"]
mod forward;
#[path = "provider_models_integration/network_proxy.rs"]
mod network_proxy;
#[path = "provider_models_integration/protocol_override.rs"]
mod protocol_override;

/// 建一个测试 Provider（api_key 加密存储），返回其 id。
async fn seed_provider(db: &sea_orm::DatabaseConnection, name: &str) -> i32 {
    let active = provider::ActiveModel {
        name: Set(name.to_string()),
        enable: Set(true),
        base_url: Set("https://api.example.com/v1".to_string()),
        api_key: Set(crypto::encrypt("sk-test")),
        custom_header: Set("{}".to_string()),
        protocol_type: Set(0),
        billing_mode: Set(0),
        extra: Set("{}".to_string()),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    active.insert(db).await.unwrap().id
}

async fn setup_app() -> (axum::Router, sea_orm::DatabaseConnection) {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    scheduler.start().await.unwrap();
    let app = common::build_authed_app(db.clone(), scheduler, log_tx).await;
    (app, db)
}

async fn send_json(app: axum::Router, method: &str, uri: &str, body: Value) -> (u16, Value) {
    let request: Request<Body> = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
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

fn model_payload(id: &str) -> Value {
    json!({
        "providerModelId": id,
        "contextLength": 128000,
        "maxOutputTokens": 4096,
        "reasoning": true,
        "toolUse": true,
        "imageUnderstand": false,
        "videoUnderstand": false,
    })
}

// ─── 模型列表刷新走 provider 网络代理 ──────────────────────────────────────────
// 场景：provider 开启网络代理（proxyEnabled + proxyAddr）时，「刷新模型」请求
// 应经 CONNECT 代理转发到供应商 Models 接口，而不是直连。

/// CONNECT/正向代理 mock：收到 CONNECT（隧道）或 `METHOD http://host/path`
/// （http 正向代理，reqwest 对 http 目标走此形式）都转发到目标；统计请求次数。
async fn spawn_connect_proxy() -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let connect_count = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&connect_count);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (mut client, _) = match listener.accept().await {
                Ok(accepted) => accepted,
                Err(_) => break,
            };
            count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            tokio::spawn(async move {
                let mut buf = [0u8; 8192];
                let mut len = 0usize;
                loop {
                    let Ok(n) = client.read(&mut buf[len..]).await else {
                        return;
                    };
                    if n == 0 {
                        return;
                    }
                    len += n;
                    if buf[..len].windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                let head = String::from_utf8_lossy(&buf[..len]);
                let Some(first_line) = head.lines().next() else {
                    return;
                };
                // CONNECT host:port → 隧道。
                if let Some(target) = first_line
                    .strip_prefix("CONNECT ")
                    .and_then(|l| l.split_whitespace().next())
                {
                    let Ok(mut target_stream) = tokio::net::TcpStream::connect(target).await else {
                        return;
                    };
                    let _ = client
                        .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                        .await;
                    let (mut cr, mut cw) = client.split();
                    let (mut tr, mut tw) = target_stream.split();
                    let _ = tokio::join!(
                        tokio::io::copy(&mut cr, &mut tw),
                        tokio::io::copy(&mut tr, &mut cw)
                    );
                    return;
                }
                // 正向代理：`METHOD http://host/path HTTP/1.1` → 转发。
                let Some((method, rest)) = first_line.split_once(' ') else {
                    return;
                };
                let Some((abs_url, version)) = rest.rsplit_once(' ') else {
                    return;
                };
                let Some(parsed) = abs_url.strip_prefix("http://") else {
                    return;
                };
                let Some((host, path)) = parsed.split_once('/') else {
                    return;
                };
                let Ok(mut target_stream) = tokio::net::TcpStream::connect(host).await else {
                    return;
                };
                let rewritten = format!("{method} /{path} {version}\r\n");
                let tail = head.split_once("\r\n").map(|(_, t)| t).unwrap_or("");
                let mut headers = String::new();
                let mut has_host = false;
                for line in tail.lines() {
                    if line.to_ascii_lowercase().starts_with("host:") {
                        has_host = true;
                    }
                    headers.push_str(line);
                    headers.push_str("\r\n");
                }
                let _ = target_stream.write_all(rewritten.as_bytes()).await;
                if !has_host {
                    let _ = target_stream
                        .write_all(format!("Host: {host}\r\n").as_bytes())
                        .await;
                }
                let _ = target_stream.write_all(headers.as_bytes()).await;
                let _ = target_stream.write_all(b"\r\n").await;
                let (mut cr, mut cw) = client.split();
                let (mut tr, mut tw) = target_stream.split();
                let _ = tokio::join!(
                    tokio::io::copy(&mut cr, &mut tw),
                    tokio::io::copy(&mut tr, &mut cw)
                );
            });
        }
    });
    (format!("http://{addr}"), connect_count)
}

/// 目标 mock：返回 OpenAI 风格 models 列表。
async fn spawn_models_mock() -> String {
    let app = axum::Router::new().route(
        "/v1/models",
        axum::routing::get(|| async {
            axum::Json(json!({
                "object": "list",
                "data": [
                    { "id": "gpt-4o", "object": "model" },
                    { "id": "gpt-4o-mini", "object": "model" },
                ]
            }))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

/// 目标 mock：返回自定义模型 ID 列表（待确认相似度匹配用例）。
async fn spawn_models_mock_with_ids(ids: &[&str]) -> String {
    let data: Vec<Value> = ids
        .iter()
        .map(|id| json!({ "id": id, "object": "model" }))
        .collect();
    let app = axum::Router::new().route(
        "/v1/models",
        axum::routing::get(move || {
            let data = data.clone();
            async move { axum::Json(json!({ "object": "list", "data": data })) }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

/// 把 seed 供应商的 base_url 指向本地 mock（seed 默认 api.example.com 不可达）。
async fn point_provider_at(db: &sea_orm::DatabaseConnection, provider_id: i32, target: &str) {
    let row = provider::Entity::find_by_id(provider_id)
        .one(db)
        .await
        .unwrap()
        .unwrap();
    let mut active: provider::ActiveModel = row.into();
    active.base_url = Set(format!("{target}/v1"));
    active.update(db).await.unwrap();
}

// ─── 模型级代理转发优先级（test_model 路径）───────────────────────────────────
// 场景：向模型上游发测试请求时，代理解析 = 模型级 → 供应商级 → 直连。
// 用两个不同端口的 CONNECT 代理 mock 区分命中：请求走哪个代理，哪个计数器 +1。

/// OpenAI 兼容 chat/completions 目标 mock（非流式 200）。
async fn spawn_chat_mock() -> String {
    let app = axum::Router::new().route(
        "/v1/chat/completions",
        axum::routing::post(|| async {
            axum::Json(json!({
                "id": "chatcmpl-test",
                "object": "chat.completion",
                "created": 0,
                "model": "m",
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "ok"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2},
            }))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

/// OpenAI Responses 目标 mock（非流式 SSE 200）——用于测速走 Responses 协议时判定。
async fn spawn_responses_mock() -> String {
    let app = axum::Router::new().route(
        "/v1/responses",
        axum::routing::post(|| async {
            let payload = "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"model\":\"m\"}}\n\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1}}}\n\ndata: [DONE]\n\n";
            (
                axum::http::StatusCode::OK,
                [("content-type", "text/event-stream")],
                payload,
            )
                .into_response()
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

/// Anthropic Messages 目标 mock（非流式 200）——用于测速回落供应商协议时判定。
async fn spawn_messages_mock() -> String {
    let app = axum::Router::new().route(
        "/v1/messages",
        axum::routing::post(|| async {
            axum::Json(json!({
                "id": "msg_test",
                "type": "message",
                "role": "assistant",
                "content": [{"type": "text", "text": "ok"}],
                "stop_reason": "end_turn",
                "usage": {"input_tokens": 1, "output_tokens": 1},
            }))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

/// 直插一个 provider + provider_model（模型级代理两字段可自定义）。
async fn seed_provider_and_model(
    db: &sea_orm::DatabaseConnection,
    name: &str,
    target: &str,
    provider_proxy: Option<String>,
    model_proxy: Option<String>,
) -> (i32, i32) {
    let active = provider::ActiveModel {
        name: Set(name.to_string()),
        enable: Set(true),
        base_url: Set(format!("{target}/v1")),
        api_key: Set(crypto::encrypt("sk-test")),
        custom_header: Set("{}".to_string()),
        protocol_type: Set(0),
        billing_mode: Set(0),
        extra: Set("{}".to_string()),
        proxy_enabled: Set(provider_proxy.is_some()),
        proxy_addr: Set(provider_proxy.unwrap_or_default()),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    let provider_id = active.insert(db).await.unwrap().id;

    let now = chrono::Utc::now();
    let model = provider_model::ActiveModel {
        provider_id: Set(provider_id),
        provider_model_id: Set("proxy-model".to_string()),
        context_length: Set(128_000),
        max_output_tokens: Set(4_096),
        proxy_enabled: Set(model_proxy.is_some()),
        proxy_addr: Set(model_proxy.unwrap_or_default()),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };
    let model_id = model.insert(db).await.unwrap().model_id;
    (provider_id, model_id)
}

/// 直插 provider + provider_model（供应商/模型协议可指定，模型 None=跟随供应商）。
async fn seed_provider_and_model_with_protocol(
    db: &sea_orm::DatabaseConnection,
    name: &str,
    target: &str,
    provider_protocol: i32,
    model_protocol: Option<i32>,
) -> (i32, i32) {
    let active = provider::ActiveModel {
        name: Set(name.to_string()),
        enable: Set(true),
        base_url: Set(format!("{target}/v1")),
        api_key: Set(crypto::encrypt("sk-test")),
        custom_header: Set("{}".to_string()),
        protocol_type: Set(provider_protocol),
        billing_mode: Set(0),
        extra: Set("{}".to_string()),
        proxy_enabled: Set(false),
        proxy_addr: Set("".to_string()),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    let provider_id = active.insert(db).await.unwrap().id;

    let now = chrono::Utc::now();
    let model = provider_model::ActiveModel {
        provider_id: Set(provider_id),
        provider_model_id: Set("protocol-model".to_string()),
        context_length: Set(128_000),
        max_output_tokens: Set(4_096),
        protocol_type: Set(model_protocol),
        proxy_enabled: Set(false),
        proxy_addr: Set("".to_string()),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };
    let model_id = model.insert(db).await.unwrap().model_id;
    (provider_id, model_id)
}

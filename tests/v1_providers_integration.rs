//! 对外只读接口（GET /v1/providers、GET /v1/providers/{id}/usage）集成测试。
//!
//! 认证走 /v1 Bearer API Key 中间件（不涉 Cookie）；用量行为与
//! /api/providers/{id}/usage 共享同一核心，成功路径经
//! `LLM_GATEWAY_USAGE_HTTP_OVERRIDE` 重定向到本地 mock（DeepSeek 余额形态）。
mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Json;
use serde_json::Value;
use tower::ServiceExt;

const OVERRIDE_ENV: &str = "LLM_GATEWAY_USAGE_HTTP_OVERRIDE";

async fn setup_app() -> axum::Router {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    common::build_authed_app(db, scheduler, log_tx).await
}

/// 未经认证包装的 app：验证 /v1 中间件对缺失/错误 Bearer 的 401 行为。
async fn setup_unauthed_app() -> axum::Router {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    common::build_app(db, scheduler, log_tx)
}

async fn send(
    app: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<&str>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    let request = builder
        .body(Body::from(body.unwrap_or_default().to_string()))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

async fn create_provider(app: &axum::Router, name: &str, base_url: &str, extra: &str) -> i64 {
    let body = serde_json::json!({
        "name": name,
        "enable": true,
        "baseUrl": base_url,
        "apiKey": "sk-v1-usage-test",
        "protocolType": 0,
        "billingMode": 0,
        "customHeader": "{}",
        "extra": extra,
    })
    .to_string();
    let (status, body) = send(app, "POST", "/api/providers", Some(&body)).await;
    assert_eq!(status, StatusCode::CREATED, "创建失败：{body}");
    body["data"]["id"].as_i64().unwrap()
}

/// 本地 mock：任意路径返回固定 DeepSeek 余额响应，并计数请求次数。
async fn spawn_mock() -> (String, Arc<AtomicUsize>) {
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = counter.clone();
    let app = axum::Router::new().fallback(move || {
        let counter = counter_clone.clone();
        async move {
            counter.fetch_add(1, Ordering::SeqCst);
            Json(serde_json::json!({
                "is_available": true,
                "balance_infos": [
                    { "currency": "CNY", "total_balance": "110.00", "granted_balance": "10.00", "topped_up_balance": "100.00" }
                ]
            }))
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{addr}"), counter)
}

#[tokio::test]
async fn v1_providers_require_bearer_api_key() {
    let app = setup_unauthed_app().await;

    // 无 Authorization 头 → 401（OpenAI 风格错误信封）。
    let (status, body) = send(&app, "GET", "/v1/providers", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "INVALID_API_KEY");

    // 错误的 Bearer → 401。
    let request = Request::builder()
        .method("GET")
        .uri("/v1/providers")
        .header("authorization", "Bearer lg-wrong-key")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // 用量接口同样被拦截。
    let (status, _) = send(&app, "GET", "/v1/providers/1/usage", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn v1_providers_list_minimal_fields() {
    let app = setup_app().await;
    let id_usage = create_provider(
        &app,
        "DS-有用量",
        "https://api.deepseek.com",
        r#"{"usage": true, "usage_type": 0}"#,
    )
    .await;
    let id_plain = create_provider(&app, "DS-无用量", "https://api.deepseek.com", "{}").await;

    let (status, body) = send(&app, "GET", "/v1/providers", None).await;
    assert_eq!(status, StatusCode::OK, "查询失败：{body}");
    assert_eq!(body["code"], "0");
    let items = body["data"].as_array().unwrap();
    assert_eq!(items.len(), 2);

    let first = &items[0];
    assert_eq!(first["id"], id_usage);
    assert_eq!(first["name"], "DS-有用量");
    assert_eq!(first["enable"], true);
    assert_eq!(first["billingMode"], 0);
    assert_eq!(first["usageSupported"], true);
    // 敏感字段不外泄。
    for key in [
        "apiKey",
        "apiKeyMasked",
        "baseUrl",
        "customHeader",
        "extra",
        "proxyAddr",
    ] {
        assert!(first.get(key).is_none(), "字段 {key} 不应出现在对外响应中");
    }

    assert_eq!(items[1]["id"], id_plain);
    assert_eq!(items[1]["usageSupported"], false);
}

#[tokio::test]
async fn v1_provider_usage_not_found() {
    let app = setup_app().await;
    let (status, body) = send(&app, "GET", "/v1/providers/999/usage", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "NOT_FOUND");
}

#[tokio::test]
async fn v1_provider_usage_not_enabled() {
    let app = setup_app().await;
    let id = create_provider(&app, "DS-未开启", "https://api.deepseek.com", "{}").await;
    let (status, body) = send(&app, "GET", &format!("/v1/providers/{id}/usage"), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["msg"].as_str().unwrap().contains("未开启用量查询"));
}

#[tokio::test]
async fn v1_provider_usage_cache_and_refresh() {
    let (mock_base, counter) = spawn_mock().await;
    temp_env::async_with_vars([(OVERRIDE_ENV, Some(mock_base.as_str()))], async {
        let app = setup_app().await;
        let id = create_provider(
            &app,
            "DeepSeek-v1",
            "https://api.deepseek.com",
            r#"{"usage": true, "usage_type": 0}"#,
        )
        .await;

        let (status, body) = send(&app, "GET", &format!("/v1/providers/{id}/usage"), None).await;
        assert_eq!(status, StatusCode::OK, "查询失败：{body}");
        let data = &body["data"];
        assert_eq!(data["kind"], "balance");
        assert_eq!(data["providerId"], id);
        assert_eq!(data["balances"][0]["amount"], 110.0);
        assert!(data["timezone"].as_str().is_some());
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        // 缓存内第二次请求不打上游。
        let (status, _) = send(&app, "GET", &format!("/v1/providers/{id}/usage"), None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        // ?refresh=1 绕过缓存。
        let (status, _) = send(
            &app,
            "GET",
            &format!("/v1/providers/{id}/usage?refresh=1"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    })
    .await;
}

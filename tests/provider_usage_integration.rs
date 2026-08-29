//! Provider 用量查询接口（GET /api/providers/{id}/usage）集成测试。
//!
//! 成功路径通过环境变量 `LLM_GATEWAY_USAGE_HTTP_OVERRIDE` 将用量请求重定向到
//! 本地 mock（DeepSeek 余额形态），并验证 60s 缓存与 ?refresh=1 绕过行为。

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

fn create_body(name: &str, base_url: &str, extra: &str) -> String {
    serde_json::json!({
        "name": name,
        "enable": true,
        "baseUrl": base_url,
        "apiKey": "sk-usage-test",
        "protocolType": 0,
        "billingMode": 0,
        "customHeader": "{}",
        "extra": extra,
    })
    .to_string()
}

async fn create_provider(app: &axum::Router, name: &str, base_url: &str, extra: &str) -> i64 {
    let (status, body) = send(
        app,
        "POST",
        "/api/providers",
        Some(&create_body(name, base_url, extra)),
    )
    .await;
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
async fn usage_not_found_provider() {
    let app = setup_app().await;
    let (status, body) = send(&app, "GET", "/api/providers/999/usage", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "NOT_FOUND");
}

#[tokio::test]
async fn usage_not_enabled() {
    let app = setup_app().await;
    let id = create_provider(&app, "DS-无用量", "https://api.deepseek.com", "{}").await;
    let (status, body) = send(&app, "GET", &format!("/api/providers/{id}/usage"), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["msg"].as_str().unwrap().contains("未开启用量查询"));
}

#[tokio::test]
async fn usage_unsupported_host() {
    let app = setup_app().await;
    // 手动开启 usage 但 host 不在支持列表（自定义网关）。
    let id = create_provider(
        &app,
        "自定义网关",
        "https://gateway.example.com/v1",
        r#"{"usage": true, "usage_type": 1}"#,
    )
    .await;
    let (status, body) = send(&app, "GET", &format!("/api/providers/{id}/usage"), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["msg"].as_str().unwrap().contains("暂不支持"));
}

#[tokio::test]
async fn usage_success_cache_and_refresh() {
    let (mock_base, counter) = spawn_mock().await;
    temp_env::async_with_vars([(OVERRIDE_ENV, Some(mock_base.as_str()))], async {
        let app = setup_app().await;
        let id = create_provider(
            &app,
            "DeepSeek-用量",
            "https://api.deepseek.com",
            r#"{"usage": true, "usage_type": 0}"#,
        )
        .await;

        let (status, body) = send(&app, "GET", &format!("/api/providers/{id}/usage"), None).await;
        assert_eq!(status, StatusCode::OK, "查询失败：{body}");
        let data = &body["data"];
        assert_eq!(data["kind"], "balance");
        assert_eq!(data["providerId"], id);
        assert!(data["fetchedAt"].as_str().is_some());
        assert_eq!(data["balances"][0]["label"], "余额（CNY）");
        assert_eq!(data["balances"][0]["amount"], 110.0);
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        // 60s 缓存内第二次请求不打上游。
        let (status, _) = send(&app, "GET", &format!("/api/providers/{id}/usage"), None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        // ?refresh=1 绕过缓存。
        let (status, _) = send(
            &app,
            "GET",
            &format!("/api/providers/{id}/usage?refresh=1"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(counter.load(Ordering::SeqCst), 2);

        // 更新 provider 后缓存失效，下次查询重新打上游。
        let (status, _) = send(
            &app,
            "PUT",
            &format!("/api/providers/{id}"),
            Some(r#"{"name": "DeepSeek-用量改"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = send(&app, "GET", &format!("/api/providers/{id}/usage"), None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    })
    .await;
}

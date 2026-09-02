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
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, Set};
use serde_json::Value;
use tower::ServiceExt;

use llm_gateway::entity::{provider, usage_cache};

const OVERRIDE_ENV: &str = "LLM_GATEWAY_USAGE_HTTP_OVERRIDE";

async fn setup_app() -> axum::Router {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    common::build_authed_app(db, scheduler, log_tx).await
}

async fn setup_app_with_db() -> (axum::Router, sea_orm::DatabaseConnection) {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    let app = common::build_authed_app(db.clone(), scheduler, log_tx).await;
    (app, db)
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

#[tokio::test]
async fn usage_db_cache_stale_after_10min_refetches() {
    let (mock_base, counter) = spawn_mock().await;
    temp_env::async_with_vars([(OVERRIDE_ENV, Some(mock_base.as_str()))], async {
        let (app, db) = setup_app_with_db().await;
        let id = create_provider(
            &app,
            "DeepSeek-过期",
            "https://api.deepseek.com",
            r#"{"usage": true, "usage_type": 0}"#,
        )
        .await;

        let (status, body) = send(&app, "GET", &format!("/api/providers/{id}/usage"), None).await;
        assert_eq!(status, StatusCode::OK, "查询失败：{body}");
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        // 把缓存行回拨到 11 分钟前 → 视为过期，下次请求需要重新抓取。
        let row = usage_cache::Entity::find()
            .filter(usage_cache::Column::ProviderId.eq(id))
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        let mut active: usage_cache::ActiveModel = row.into();
        active.fetched_at = Set(chrono::Utc::now() - chrono::Duration::minutes(11));
        active.update(&db).await.unwrap();

        let (status, _) = send(&app, "GET", &format!("/api/providers/{id}/usage"), None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(counter.load(Ordering::SeqCst), 2);

        // 重取后落库刷新 → 10 分钟内再次请求直接打缓存。
        let (status, _) = send(&app, "GET", &format!("/api/providers/{id}/usage"), None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    })
    .await;
}

#[tokio::test]
async fn refresh_all_usage_only_writes_usage_enabled_providers() {
    let (mock_base, counter) = spawn_mock().await;
    temp_env::async_with_vars([(OVERRIDE_ENV, Some(mock_base.as_str()))], async {
        let (db, _scheduler, _log_tx) = common::setup_db_and_scheduler().await;
        let now = chrono::Utc::now();
        // 已禁用的用量供应商也要被监测（停用后恢复依赖持续刷新）。
        let usage_provider = provider::ActiveModel {
            name: Set("DS-已禁用".to_string()),
            enable: Set(false),
            base_url: Set("https://api.deepseek.com".to_string()),
            api_key: Set(llm_gateway::crypto::encrypt("sk-x")),
            custom_header: Set("{}".to_string()),
            status: Set(0),
            protocol_type: Set(0),
            billing_mode: Set(0),
            extra: Set(r#"{"usage": true, "usage_type": 0}"#.to_string()),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap()
        .id;
        provider::ActiveModel {
            name: Set("DS-无用量".to_string()),
            enable: Set(true),
            base_url: Set("https://api.deepseek.com".to_string()),
            api_key: Set(llm_gateway::crypto::encrypt("sk-x")),
            custom_header: Set("{}".to_string()),
            status: Set(0),
            protocol_type: Set(0),
            billing_mode: Set(0),
            extra: Set("{}".to_string()),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let ok = llm_gateway::usage::persist::refresh_all_usage(&db)
            .await
            .unwrap();
        assert_eq!(ok, 1);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        assert!(
            usage_cache::Entity::find()
                .filter(usage_cache::Column::ProviderId.eq(usage_provider))
                .one(&db)
                .await
                .unwrap()
                .is_some()
        );
        assert_eq!(usage_cache::Entity::find().count(&db).await.unwrap(), 1);
    })
    .await;
}

// ── SenseNova：OAuth 续期 + refresh_token 轮换写回 + pool-usage ──

const SENSENOVA_POOL_USAGE_BODY: &str = r#"{
  "plan": { "id": "free", "name": "Free Plan", "type": "TOKEN_PLAN_PLAN_TYPE_FREE" },
  "pools": [
    { "id": "pool_a", "name": "通用积分池", "pool_type": "default",
      "window_5h": { "limit": "60000", "used": "33586.30032", "remaining": "26413.69968", "reset_at": "1788365437" },
      "window_7d": { "limit": "600000", "used": "51388.65712", "remaining": "548611.34288", "reset_at": "1788862237" } },
    { "id": "pool_b", "name": "Flash-Lite积分池", "pool_type": "dedicated",
      "window_5h": { "limit": "10000", "used": "9999", "remaining": "1", "reset_at": "1788365437" } }
  ]
}"#;

#[derive(Clone, Default)]
struct SensenovaMockState {
    renewal_hits: Arc<AtomicUsize>,
    usage_hits: Arc<AtomicUsize>,
    last_renewal_form: Arc<std::sync::Mutex<Option<String>>>,
    last_auth: Arc<std::sync::Mutex<Option<String>>>,
}

/// mock：POST /oauth2/token 返回固定续期响应（轮换出 rt-new），
/// GET pool-usage 返回双积分池响应，并记录收到的表单与 Authorization。
async fn spawn_sensenova_mock() -> (String, SensenovaMockState) {
    spawn_sensenova_mock_with_renewal(serde_json::json!({
        "access_token": "at-1",
        "expires_in": 10799,
        "refresh_token": "rt-new",
        "token_type": "bearer"
    }))
    .await
}

/// 同上，但续期响应体可自定义（如 invalid_grant 失败场景）。
async fn spawn_sensenova_mock_with_renewal(renewal: Value) -> (String, SensenovaMockState) {
    use axum::http::HeaderMap;
    use axum::routing::{get, post};

    let state = SensenovaMockState::default();
    let app = {
        let renewal_state = state.clone();
        let usage_state = state.clone();
        axum::Router::new()
            .route(
                "/oauth2/token",
                post(move |body: String| {
                    let state = renewal_state.clone();
                    let renewal = renewal.clone();
                    async move {
                        state.renewal_hits.fetch_add(1, Ordering::SeqCst);
                        *state.last_renewal_form.lock().unwrap() = Some(body);
                        Json(renewal)
                    }
                }),
            )
            .route(
                "/lite/console/v1/tokenplan/pool-usage",
                get(move |headers: HeaderMap| {
                    let state = usage_state.clone();
                    async move {
                        state.usage_hits.fetch_add(1, Ordering::SeqCst);
                        *state.last_auth.lock().unwrap() = headers
                            .get("authorization")
                            .and_then(|v| v.to_str().ok())
                            .map(str::to_string);
                        Json(serde_json::from_str::<Value>(SENSENOVA_POOL_USAGE_BODY).unwrap())
                    }
                }),
            )
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{addr}"), state)
}

#[tokio::test]
async fn sensenova_usage_full_chain_with_rotation_writeback() {
    let (mock_base, mock) = spawn_sensenova_mock().await;
    temp_env::async_with_vars([(OVERRIDE_ENV, Some(mock_base.as_str()))], async {
        let (app, db) = setup_app_with_db().await;
        let id = create_provider(
            &app,
            "SenseNova-订阅",
            "https://token.sensenova.cn/v1",
            r#"{"usage": true, "usage_type": 0, "refresh_token": "rt-1"}"#,
        )
        .await;

        let (status, body) = send(&app, "GET", &format!("/api/providers/{id}/usage"), None).await;
        assert_eq!(status, StatusCode::OK, "查询失败：{body}");
        let data = &body["data"];
        assert_eq!(data["kind"], "quota");
        assert_eq!(data["plan"], "Free Plan");
        // 每池独立窗口，label = 池名。
        let windows = data["windows"].as_array().unwrap();
        assert_eq!(windows.len(), 3);
        assert_eq!(windows[0]["window"], "five_hour");
        assert_eq!(windows[0]["label"], "通用积分池");
        assert_eq!(windows[0]["used"], 33586.3);
        assert_eq!(windows[0]["limit"], 60000.0);
        assert_eq!(windows[0]["unit"], "积分");
        assert_eq!(windows[1]["window"], "weekly");
        assert_eq!(windows[1]["label"], "通用积分池");
        assert_eq!(windows[2]["label"], "Flash-Lite积分池");
        assert_eq!(windows[2]["remainingPercent"], 0.01);
        // pool-usage 用续期得到的 access_token 调用。
        assert_eq!(
            mock.last_auth.lock().unwrap().as_deref(),
            Some("Bearer at-1")
        );

        // 轮换出的新 refresh_token 已写回 provider extra。
        let model = provider::Entity::find_by_id(id as i32)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        let extra: Value = serde_json::from_str(&model.extra).unwrap();
        assert_eq!(extra["refresh_token"], "rt-new");

        // 绕过缓存再查一次：续期用的是写回后的 rt-new（凭据链不断）。
        let (status, _) = send(
            &app,
            "GET",
            &format!("/api/providers/{id}/usage?refresh=1"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            mock.last_renewal_form
                .lock()
                .unwrap()
                .as_deref()
                .unwrap()
                .contains("refresh_token=rt-new")
        );
    })
    .await;
}

#[tokio::test]
async fn sensenova_platform_host_dispatch_and_missing_credential() {
    let (mock_base, _mock) = spawn_sensenova_mock().await;
    temp_env::async_with_vars([(OVERRIDE_ENV, Some(mock_base.as_str()))], async {
        let app = setup_app().await;
        // 控制台域名同样分发到 SenseNova fetcher。
        let id = create_provider(
            &app,
            "SenseNova-控制台域",
            "https://platform.sensenova.cn/v1",
            r#"{"usage": true, "usage_type": 0}"#,
        )
        .await;
        let (status, body) = send(&app, "GET", &format!("/api/providers/{id}/usage"), None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body["msg"].as_str().unwrap().contains("refresh_token"));
    })
    .await;
}

#[tokio::test]
async fn sensenova_invalid_grant_maps_to_auth_error() {
    // 续期端点返回 200 + error 字段（refresh_token 失效）→ 走鉴权失败链路。
    let (mock_base, _mock) = spawn_sensenova_mock_with_renewal(serde_json::json!({
        "error": "invalid_grant",
        "error_description": "The refresh token is invalid"
    }))
    .await;
    temp_env::async_with_vars([(OVERRIDE_ENV, Some(mock_base.as_str()))], async {
        let app = setup_app().await;
        let id = create_provider(
            &app,
            "SenseNova-失效",
            "https://token.sensenova.cn/v1",
            r#"{"usage": true, "usage_type": 0, "refresh_token": "rt-dead"}"#,
        )
        .await;
        let (status, body) = send(&app, "GET", &format!("/api/providers/{id}/usage"), None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["msg"], "用量查询凭据无效或已过期");
    })
    .await;
}

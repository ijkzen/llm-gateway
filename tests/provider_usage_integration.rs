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

fn create_body_with_billing(name: &str, base_url: &str, extra: &str, billing_mode: i32) -> String {
    serde_json::json!({
        "name": name,
        "enable": true,
        "baseUrl": base_url,
        "apiKey": "sk-usage-test",
        "protocolType": 0,
        "billingMode": billing_mode,
        "customHeader": "{}",
        "extra": extra,
    })
    .to_string()
}

async fn create_provider(app: &axum::Router, name: &str, base_url: &str, extra: &str) -> i64 {
    create_provider_with_billing(app, name, base_url, extra, 0).await
}

async fn create_provider_with_billing(
    app: &axum::Router,
    name: &str,
    base_url: &str,
    extra: &str,
    billing_mode: i32,
) -> i64 {
    let body = create_body_with_billing(name, base_url, extra, billing_mode);
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

#[derive(Clone)]
struct KrillMockState {
    subscription_replies: Arc<std::sync::Mutex<std::collections::VecDeque<(StatusCode, Value)>>>,
    subscription_hits: Arc<AtomicUsize>,
    login_hits: Arc<AtomicUsize>,
    auth_headers: Arc<std::sync::Mutex<Vec<Option<String>>>>,
}

async fn spawn_krill_mock(
    subscription_replies: Vec<(StatusCode, Value)>,
    login_reply: (StatusCode, Value),
) -> (String, KrillMockState) {
    use axum::http::HeaderMap;
    use axum::response::IntoResponse;
    use axum::routing::{get, post};

    let state = KrillMockState {
        subscription_replies: Arc::new(std::sync::Mutex::new(subscription_replies.into())),
        subscription_hits: Arc::new(AtomicUsize::new(0)),
        login_hits: Arc::new(AtomicUsize::new(0)),
        auth_headers: Arc::new(std::sync::Mutex::new(Vec::new())),
    };
    let subscription_state = state.clone();
    let login_state = state.clone();
    let app = axum::Router::new()
        .route(
            "/api/subscription",
            get(move |headers: HeaderMap| {
                let state = subscription_state.clone();
                async move {
                    state.subscription_hits.fetch_add(1, Ordering::SeqCst);
                    state.auth_headers.lock().unwrap().push(
                        headers
                            .get("authorization")
                            .and_then(|value| value.to_str().ok())
                            .map(str::to_string),
                    );
                    let (status, body) = state
                        .subscription_replies
                        .lock()
                        .unwrap()
                        .pop_front()
                        .expect("unexpected subscription request");
                    (status, Json(body)).into_response()
                }
            }),
        )
        .route(
            "/api/auth/login",
            post(move || {
                let state = login_state.clone();
                let reply = login_reply.clone();
                async move {
                    state.login_hits.fetch_add(1, Ordering::SeqCst);
                    (reply.0, Json(reply.1)).into_response()
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{addr}"), state)
}

fn krill_balance_reply() -> Value {
    serde_json::json!({
        "success": true,
        "code": 0,
        "data": {
            "subscriptions": [],
            "summary": {},
            "credit_balance_usd": "24.5",
            "welfare_balance_usd": "0.5",
            "request_count_quota": null
        }
    })
}

fn krill_login_reply() -> (StatusCode, Value) {
    (
        StatusCode::OK,
        serde_json::json!({
            "success": true,
            "code": 0,
            "data": { "token": "jwt-new", "user": {} }
        }),
    )
}

#[tokio::test]
async fn krill_valid_jwt_queries_balance_without_login() {
    let (mock_base, mock) = spawn_krill_mock(
        vec![(StatusCode::OK, krill_balance_reply())],
        krill_login_reply(),
    )
    .await;
    temp_env::async_with_vars([(OVERRIDE_ENV, Some(mock_base.as_str()))], async {
        let app = setup_app().await;
        let id = create_provider(
            &app,
            "Krill-按量",
            "https://api-slb.krill-ai.net/v1",
            r#"{"usage":true,"usage_type":0,"email":"u@example.com","password":"pw","jwt":"jwt-old"}"#,
        )
        .await;

        let (status, body) = send(&app, "GET", &format!("/api/providers/{id}/usage"), None).await;
        assert_eq!(status, StatusCode::OK, "查询失败：{body}");
        assert_eq!(body["data"]["kind"], "balance");
        assert_eq!(body["data"]["balances"][0]["amount"], 25.0);
        assert_eq!(mock.subscription_hits.load(Ordering::SeqCst), 1);
        assert_eq!(mock.login_hits.load(Ordering::SeqCst), 0);
        assert_eq!(
            mock.auth_headers.lock().unwrap().as_slice(),
            &[Some("Bearer jwt-old".to_string())]
        );
    })
    .await;
}

#[tokio::test]
async fn krill_missing_jwt_logs_in_and_writes_encrypted_token() {
    use llm_gateway::crypto::ENCRYPTION_KEY_ENV;

    let (mock_base, mock) = spawn_krill_mock(
        vec![(StatusCode::OK, krill_balance_reply())],
        krill_login_reply(),
    )
    .await;
    temp_env::async_with_vars(
        [
            (OVERRIDE_ENV, Some(mock_base.as_str())),
            (ENCRYPTION_KEY_ENV, Some("krill-test-key")),
        ],
        async {
            let (app, db) = setup_app_with_db().await;
            let id = create_provider(
                &app,
                "Krill-首次登录",
                "https://api.krill-ai.net/v1",
                r#"{"usage":true,"usage_type":0,"email":"u@example.com","password":"pw","jwt":"","keep":"value"}"#,
            )
            .await;

            let (status, body) =
                send(&app, "GET", &format!("/api/providers/{id}/usage"), None).await;
            assert_eq!(status, StatusCode::OK, "查询失败：{body}");
            assert_eq!(mock.login_hits.load(Ordering::SeqCst), 1);
            assert_eq!(mock.subscription_hits.load(Ordering::SeqCst), 1);
            assert_eq!(
                mock.auth_headers.lock().unwrap().as_slice(),
                &[Some("Bearer jwt-new".to_string())]
            );

            let row = provider::Entity::find_by_id(id as i32)
                .one(&db)
                .await
                .unwrap()
                .unwrap();
            assert!(row.extra.starts_with("enc:v1:"));
            let extra: Value =
                serde_json::from_str(&llm_gateway::crypto::decrypt(&row.extra).unwrap()).unwrap();
            assert_eq!(extra["jwt"], "jwt-new");
            assert_eq!(extra["password"], "pw");
            assert_eq!(extra["keep"], "value");
        },
    )
    .await;
}

#[tokio::test]
async fn krill_auth_failure_logs_in_once_and_retries_once() {
    let auth_error = serde_json::json!({ "success": false, "code": 401, "message": "expired" });
    let (mock_base, mock) = spawn_krill_mock(
        vec![
            (StatusCode::OK, auth_error),
            (StatusCode::OK, krill_balance_reply()),
        ],
        krill_login_reply(),
    )
    .await;
    temp_env::async_with_vars([(OVERRIDE_ENV, Some(mock_base.as_str()))], async {
        let app = setup_app().await;
        let id = create_provider(
            &app,
            "Krill-JWT过期",
            "https://api.cdn-krill-ai.com/v1",
            r#"{"usage":true,"usage_type":0,"email":"u@example.com","password":"pw","jwt":"jwt-old"}"#,
        )
        .await;

        let (status, body) = send(&app, "GET", &format!("/api/providers/{id}/usage"), None).await;
        assert_eq!(status, StatusCode::OK, "查询失败：{body}");
        assert_eq!(mock.login_hits.load(Ordering::SeqCst), 1);
        assert_eq!(mock.subscription_hits.load(Ordering::SeqCst), 2);
        assert_eq!(
            mock.auth_headers.lock().unwrap().as_slice(),
            &[
                Some("Bearer jwt-old".to_string()),
                Some("Bearer jwt-new".to_string())
            ]
        );
    })
    .await;
}

#[tokio::test]
async fn krill_upstream_error_does_not_login() {
    let (mock_base, mock) = spawn_krill_mock(
        vec![(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "message": "down" }),
        )],
        krill_login_reply(),
    )
    .await;
    temp_env::async_with_vars([(OVERRIDE_ENV, Some(mock_base.as_str()))], async {
        let app = setup_app().await;
        let id = create_provider(
            &app,
            "Krill-上游故障",
            "https://api-slb.krill-ai.net/v1",
            r#"{"usage":true,"usage_type":0,"email":"u@example.com","password":"pw","jwt":"jwt-old"}"#,
        )
        .await;

        let (status, _) = send(&app, "GET", &format!("/api/providers/{id}/usage"), None).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(mock.subscription_hits.load(Ordering::SeqCst), 1);
        assert_eq!(mock.login_hits.load(Ordering::SeqCst), 0);
    })
    .await;
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
            // 无 refresh_token 也无 username/password → 缺用户可维护凭据 username。
            r#"{"usage": true, "usage_type": 0}"#,
        )
        .await;
        let (status, body) = send(&app, "GET", &format!("/api/providers/{id}/usage"), None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body["msg"].as_str().unwrap().contains("username"));
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

// ─── SenseNova 登录自愈：refresh_token 失效 → 账号密码登录 → 写回 → 重试 ──
//
// mock 覆盖登录六步（research.md）：
//   GET  /oauth2/auth（无 login_verifier）→ 302 /login?login_challenge=
//   GET  /.well-known/jwks.json             → 测试 RSA 公钥
//   POST /iam/authn/v1/auth/nova/login      → 200 {redirect:/oauth2/auth?login_verifier=}
//   GET  /oauth2/auth?login_verifier=       → 302 /?code=login-code
//   POST /oauth2/token（authorization_code）→ access_token + refresh_token(rt-logged-in)
// 续期/查询复用既有 /oauth2/token 与 pool-usage 路由：按 grant_type/refresh_token 区分。
// 所有 host 经 OVERRIDE_ENV 重写到本 mock；登录子客户端同样读该环境变量。

#[derive(Clone, Default)]
struct SensenovaLoginMockState {
    login_hits: Arc<AtomicUsize>,
    token_hits: Arc<AtomicUsize>,
    last_login_body: Arc<std::sync::Mutex<Option<String>>>,
    login_should_fail: Arc<std::sync::Mutex<bool>>,
}

async fn spawn_sensenova_login_mock() -> (String, SensenovaLoginMockState) {
    use axum::Router;
    use axum::http::header::LOCATION;
    use axum::http::{HeaderMap, HeaderValue, StatusCode as AxumStatus};
    use axum::routing::{get, post};

    let state = SensenovaLoginMockState::default();
    let app = {
        let st = state.clone();
        let st2 = state.clone();
        let st3 = state.clone();
        let st4 = state.clone();
        Router::new()
            // 初始 authorize → 302 到登录页（带 login_challenge）。
            .route(
                "/oauth2/auth",
                get(move |uri: axum::http::Uri| {
                    let st = st.clone();
                    async move {
                        let q = uri.query().unwrap_or("");
                        if q.contains("login_verifier=") {
                            // 登录后携 login_verifier 回来 → 302 到 ?code=
                            let mut r = axum::response::Response::new(axum::body::Body::empty());
                            *r.status_mut() = AxumStatus::FOUND;
                            r.headers_mut().insert(
                                LOCATION,
                                HeaderValue::from_str("/?code=login-code-1").unwrap(),
                            );
                            r
                        } else {
                            // 初始 authorize → 登录页
                            let _ = &st;
                            let mut r = axum::response::Response::new(axum::body::Body::empty());
                            *r.status_mut() = AxumStatus::FOUND;
                            r.headers_mut().insert(
                                LOCATION,
                                HeaderValue::from_str(
                                    "/login?login_challenge=abc123loginchallenge",
                                )
                                .unwrap(),
                            );
                            r
                        }
                    }
                }),
            )
            // 登录页本体：200（跟随到此处即拿到 login_challenge）。
            .route("/login", get(|| async { "login page" }))
            // JWKS：测试 RSA 公钥（2048 位，kid=public:hydra.openid.id-token）。
            .route(
                "/.well-known/jwks.json",
                get(|| async {
                    Json(serde_json::json!({
                        "keys": [{
                            "kid": "public:hydra.openid.id-token",
                            "kty": "RSA",
                            "alg": "RS256",
                            "use": "sig",
                            "n": "5nsU994-8lnsOb93Lzu8lIYr92Rhdyw7UXaEKBpIRJYdVQRKFUFynWUS-MlDi19STFK_PvYBmC0fTLhfsTEp-zJIPuBLhpvW_3nHwtiLnlhCuRTelZYwsIsMds2-4gCx_bynVKSp6ZvdZ7781mWvy3zpVuG-2z02YSno1Yi_txVTjXzZnb0Jf_EOjbWjh9N6s-gaTVLVu34gZ0vkEICQ_Mn1mzdMVpcBfN4v7KxnsiyjYorGAdeMwPxAyPlIFi1oxKhknLZTWGuypURZp2adMY9CiK0yZqVR3TaRgQ3cowrTHW-oIbXq5lHFVNickn_NnBq-wiGgwjgsg54lFDvWrw",
                            "e": "AQAB"
                        }]
                    }))
                }),
            )
            // nova/login：记录请求体；默认返回 redirect（登录成功），可配置失败。
            .route(
                "/iam/authn/v1/auth/nova/login",
                post(move |body: String| {
                    let st = st2.clone();
                    async move {
                        st.login_hits.fetch_add(1, Ordering::SeqCst);
                        *st.last_login_body.lock().unwrap() = Some(body.clone());
                        if *st.login_should_fail.lock().unwrap() {
                            return Json(serde_json::json!({
                                "code": 3,
                                "message": "InvalidArgument",
                                "details": [{
                                    "@type": "type.googleapis.com/google.rpc.ErrorInfo",
                                    "reason": "incorrectUsernameOrPassword",
                                    "domain": "iam",
                                    "metadata": {}
                                }]
                            }));
                        }
                        Json(serde_json::json!({
                            "access_token": "",
                            "refresh_token": "",
                            "redirect": "https://platform.sensenova.cn/oauth2/auth?client_id=nova&login_verifier=verifier-1&redirect_uri=https%3A%2F%2Fplatform.sensenova.cn&response_type=code&scope=openid+offline+offline_access&state=s",
                        }))
                    }
                }),
            )
            // token 端点：authorization_code → 登录产物 rt-logged-in；
            // refresh_token=rt-dead → invalid_grant；refresh_token=rt-logged-in → 续期成功。
            .route(
                "/oauth2/token",
                post(move |body: String| {
                    let st = st3.clone();
                    async move {
                        st.token_hits.fetch_add(1, Ordering::SeqCst);
                        if body.contains("grant_type=authorization_code") {
                            (
                                AxumStatus::OK,
                                Json(serde_json::json!({
                                    "access_token": "at-login",
                                    "expires_in": 10799,
                                    "refresh_token": "rt-logged-in",
                                    "scope": "openid offline offline_access",
                                    "token_type": "bearer"
                                })),
                            )
                        } else if body.contains("refresh_token=rt-dead") {
                            // 生产实测：失效 refresh_token 返回 HTTP 400 + invalid_grant
                            //（非 200/401/403），应同样触发登录自愈。
                            (
                                AxumStatus::BAD_REQUEST,
                                Json(serde_json::json!({
                                    "error": "invalid_grant",
                                    "error_description": "The refresh token is invalid"
                                })),
                            )
                        } else {
                            (
                                AxumStatus::OK,
                                Json(serde_json::json!({
                                    "access_token": "at-renewed",
                                    "expires_in": 10799,
                                    "refresh_token": "rt-renewed-2",
                                    "token_type": "bearer"
                                })),
                            )
                        }
                    }
                }),
            )
            // 根路径（code 落地）+ pool-usage。
            .route("/", get(|| async { "callback" }))
            .route(
                "/lite/console/v1/tokenplan/pool-usage",
                get(move |headers: HeaderMap| {
                    let st = st4.clone();
                    async move {
                        let _ = &st;
                        let _ = headers;
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

/// 场景 1：refresh_token 失效（invalid_grant）且有账号密码 → 自动登录 → 写回新
/// refresh_token → 用新 token 续期查询成功。
#[tokio::test]
async fn sensenova_invalid_grant_self_heals_via_login_and_writes_back() {
    let (mock_base, mock) = spawn_sensenova_login_mock().await;
    temp_env::async_with_vars([(OVERRIDE_ENV, Some(mock_base.as_str()))], async {
        let (app, db) = setup_app_with_db().await;
        let id = create_provider(
            &app,
            "SenseNova-自愈",
            "https://token.sensenova.cn/v1",
            r#"{"usage": true, "usage_type": 0, "refresh_token": "rt-dead", "username": "ijkzen", "password": "pw"}"#,
        )
        .await;

        let (status, body) = send(&app, "GET", &format!("/api/providers/{id}/usage"), None).await;
        assert_eq!(status, StatusCode::OK, "查询失败：{body}");
        // 登录确实发生了一次。
        assert_eq!(mock.login_hits.load(Ordering::SeqCst), 1);
        // 登录请求体包含 username 与 5 段 JWE 密码。
        let login_body = mock.last_login_body.lock().unwrap().clone().unwrap();
        let login_json: Value = serde_json::from_str(&login_body).unwrap();
        assert_eq!(login_json["username"], "ijkzen");
        assert_eq!(login_json["is_encrypt"], true);
        let jwe = login_json["password"].as_str().unwrap();
        assert_eq!(jwe.split('.').count(), 5, "密码应为 5 段 JWE");

        // 新 refresh_token 已写回 provider extra。
        let model = provider::Entity::find_by_id(id as i32)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        let extra: Value = serde_json::from_str(&model.extra).unwrap();
        assert_eq!(extra["refresh_token"], "rt-logged-in");
        assert_eq!(extra["username"], "ijkzen", "其余键保留");
        assert_eq!(extra["password"], "pw");
    })
    .await;
}

/// 场景 2：refresh_token 缺失、只有账号密码 → 直接登录引导写回并查询成功。
#[tokio::test]
async fn sensenova_missing_refresh_token_logs_in_with_credentials() {
    let (mock_base, mock) = spawn_sensenova_login_mock().await;
    temp_env::async_with_vars([(OVERRIDE_ENV, Some(mock_base.as_str()))], async {
        let (app, db) = setup_app_with_db().await;
        let id = create_provider(
            &app,
            "SenseNova-仅账号密码",
            "https://token.sensenova.cn/v1",
            r#"{"usage": true, "usage_type": 0, "username": "ijkzen", "password": "pw"}"#,
        )
        .await;

        let (status, body) = send(&app, "GET", &format!("/api/providers/{id}/usage"), None).await;
        assert_eq!(status, StatusCode::OK, "查询失败：{body}");
        assert_eq!(mock.login_hits.load(Ordering::SeqCst), 1);
        let model = provider::Entity::find_by_id(id as i32)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        let extra: Value = serde_json::from_str(&model.extra).unwrap();
        assert_eq!(extra["refresh_token"], "rt-logged-in");
    })
    .await;
}

/// 场景 3：登录失败（账号或密码错误）→ Auth → 400「用量查询凭据无效或已过期」。
#[tokio::test]
async fn sensenova_login_failure_maps_to_auth_error() {
    let (mock_base, mock) = spawn_sensenova_login_mock().await;
    temp_env::async_with_vars([(OVERRIDE_ENV, Some(mock_base.as_str()))], async {
        let app = setup_app().await;
        *mock.login_should_fail.lock().unwrap() = true;
        let id = create_provider(
            &app,
            "SenseNova-登录失败",
            "https://token.sensenova.cn/v1",
            r#"{"usage": true, "usage_type": 0, "refresh_token": "rt-dead", "username": "ijkzen", "password": "wrong"}"#,
        )
        .await;
        let (status, body) = send(&app, "GET", &format!("/api/providers/{id}/usage"), None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["msg"], "用量查询凭据无效或已过期");
    })
    .await;
}

// ─── 用量抓取走 provider 网络代理 ─────────────────────────────────────────────
// 场景：provider 开启网络代理（proxyEnabled + proxyAddr）。用量抓取应经
// 代理转发到厂商端点（与主转发链路一致），而不是直连。
//
// 验证手法：mock 一个代理服务器（收到请求后桥接到目标），同时用 OVERRIDE_ENV
// 把厂商 URL 重写到本地目标 mock。若抓取真走了代理，代理收到请求；若代码没生效
// （直连），目标 mock 也能通但代理计数为 0。
//
// 注意：reqwest 对 http:// 目标走「正向代理」（请求行带完整 URL），对 https://
// 才走 CONNECT 隧道。OVERWRITE 把 URL 变成 http://127.0.0.1:<port>，因此这里
// 代理 mock 需要支持正向代理形式，而不是只认 CONNECT。
async fn spawn_forward_proxy_usage() -> (String, Arc<AtomicUsize>) {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let request_count = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&request_count);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (mut client, _) = match listener.accept().await {
                Ok(accepted) => accepted,
                Err(_) => break,
            };
            count.fetch_add(1, AtomicOrdering::SeqCst);
            tokio::spawn(async move {
                // 读请求头（到 \r\n\r\n）。
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
                // CONNECT host:port → 隧道模式。
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
                // 正向代理：请求行是 `METHOD http://host/path HTTP/1.1`，转发给目标。
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
                // 重写请求行为 path-only + Host 头，转发。
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
    (format!("http://{addr}"), request_count)
}

/// provider 开启网络代理时，用量抓取经代理转发。
#[tokio::test]
async fn usage_goes_through_provider_proxy() {
    let (mock_base, target_counter) = spawn_mock().await;
    let (proxy_addr, connect_counter) = spawn_forward_proxy_usage().await;
    temp_env::async_with_vars([(OVERRIDE_ENV, Some(mock_base.as_str()))], async {
        let app = setup_app().await;
        let body = serde_json::json!({
            "name": "DeepSeek-代理",
            "enable": true,
            "baseUrl": "https://api.deepseek.com",
            "apiKey": "sk-usage-proxy",
            "protocolType": 0,
            "billingMode": 0,
            "customHeader": "{}",
            "extra": r#"{"usage": true, "usage_type": 0}"#,
            "proxyEnabled": true,
            "proxyAddr": proxy_addr,
        })
        .to_string();
        let (status, body) = send(&app, "POST", "/api/providers", Some(&body)).await;
        assert_eq!(status, StatusCode::CREATED, "创建失败：{body}");
        let id = body["data"]["id"].as_i64().unwrap();

        let (status, resp) = send(&app, "GET", &format!("/api/providers/{id}/usage"), None).await;
        assert_eq!(status, StatusCode::OK, "查询失败：{resp}");
        assert_eq!(resp["data"]["kind"], "balance");
        assert_eq!(
            connect_counter.load(Ordering::SeqCst),
            1,
            "用量抓取应经代理一次"
        );
        assert_eq!(
            target_counter.load(Ordering::SeqCst),
            1,
            "目标 mock 应收到 1 次请求"
        );
    })
    .await;
}

/// provider 未开启代理时用量抓取仍直连（不引入代理）。
#[tokio::test]
async fn usage_direct_without_proxy_still_works() {
    let (mock_base, target_counter) = spawn_mock().await;
    temp_env::async_with_vars([(OVERRIDE_ENV, Some(mock_base.as_str()))], async {
        let app = setup_app().await;
        let id = create_provider(
            &app,
            "DeepSeek-直连",
            "https://api.deepseek.com",
            r#"{"usage": true, "usage_type": 0}"#,
        )
        .await;
        let (status, body) = send(&app, "GET", &format!("/api/providers/{id}/usage"), None).await;
        assert_eq!(status, StatusCode::OK, "查询失败：{body}");
        assert_eq!(target_counter.load(Ordering::SeqCst), 1);
    })
    .await;
}

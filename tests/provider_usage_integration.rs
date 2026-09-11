//! Provider 用量查询接口（GET /api/providers/{id}/usage）集成测试。
//!
//! 成功路径通过环境变量 `LLM_GATEWAY_USAGE_HTTP_OVERRIDE` 将用量请求重定向到
//! 本地 mock（DeepSeek 余额形态），并验证 60s 缓存与 ?refresh=1 绕过行为。
//!
//! SenseNova 登录测试有意用同步锁跨 await 串行化共享冷却状态。
#![allow(clippy::await_holding_lock)]

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

#[path = "provider_usage_integration/cache.rs"]
mod cache;
#[path = "provider_usage_integration/krill.rs"]
mod krill;
#[path = "provider_usage_integration/sensenova.rs"]
mod sensenova;
#[path = "provider_usage_integration/token_accounts.rs"]
mod token_accounts;
#[path = "provider_usage_integration/usage_proxy.rs"]
mod usage_proxy;

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

// ── TokenRhythm：账号密码登录换会话 Cookie + 钱包查询 ──

#[derive(Clone)]
struct TokenRhythmMockState {
    wallet_replies: Arc<std::sync::Mutex<std::collections::VecDeque<(StatusCode, Value)>>>,
    wallet_hits: Arc<AtomicUsize>,
    login_hits: Arc<AtomicUsize>,
    cookie_headers: Arc<std::sync::Mutex<Vec<Option<String>>>>,
}

/// TokenRhythm mock：`/api/auth/login` 下发 tr_session Cookie，
/// `/api/wallet/summary` 按队列依次返回响应并记录请求 Cookie 头。
async fn spawn_tokenrhythm_mock(
    wallet_replies: Vec<(StatusCode, Value)>,
) -> (String, TokenRhythmMockState) {
    use axum::http::{HeaderMap, header};
    use axum::response::IntoResponse;
    use axum::routing::{get, post};

    let state = TokenRhythmMockState {
        wallet_replies: Arc::new(std::sync::Mutex::new(wallet_replies.into())),
        wallet_hits: Arc::new(AtomicUsize::new(0)),
        login_hits: Arc::new(AtomicUsize::new(0)),
        cookie_headers: Arc::new(std::sync::Mutex::new(Vec::new())),
    };
    let wallet_state = state.clone();
    let login_state = state.clone();
    let app = axum::Router::new()
        .route(
            "/api/wallet/summary",
            get(move |headers: HeaderMap| {
                let state = wallet_state.clone();
                async move {
                    state.wallet_hits.fetch_add(1, Ordering::SeqCst);
                    state.cookie_headers.lock().unwrap().push(
                        headers
                            .get("cookie")
                            .and_then(|value| value.to_str().ok())
                            .map(str::to_string),
                    );
                    let (status, body) = state
                        .wallet_replies
                        .lock()
                        .unwrap()
                        .pop_front()
                        .expect("unexpected wallet request");
                    (status, Json(body)).into_response()
                }
            }),
        )
        .route(
            "/api/auth/login",
            post(move || {
                let state = login_state.clone();
                async move {
                    state.login_hits.fetch_add(1, Ordering::SeqCst);
                    (
                        StatusCode::OK,
                        [(
                            header::SET_COOKIE,
                            "tr_session=sess-new; Max-Age=2592000; HttpOnly; Path=/; SameSite=Lax; Secure",
                        )],
                        Json(serde_json::json!({
                            "code": 0,
                            "message": "ok",
                            "data": { "user": { "id": "u1", "name": "w5aw3e", "status": "active" } }
                        })),
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
    (format!("http://{addr}"), state)
}

fn tokenrhythm_wallet_reply(available: &str) -> Value {
    serde_json::json!({
        "code": 0,
        "message": "ok",
        "data": {
            "currency": "CNY",
            "availableBalanceCny": available,
            "giftAvailableCny": available,
            "giftStatus": "active",
            "asOf": "2026-09-11T07:33:10.838Z"
        }
    })
}

fn tokenrhythm_expired_reply() -> (StatusCode, Value) {
    (
        StatusCode::UNAUTHORIZED,
        serde_json::json!({ "code": "UNAUTHORIZED", "message": "未认证或登录已过期" }),
    )
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

/// 触发真实登录的测试共享 sensenova 模块级登录冷却静态，必须串行执行，
/// 否则并行下失败用例的冷却会污染成功用例。
static SENSENOVA_LOGIN_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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

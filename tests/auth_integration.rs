//! 认证集成测试：初始化流程、登录、会话拦截、修改密码、登出与 /v1 Bearer 鉴权。

mod common;

use axum::body::Body;
use axum::http::{Request, header};
use llm_gateway::entity::user;
use sea_orm::{ActiveModelTrait, EntityTrait, PaginatorTrait, Set};
use serde_json::{Value, json};
use tower::ServiceExt;

const ADMIN: &str = "Admin";
const PASSWORD: &str = "Password";

async fn setup_app() -> (axum::Router, sea_orm::DatabaseConnection) {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    scheduler.start().await.unwrap();
    let app = common::build_app(db.clone(), scheduler, log_tx);
    (app, db)
}

type TestResponse = (u16, Vec<(String, String)>, Value);

async fn send_with_headers(
    app: axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
    extra_headers: &[(&str, &str)],
) -> TestResponse {
    let mut builder = Request::builder().method(method).uri(uri);
    for (name, value) in extra_headers {
        builder = builder.header(*name, *value);
    }
    if body.is_some() {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    let request = builder
        .body(Body::from(body.map(|b| b.to_string()).unwrap_or_default()))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    let status = response.status().as_u16();
    let headers: Vec<(String, String)> = response
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or_default().to_string()))
        .collect();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let parsed: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, headers, parsed)
}

async fn send_json(app: axum::Router, method: &str, uri: &str, body: Value) -> TestResponse {
    send_with_headers(app, method, uri, Some(body), &[]).await
}

/// 从 Set-Cookie 响应头提取 lg_session 令牌。
fn session_token_from(headers: &[(String, String)]) -> Option<String> {
    headers.iter().find_map(|(name, value)| {
        if name == "set-cookie" && value.starts_with("lg_session=") {
            value
                .split(';')
                .next()
                .and_then(|pair| pair.split_once('='))
                .map(|(_, token)| token.to_string())
        } else {
            None
        }
    })
}

fn cookie(token: &str) -> [(&'static str, String); 1] {
    [("cookie", format!("lg_session={token}"))]
}

async fn init_admin(app: &axum::Router) -> String {
    let (status, headers, body) = send_json(
        app.clone(),
        "POST",
        "/api/auth/init",
        json!({ "username": ADMIN, "password": PASSWORD }),
    )
    .await;
    assert_eq!(status, 200, "init should succeed: {body}");
    assert_eq!(body["code"], "0");
    assert_eq!(body["data"]["username"], ADMIN);
    session_token_from(&headers).expect("init should set session cookie")
}

#[tokio::test]
async fn init_flow_creates_first_user_and_session() {
    let (app, _db) = setup_app().await;

    let (status, _, body) = send_json(app.clone(), "GET", "/api/auth/status", json!({})).await;
    assert_eq!(status, 200);
    assert_eq!(body["data"]["initialized"], false);

    let token = init_admin(&app).await;
    assert!(!token.is_empty());

    let (status, _, body) = send_json(app.clone(), "GET", "/api/auth/status", json!({})).await;
    assert_eq!(status, 200);
    assert_eq!(body["data"]["initialized"], true);

    // 已初始化后再次 init 被拒绝。
    let (status, _, body) = send_json(
        app.clone(),
        "POST",
        "/api/auth/init",
        json!({ "username": "Other", "password": "Secret1" }),
    )
    .await;
    assert_eq!(status, 400);
    assert_eq!(body["code"], "INVALID_INPUT");
}

/// 12-01 回归：并发双初始化（不同用户名）只能建一个用户——原子「表空才插入」
/// 由数据库保证，单用户假设不被破坏。
#[tokio::test]
async fn concurrent_init_creates_exactly_one_user() {
    let (app, db) = setup_app().await;
    let a = app.clone();
    let b = app.clone();
    let h1 = tokio::spawn(async move {
        send_json(
            a,
            "POST",
            "/api/auth/init",
            json!({ "username": "Admin", "password": PASSWORD }),
        )
        .await
    });
    let h2 = tokio::spawn(async move {
        send_json(
            b,
            "POST",
            "/api/auth/init",
            json!({ "username": "Other", "password": PASSWORD }),
        )
        .await
    });
    let (r1, r2) = (h1.await.unwrap(), h2.await.unwrap());
    // 两者恰好一个成功（200 + 会话），另一个 400（已初始化）。
    let statuses = [r1.0, r2.0];
    assert_eq!(
        statuses.iter().filter(|s| **s == 200).count(),
        1,
        "并发 init 应恰好一个成功：{statuses:?}"
    );
    assert_eq!(
        statuses.iter().filter(|s| **s == 400).count(),
        1,
        "另一个应被拒：{statuses:?}"
    );
    // 库里只有一个用户。
    let count = user::Entity::find().count(&db).await.unwrap();
    assert_eq!(count, 1, "并发初始化不得建出两个用户");
}

#[tokio::test]
async fn init_rejects_invalid_input() {
    let (app, _db) = setup_app().await;
    let (status, _, _) = send_json(
        app.clone(),
        "POST",
        "/api/auth/init",
        json!({ "username": "", "password": PASSWORD }),
    )
    .await;
    assert_eq!(status, 400);
    let (status, _, _) = send_json(
        app.clone(),
        "POST",
        "/api/auth/init",
        json!({ "username": ADMIN, "password": "12345" }),
    )
    .await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn login_and_session_guard() {
    let (app, _db) = setup_app().await;
    init_admin(&app).await;

    // 未登录访问管理接口 → 401 统一信封。
    let (status, _, body) = send_json(app.clone(), "GET", "/api/settings", json!({})).await;
    assert_eq!(status, 401);
    assert_eq!(body["code"], "UNAUTHORIZED");

    // healthz 不需要登录，并暴露编译期版本号（跟随 Cargo.toml）。
    let (status, _, body) = send_json(app.clone(), "GET", "/api/healthz", json!({})).await;
    assert_eq!(status, 200);
    assert_eq!(body["status"], "ok");
    assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));

    // 错误密码 → 401。
    let (status, _, body) = send_json(
        app.clone(),
        "POST",
        "/api/auth/login",
        json!({ "username": ADMIN, "password": "wrong-pass" }),
    )
    .await;
    assert_eq!(status, 401);
    assert_eq!(body["code"], "UNAUTHORIZED");

    // 正确登录 → 200 + Cookie。
    let (status, headers, body) = send_json(
        app.clone(),
        "POST",
        "/api/auth/login",
        json!({ "username": ADMIN, "password": PASSWORD }),
    )
    .await;
    assert_eq!(status, 200, "login should succeed: {body}");
    assert_eq!(body["data"]["username"], ADMIN);
    let token = session_token_from(&headers).expect("login sets cookie");
    assert!(
        headers
            .iter()
            .any(|(n, v)| n == "set-cookie" && v.contains("HttpOnly"))
    );

    // 带 Cookie 访问管理接口 → 200，且 /api/auth/me 返回用户名。
    let headers: Vec<(&str, String)> = cookie(&token).to_vec();
    let header_refs: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let (status, _, body) =
        send_with_headers(app.clone(), "GET", "/api/settings", None, &header_refs).await;
    assert_eq!(status, 200);
    assert_eq!(body["code"], "0");
    let (status, _, body) =
        send_with_headers(app.clone(), "GET", "/api/auth/me", None, &header_refs).await;
    assert_eq!(status, 200);
    assert_eq!(body["data"]["username"], ADMIN);

    // 伪造 Cookie → 401。
    let (status, _, _) = send_with_headers(
        app.clone(),
        "GET",
        "/api/settings",
        None,
        &[("cookie", "lg_session=deadbeef")],
    )
    .await;
    assert_eq!(status, 401);
}

/// 请求体不设上限（09-11 拍板去除原 5MB DefaultBodyLimit）：超过 5MB 的请求体
/// 不再被 extractor 以 413 拦下，而是正常进入 handler（密码错误 → 401）。
#[tokio::test]
async fn oversized_request_body_reaches_handler() {
    let (app, _db) = setup_app().await;
    init_admin(&app).await;

    let big = json!({
        "username": ADMIN,
        "password": "a".repeat(6 * 1024 * 1024),
    });
    let (status, _, _) = send_with_headers(app, "POST", "/api/auth/login", Some(big), &[]).await;
    assert_eq!(
        status, 401,
        "超过 5MB 的请求体应进入 handler（密码错误 → 401），而非被 413 拒绝"
    );
}

#[tokio::test]
async fn change_password_revokes_other_sessions() {
    let (app, _db) = setup_app().await;
    let token_a = init_admin(&app).await;

    // 第二个会话。
    let (_, headers, _) = send_json(
        app.clone(),
        "POST",
        "/api/auth/login",
        json!({ "username": ADMIN, "password": PASSWORD }),
    )
    .await;
    let token_b = session_token_from(&headers).unwrap();

    // 旧密码错误 → 400（携带会话 A）。
    let a_headers: Vec<(&str, String)> = cookie(&token_a).to_vec();
    let a_refs: Vec<(&str, &str)> = a_headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let (status, _, _) = send_with_headers(
        app.clone(),
        "POST",
        "/api/auth/change-password",
        Some(json!({ "oldPassword": "wrong-old", "newPassword": "NewPassword1" })),
        &a_refs,
    )
    .await;
    assert_eq!(status, 400);

    // 正确修改（携带会话 A）。
    let a_headers: Vec<(&str, String)> = cookie(&token_a).to_vec();
    let a_refs: Vec<(&str, &str)> = a_headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let (status, _, body) = send_with_headers(
        app.clone(),
        "POST",
        "/api/auth/change-password",
        Some(json!({ "oldPassword": PASSWORD, "newPassword": "NewPassword1" })),
        &a_refs,
    )
    .await;
    assert_eq!(status, 200, "change password should succeed: {body}");

    // 会话 B（其他会话）被吊销。
    let b_cookie = format!("lg_session={token_b}");
    let (status, _, _) = send_with_headers(
        app.clone(),
        "GET",
        "/api/settings",
        None,
        &[("cookie", b_cookie.as_str())],
    )
    .await;
    assert_eq!(status, 401);

    // 会话 A 仍然有效。
    let (status, _, _) = send_with_headers(app.clone(), "GET", "/api/auth/me", None, &a_refs).await;
    assert_eq!(status, 200);

    // 旧密码不能再登录，新密码可以。
    let (status, _, _) = send_json(
        app.clone(),
        "POST",
        "/api/auth/login",
        json!({ "username": ADMIN, "password": PASSWORD }),
    )
    .await;
    assert_eq!(status, 401);
    let (status, _, body) = send_json(
        app.clone(),
        "POST",
        "/api/auth/login",
        json!({ "username": ADMIN, "password": "NewPassword1" }),
    )
    .await;
    assert_eq!(status, 200, "new password should work: {body}");
}

#[tokio::test]
async fn logout_revokes_session() {
    let (app, _db) = setup_app().await;
    let token = init_admin(&app).await;

    let headers: Vec<(&str, String)> = cookie(&token).to_vec();
    let refs: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let (status, _, _) =
        send_with_headers(app.clone(), "POST", "/api/auth/logout", None, &refs).await;
    assert_eq!(status, 200);

    let (status, _, _) = send_with_headers(app.clone(), "GET", "/api/settings", None, &refs).await;
    assert_eq!(status, 401);
}

async fn seed_api_key(db: &sea_orm::DatabaseConnection, name: &str, plain: &str, enable: bool) {
    let active = llm_gateway::entity::api_key::ActiveModel {
        name: Set(name.to_string()),
        key: Set(llm_gateway::crypto::encrypt(plain)),
        key_hash: Set(Some(llm_gateway::auth::hash_token(plain))),
        enable: Set(enable),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    active.insert(db).await.unwrap();
}

#[tokio::test]
async fn v1_requires_bearer_api_key() {
    let (app, db) = setup_app().await;
    seed_api_key(&db, "test-key", "lg-0123456789abcdef", true).await;

    // 无 Bearer → 401 OpenAI 错误格式。
    let (status, _, body) = send_json(app.clone(), "GET", "/v1/models", json!({})).await;
    assert_eq!(status, 401);
    assert!(body["error"]["message"].is_string());
    assert_eq!(body["error"]["code"], "INVALID_API_KEY");

    // 无效 Bearer → 401。
    let (status, _, _) = send_with_headers(
        app.clone(),
        "GET",
        "/v1/models",
        None,
        &[("authorization", "Bearer lg-wrong")],
    )
    .await;
    assert_eq!(status, 401);

    // 有效 Bearer → 200。
    let (status, _, body) = send_with_headers(
        app.clone(),
        "GET",
        "/v1/models",
        None,
        &[("authorization", "Bearer lg-0123456789abcdef")],
    )
    .await;
    assert_eq!(status, 200, "valid key should pass: {body}");
    assert_eq!(body["object"], "list");

    // 禁用的 key → 401。
    seed_api_key(&db, "disabled-key", "lg-disabled-key0000", false).await;
    let (status, _, _) = send_with_headers(
        app.clone(),
        "GET",
        "/v1/models",
        None,
        &[("authorization", "Bearer lg-disabled-key0000")],
    )
    .await;
    assert_eq!(status, 401);
}

/// 12-09（T2）：过期会话请求 401，且该会话行被顺带删除。
#[tokio::test]
async fn expired_session_is_rejected_and_row_purged() {
    use sea_orm::{ActiveModelTrait, EntityTrait, Set};
    let (app, db) = setup_app().await;
    init_admin(&app).await;

    // 直接种一条已过期会话（token 明文；表存 SHA-256 摘要）。
    let token = "expired-token-abc";
    let user = llm_gateway::entity::user::Entity::find()
        .one(&db)
        .await
        .unwrap()
        .expect("init 应已建用户");
    llm_gateway::entity::session::ActiveModel {
        id: Set(llm_gateway::auth::hash_token(token)),
        user_id: Set(user.id),
        expires_at: Set(chrono::Utc::now() - chrono::TimeDelta::hours(1)),
        created_at: Set(chrono::Utc::now()),
    }
    .insert(&db)
    .await
    .unwrap();

    let (status, _, _) = send_with_headers(
        app.clone(),
        "GET",
        "/api/settings",
        None,
        &[("cookie", &format!("lg_session={token}"))],
    )
    .await;
    assert_eq!(status, 401, "过期会话应 401");

    // 过期行被顺带清理（session_user 的 purge 分支）。
    let remaining = llm_gateway::entity::session::Entity::find()
        .all(&db)
        .await
        .unwrap();
    assert!(
        remaining
            .iter()
            .all(|s| s.id != llm_gateway::auth::hash_token(token)),
        "过期会话行应被删除"
    );
}

/// 12-09（T4/12-06）：Set-Cookie 属性完整，且 logout 在无会话时也公开可用。
#[tokio::test]
async fn cookie_attributes_and_public_logout() {
    let (app, _db) = setup_app().await;
    init_admin(&app).await;
    let (status, headers, _) = send_json(
        app.clone(),
        "POST",
        "/api/auth/login",
        json!({ "username": ADMIN, "password": PASSWORD }),
    )
    .await;
    assert_eq!(status, 200);
    let set_cookie = headers
        .iter()
        .find(|(n, _)| n == "set-cookie")
        .map(|(_, v)| v.clone())
        .expect("应设置会话 Cookie");
    for needle in ["HttpOnly", "SameSite=Lax", "Path=/", "Max-Age="] {
        assert!(set_cookie.contains(needle), "缺少 {needle}：{set_cookie}");
    }
    // 12-07：Max-Age 取自 SESSION_TTL_SECS 常量（不再字面量漂移）。
    // 剩余秒由 expires_at − now 取整得出，允许 1 秒内舍入。
    let ttl = llm_gateway::auth::SESSION_TTL_SECS;
    let max_age: i64 = set_cookie
        .split("Max-Age=")
        .nth(1)
        .and_then(|rest| rest.split(';').next())
        .and_then(|v| v.trim().parse().ok())
        .expect("应带 Max-Age 值");
    assert!(
        (ttl - max_age).abs() <= 1,
        "Max-Age 应等于会话 TTL 常量（±1s 舍入）：ttl={ttl} max_age={max_age}"
    );

    // 12-05：无会话也能 logout（公开路径，幂等清 cookie）。
    let (status, headers, body) =
        send_json(app.clone(), "POST", "/api/auth/logout", json!({})).await;
    assert_eq!(status, 200, "无会话 logout 应成功：{body}");
    assert!(
        headers
            .iter()
            .any(|(n, v)| n == "set-cookie" && v.contains("Max-Age=0")),
        "logout 应下发清 cookie 指令"
    );
}

/// 12-06：`/v1/messages` 前缀有边界——`/v1/messages-foo` 不按 messages 处理
/// （用 x-api-key 头判断：仅 messages 端点接受该头放行）。
#[tokio::test]
async fn v1_messages_prefix_requires_boundary() {
    let (app, db) = setup_app().await;
    seed_api_key(&db, "k1", "lg-test-key-1", true).await;

    // 精确 messages 路径 + x-api-key：通过鉴权（后续 404/业务错误与鉴权无关）。
    let (status, _, _) = send_with_headers(
        app.clone(),
        "POST",
        "/v1/messages",
        Some(json!({})),
        &[("x-api-key", "lg-test-key-1")],
    )
    .await;
    assert_ne!(status, 401, "messages 端点应接受 x-api-key");

    // 带后缀的同前缀路径：不应把 x-api-key 当作有效凭证。
    let (status, _, _) = send_with_headers(
        app.clone(),
        "POST",
        "/v1/messages-foo",
        Some(json!({})),
        &[("x-api-key", "lg-test-key-1")],
    )
    .await;
    assert_eq!(status, 401, "带后缀路径不应按 messages 接受 x-api-key");
}

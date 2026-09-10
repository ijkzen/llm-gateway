use super::*;

/// AgentRouter 端到端：CookieCloud 拉取解密登录态 + New-Api-User 头 → 拉
/// /api/user/self 的 quota → ÷500000 归一化为单条美元余额。
///
/// mock 同时提供 CookieCloud `/get/{uuid}` 与 `/api/user/self`；CookieCloud
/// 载荷由 openssl 预生成（md5(uuid-password) 前 16 字符作 passphrase），
/// 断言请求头带 Cookie（session/acw_tc）与 New-Api-User、浏览器 UA。
#[tokio::test]
async fn agentrouter_usage_fetches_quota_balance_with_cookie_and_user_header() {
    use axum::http::HeaderMap;
    use axum::response::IntoResponse;
    use axum::routing::get;

    // openssl enc -aes-256-cbc -md md5 -salt（见 cookiecloud.rs 测试向量说明）。
    const CLOUD_ENCRYPTED: &str = "U2FsdGVkX1/Xcm7x191hHmE3HGxu7lVc4mBt2U032Ptv7ArbR+PvVOFZz5nbeLdbrjlZ683ZKmstnvcmRanGZPr0nrwuPoV5ajlUtbdPIZU9SSNGAuG1pDxsRDVtYa4pyTZM3HqTbkFt8oJw5VvqY0HHxu/ZxfPImH3rdnFJC5wf8Inv5lvj9xB+b0bMtrmhG0TumnlPu16TRJcNxgnH+/bPC+LzVwL9+mIfgWe+O5A=";
    let seen = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let seen_clone = seen.clone();
    let app = axum::Router::new()
        .route(
            "/get/ar-test-uuid-0001",
            get(|| async move {
                Json(serde_json::json!({ "encrypted": CLOUD_ENCRYPTED })).into_response()
            }),
        )
        .route(
            "/api/user/self",
            get(move |headers: HeaderMap| {
                let seen = seen_clone.clone();
                async move {
                    for name in ["cookie", "new-api-user", "user-agent"] {
                        if let Some(v) = headers.get(name).and_then(|v| v.to_str().ok()) {
                            seen.lock().unwrap().push(format!("{name}={v}"));
                        }
                    }
                    Json(serde_json::json!({
                        "data": {
                            "id": 591449,
                            "username": "github_591449",
                            "display_name": "IJKZEN",
                            "quota": 99998626,
                            "used_quota": 1374,
                            "request_count": 7,
                            "group": "default"
                        },
                        "message": "",
                        "success": true
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
    let mock_base = format!("http://{addr}");

    temp_env::async_with_vars([(OVERRIDE_ENV, Some(mock_base.as_str()))], async {
        let app = setup_app().await;
        // CookieCloud 四键的 server/uuid/password/domain + new_api_user 全配齐。
        let id = create_provider(
            &app,
            "AgentRouter-用量",
            "https://agentrouter.org/v1",
            r#"{"usage": true, "usage_type": 0, "cookie_cloud_server": "https://cc.example", "uuid": "ar-test-uuid-0001", "password": "ar-test-pass", "domain": "agentrouter.org", "new_api_user": "591449"}"#,
        )
        .await;

        let (status, body) = send(
            &app,
            "GET",
            &format!("/api/providers/{id}/usage?refresh=1"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "查询失败：{body}");
        let data = &body["data"];
        assert_eq!(data["kind"], "balance");
        assert_eq!(data["balances"].as_array().unwrap().len(), 1);
        assert_eq!(data["balances"][0]["label"], "剩余额度");
        assert_eq!(data["balances"][0]["amount"], 200.0); // 99998626 / 500000 = 199.997252 → 200.0
        assert_eq!(data["balances"][0]["currency"], "USD");
        assert_eq!(
            data["balances"][0]["primary"].as_bool(),
            Some(true),
            "单条余额应为 primary"
        );

        // 出站头断言：Cookie 含 session 与 acw_tc；New-Api-User；浏览器 UA。
        let seen = seen.lock().unwrap();
        let joined = seen.join("\n");
        assert!(
            joined.contains("cookie=session=sess-abc123; acw_tc=acw-tc-xyz789"),
            "应带 CookieCloud 解出的 Cookie 头：{joined}"
        );
        assert!(
            joined.contains("new-api-user=591449"),
            "应带 New-Api-User 头：{joined}"
        );
        assert!(
            joined.contains("user-agent=Mozilla/5.0"),
            "应带浏览器 UA：{joined}"
        );
    })
    .await;
}

/// TokenRhythm 端到端：CookieCloud 拉取解密登录态 Cookie → 拉
/// /api/wallet/summary 的 availableBalanceCny → 归一化为单条 CNY 余额。
///
/// mock 同时提供 CookieCloud `/get/{uuid}` 与 `/api/wallet/summary`；CookieCloud
/// 载荷由 openssl 预生成（md5(uuid-password) 前 16 字符作 passphrase），
/// 断言请求头带 Cookie（tr_session/tr_csrf）与浏览器 UA。
#[tokio::test]
async fn tokenrhythm_usage_fetches_wallet_balance_with_cookie() {
    use axum::http::HeaderMap;
    use axum::response::IntoResponse;
    use axum::routing::get;

    // openssl enc -aes-256-cbc -md md5 -salt（见 cookiecloud.rs 测试向量说明）。
    const CLOUD_ENCRYPTED: &str = "U2FsdGVkX1+0e4tdFG+3Mf+hiNa/tg4ApRzkK8wrp79wvU/M+kusiwH8HsvHXTei7N2fbXN7oEaPqCLbLQXUWwwJH2xlg8K8P7ZUhkUY5bFSf+1K9XbNpMPN/SkqfhNT2JLow20Ydf9vAjTLi6imoW3LfLVNo8oty1ratupZUbA1b965vzSpSff6xqj2w1NhZPzBKeuGplgF7hHuW0s+F4kKzrqNjCWjw51z0BcaD0k=";
    let seen = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let seen_clone = seen.clone();
    let app = axum::Router::new()
        .route(
            "/get/tr-test-uuid-0002",
            get(|| async move {
                Json(serde_json::json!({ "encrypted": CLOUD_ENCRYPTED })).into_response()
            }),
        )
        .route(
            "/api/wallet/summary",
            get(move |headers: HeaderMap| {
                let seen = seen_clone.clone();
                async move {
                    for name in ["cookie", "user-agent"] {
                        if let Some(v) = headers.get(name).and_then(|v| v.to_str().ok()) {
                            seen.lock().unwrap().push(format!("{name}={v}"));
                        }
                    }
                    Json(serde_json::json!({
                        "code": 0,
                        "message": "ok",
                        "data": {
                            "currency": "CNY",
                            "availableBalanceCny": "9.74899120",
                            "giftAvailableCny": "9.74899120",
                            "giftLockedCny": "58.00000000",
                            "rechargeBalanceCny": "0.00000000",
                            "debtBalanceCny": "0.00000000",
                            "frozenBalanceCny": "0.00000000",
                            "giftTotalCny": "67.74899120",
                            "giftStatus": "pending_activation",
                            "voidedGiftCny": "0.00000000",
                            "asOf": "2026-09-07T06:00:25.806Z"
                        },
                        "traceId": "trace_fc2e5a73-207f-46bf-9cb1-f00a85bf5be3"
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
    let mock_base = format!("http://{addr}");

    temp_env::async_with_vars([(OVERRIDE_ENV, Some(mock_base.as_str()))], async {
        let app = setup_app().await;
        // CookieCloud 四键的 server/uuid/password/domain 全配齐。
        let id = create_provider(
            &app,
            "TokenRhythm-用量",
            "https://tokenrhythm.studio/v1",
            r#"{"usage": true, "usage_type": 0, "cookie_cloud_server": "https://cc.example", "uuid": "tr-test-uuid-0002", "password": "tr-test-pass", "domain": "tokenrhythm.studio"}"#,
        )
        .await;

        let (status, body) = send(
            &app,
            "GET",
            &format!("/api/providers/{id}/usage?refresh=1"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "查询失败：{body}");
        let data = &body["data"];
        assert_eq!(data["kind"], "balance");
        assert_eq!(data["balances"].as_array().unwrap().len(), 1);
        assert_eq!(data["balances"][0]["label"], "可用余额");
        assert!((data["balances"][0]["amount"].as_f64().unwrap() - 9.75).abs() < 1e-9);
        assert_eq!(data["balances"][0]["currency"], "CNY");
        assert_eq!(
            data["balances"][0]["primary"].as_bool(),
            Some(true),
            "单条余额应为 primary"
        );

        // 出站头断言：Cookie 含 tr_session 与 tr_csrf；浏览器 UA。
        let seen = seen.lock().unwrap();
        let joined = seen.join("\n");
        assert!(
            joined.contains("cookie=tr_session=sess-tr-xyz; tr_csrf=csrf-abc"),
            "应带 CookieCloud 解出的 Cookie 头：{joined}"
        );
        assert!(
            joined.contains("user-agent=Mozilla/5.0"),
            "应带浏览器 UA：{joined}"
        );
    })
    .await;
}

/// Xiaomi 端到端（06-09 逆向接口回归网）：CookieCloud 拉取解密 + /balance 包络
/// → 归一化余额；同时锁「业务包络失效码 code=401 → 401 提示重新同步」新语义。
#[tokio::test]
async fn xiaomi_usage_success_and_envelope_auth_failure() {
    use axum::http::HeaderMap;
    use axum::response::IntoResponse;
    use axum::routing::get;

    // 与 AgentRouter 用例同一 openssl 预生成载荷（uuid=ar-test-uuid-0001 /
    // password=ar-test-pass，内层 cookie_data 键为 .agentrouter.org）——解密只需
    // uuid/password，domain 只做 cookie 过滤，故此处 domain 也填 agentrouter.org
    //（复用的是同一份密文，不影响本用例要验的余额包络分支）。
    const CLOUD_ENCRYPTED: &str = "U2FsdGVkX1/Xcm7x191hHmE3HGxu7lVc4mBt2U032Ptv7ArbR+PvVOFZz5nbeLdbrjlZ683ZKmstnvcmRanGZPr0nrwuPoV5ajlUtbdPIZU9SSNGAuG1pDxsRDVtYa4pyTZM3HqTbkFt8oJw5VvqY0HHxu/ZxfPImH3rdnFJC5wf8Inv5lvj9xB+b0bMtrmhG0TumnlPu16TRJcNxgnH+/bPC+LzVwL9+mIfgWe+O5A=";
    const CLOUD_UUID: &str = "ar-test-uuid-0001";
    const CLOUD_PASSWORD: &str = "ar-test-pass";

    let seen_cookie = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let cookie_sink = seen_cookie.clone();
    // 由测试驱动的余额响应体（成功包络 / 失效包络 两形态）。
    let balance_body = Arc::new(std::sync::Mutex::new(
        serde_json::json!({
            "code": 0,
            "data": {"currency": "CNY", "balance": 12.34, "giftBalance": 1.0}
        })
        .to_string(),
    ));
    let body_sink = balance_body.clone();

    let app = axum::Router::new()
        .route(
            &format!("/get/{CLOUD_UUID}"),
            get(|| async move { Json(serde_json::json!({ "encrypted": CLOUD_ENCRYPTED })) }),
        )
        .route(
            "/api/v1/balance",
            get(move |headers: HeaderMap| {
                let sink = cookie_sink.clone();
                let body = body_sink.clone();
                async move {
                    if let Some(cookie) = headers.get("cookie").and_then(|v| v.to_str().ok()) {
                        sink.lock().unwrap().push(cookie.to_string());
                    }
                    let text = body.lock().unwrap().clone();
                    (StatusCode::OK, [("content-type", "application/json")], text).into_response()
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let mock_base = format!("http://{addr}");

    temp_env::async_with_vars([(OVERRIDE_ENV, Some(mock_base.as_str()))], async {
        let app = setup_app().await;
        let extra = format!(
            r#"{{"usage": true, "usage_type": 0, "cookie_cloud_server": "https://cc.example", "uuid": "{CLOUD_UUID}", "password": "{CLOUD_PASSWORD}", "domain": "agentrouter.org"}}"#
        );
        let id = create_provider(
            &app,
            "Xiaomi-用量",
            "https://api.xiaomimimo.com/v1",
            &extra,
        )
        .await;

        // 成功形态：包络 code=0 → 余额归一（balance 为 primary）。
        let (status, body) = send(
            &app,
            "GET",
            &format!("/api/providers/{id}/usage?refresh=1"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "查询失败：{body}");
        let data = &body["data"];
        assert_eq!(data["kind"], "balance");
        let balances = data["balances"].as_array().unwrap();
        assert_eq!(balances[0]["label"], "余额");
        assert_eq!(balances[0]["amount"], 12.34);
        assert_eq!(balances[0]["primary"].as_bool(), Some(true));

        // 失效形态：包络 code=401 → 400 提示凭据过期（归位一治理新语义）。
        *balance_body.lock().unwrap() =
            serde_json::json!({"code": 401, "message": "登录态已失效"}).to_string();
        let (status, body) = send(
            &app,
            "GET",
            &format!("/api/providers/{id}/usage?refresh=1"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "应归 400 凭据类：{body}");
        assert!(
            body["msg"].as_str().unwrap_or("").contains("凭据"),
            "文案应提示凭据无效/过期：{body}"
        );
    })
    .await;
}

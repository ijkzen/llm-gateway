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

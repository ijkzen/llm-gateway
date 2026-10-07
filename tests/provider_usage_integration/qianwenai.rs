use super::*;

/// CookieCloud 载荷由 openssl 预生成（md5(uuid-password) 前 16 字符作 passphrase，
/// 见 cookiecloud.rs 测试向量说明），cookie 域为 `.qianwenai.com`。
const CLOUD_UUID: &str = "qw-test-uuid-0001";
const CLOUD_PASSWORD: &str = "qw-test-pass";
const CLOUD_ENCRYPTED: &str = "U2FsdGVkX18Ucmw6+14FBzatvNP8JKNvFEt9dtiaSvhwyPEQP2sgDJW9g4LyOe+McWVlQGx4r4wElwDr6c7FAo3i/tEYTd6xHXyofisYOzBftMe+5JZG+iQls98lgwFkQqgvV6MTLdeuVUlQqbKlbnEl/X/JHHrCq9b+XwJOY8xTJwbz5TmFt8gJoSZZgoqyylKUTwYgZmz13znIl1O/YMNemSOvXLUXjnK6F6wZF34wc82Q4WWUQOAnWdDhdjk1";

/// 千问 Token Plan 端到端：CookieCloud 解出千问控制台登录态 → secToken →
/// 用量 RPC（当前控制台下发月窗口，0–1 已用比例）→ 归一化为订阅制窗口。
///
/// mock 同时提供 CookieCloud `/get/{uuid}`、`/tool/user/info.json` 与
/// `/data/api.json`；CookieCloud 载荷由 openssl 预生成（md5(uuid-password)
/// 前 16 字符作 passphrase），断言出站 query/form/头（sec_token、region、
/// cornerstoneParam、Origin/Referer）。
#[tokio::test]
async fn qianwenai_usage_fetches_monthly_quota_window() {
    use axum::http::{HeaderMap, Uri};
    use axum::response::IntoResponse;
    use axum::routing::{get, post};

    let seen = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let seen_info = seen.clone();
    let seen_usage = seen.clone();
    let app = axum::Router::new()
        .route(
            &format!("/get/{CLOUD_UUID}"),
            get(|| async move {
                Json(serde_json::json!({ "encrypted": CLOUD_ENCRYPTED })).into_response()
            }),
        )
        .route(
            "/tool/user/info.json",
            get(move |headers: HeaderMap| {
                let seen = seen_info.clone();
                async move {
                    for name in ["cookie", "referer"] {
                        if let Some(v) = headers.get(name).and_then(|v| v.to_str().ok()) {
                            seen.lock().unwrap().push(format!("info.{name}={v}"));
                        }
                    }
                    Json(serde_json::json!({
                        "code": "200",
                        "data": { "secToken": "sec-token-xyz" }
                    }))
                    .into_response()
                }
            }),
        )
        .route(
            "/data/api.json",
            post(move |headers: HeaderMap, uri: Uri, body: String| {
                let seen = seen_usage.clone();
                async move {
                    {
                        let mut seen = seen.lock().unwrap();
                        seen.push(format!("usage.query={}", uri.query().unwrap_or_default()));
                        seen.push(format!("usage.form={body}"));
                        for name in ["cookie", "origin", "referer"] {
                            if let Some(v) = headers.get(name).and_then(|v| v.to_str().ok()) {
                                seen.push(format!("usage.{name}={v}"));
                            }
                        }
                    }
                    Json(serde_json::json!({
                        "code": "200",
                        "successResponse": true,
                        "data": {
                            "success": true, "errorCode": "", "errorMsg": "",
                            "api": "zeldaHttp.apikeyMgr./tokenplan/personal/api/v2/usage",
                            "DataV2": {
                                "ret": ["SUCCESS::接口调用成功"],
                                "data": {
                                    "msg": "Success.", "code": "SUCCESS", "success": true,
                                    "data": {
                                        "per1MonthPercentage": 0.25,
                                        "per1MonthResetTime": 1794067200000i64
                                    }
                                }
                            }
                        }
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
        let extra = format!(
            r#"{{"usage": true, "usage_type": 1, "cookie_cloud_server": "https://cc.example", "uuid": "{CLOUD_UUID}", "password": "{CLOUD_PASSWORD}", "domain": "qianwenai.com"}}"#
        );
        let id = create_provider_with_billing(
            &app,
            "Qianwen-用量",
            "https://token-plan.maas.qianwenaiapi.com/compatible-mode/v1",
            &extra,
            1,
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
        assert_eq!(data["kind"], "quota");
        let windows = data["windows"].as_array().unwrap();
        assert_eq!(windows.len(), 3, "固定 5h/周/月三槽位：{data}");
        assert_eq!(windows[0]["available"].as_bool(), Some(false));
        assert_eq!(windows[1]["available"].as_bool(), Some(false));
        assert_eq!(windows[2]["window"], "monthly");
        assert_eq!(windows[2]["available"].as_bool(), Some(true));
        assert_eq!(windows[2]["usedPercent"], 25.0);
        assert_eq!(windows[2]["remainingPercent"], 75.0);
        assert!(
            !windows[2]["resetsAt"].is_null(),
            "月窗口应带重置时间：{data}"
        );

        let seen = seen.lock().unwrap();
        let joined = seen.join("\n");
        // CookieCloud 解出的登录态 Cookie 用于两段请求。
        assert!(
            joined.contains("info.cookie=login_qianwenai_ticket=qw-ticket-abc"),
            "info 请求应带 CookieCloud 解出的 Cookie：{joined}"
        );
        assert!(
            joined.contains("usage.cookie=login_qianwenai_ticket=qw-ticket-abc"),
            "用量请求应带同一 Cookie：{joined}"
        );
        // query：product/action/api/_t。
        assert!(
            joined.contains("action=BroadScopeAspnGateway") && joined.contains("product=sfm_bailian"),
            "query 应含网关动作与产品：{joined}"
        );
        assert!(
            joined.contains("api=zeldaHttp.apikeyMgr.%2Ftokenplan%2Fpersonal%2Fapi%2Fv2%2Fusage"),
            "query 应含 percent 编码的业务 API：{joined}"
        );
        // form：sec_token / region / params（含 cornerstoneParam 六字段）。
        assert!(
            joined.contains("sec_token=sec-token-xyz"),
            "form 应带 secToken：{joined}"
        );
        assert!(
            joined.contains("region=cn-beijing"),
            "form 应带 region：{joined}"
        );
        assert!(
            joined.contains("QIANWENAI") && joined.contains("p_efm"),
            "params 应含 consoleSite/productCode：{joined}"
        );
        // 控制台来源头。
        assert!(
            joined.contains("usage.origin=https://platform.qianwenai.com"),
            "用量请求应带 Origin：{joined}"
        );
        assert!(
            joined.contains("usage.referer=https://platform.qianwenai.com/home/billing/subscription/token-plan-individual"),
            "用量请求应带 Referer：{joined}"
        );
    })
    .await;
}

/// 登录态失效（HTTP 200 + 体内标记，非传输状态码）→ 400 类「凭据无效或已过期」。
#[tokio::test]
async fn qianwenai_usage_maps_expired_console_session_to_auth_error() {
    use axum::response::IntoResponse;
    use axum::routing::{get, post};

    let app = axum::Router::new()
        .route(
            &format!("/get/{CLOUD_UUID}"),
            get(|| async move {
                Json(serde_json::json!({ "encrypted": CLOUD_ENCRYPTED })).into_response()
            }),
        )
        .route(
            "/tool/user/info.json",
            get(|| async {
                // 未登录：HTTP 200 + 体内标记，无 secToken。
                Json(serde_json::json!({
                    "code": "ConsoleNeedLogin",
                    "message": "ConsoleNeedLogin"
                }))
                .into_response()
            }),
        )
        .route(
            "/data/api.json",
            post(|| async { Json(serde_json::json!({})).into_response() }),
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
            r#"{{"usage": true, "usage_type": 1, "cookie_cloud_server": "https://cc.example", "uuid": "{CLOUD_UUID}", "password": "{CLOUD_PASSWORD}", "domain": "qianwenai.com"}}"#
        );
        let id = create_provider_with_billing(
            &app,
            "Qianwen-未登录",
            "https://token-plan.maas.qianwenaiapi.com/compatible-mode/v1",
            &extra,
            1,
        )
        .await;

        let (status, body) = send(
            &app,
            "GET",
            &format!("/api/providers/{id}/usage?refresh=1"),
            None,
        )
        .await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "登录失效应归 400 类：{body}"
        );
        assert!(
            body.to_string().contains("凭据无效或已过期"),
            "错误信息应为凭据失效文案：{body}"
        );
    })
    .await;
}

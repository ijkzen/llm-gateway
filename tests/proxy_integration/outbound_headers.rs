use super::*;

#[tokio::test]
async fn trace_headers_are_forwarded_verbatim_and_credentials_stripped() {
    let captured_headers = capture_headers();
    let base = spawn_mock_with_headers(capture(), captured_headers.clone()).await;
    let (app, _db) = common_setup_with_member(&base, 0, 0, 0).await;

    let (status, text) = send_chat_with_headers(
        &app,
        chat_body("vm-x", false),
        &[
            (
                "traceparent",
                "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
            ),
            ("tracestate", "vendor=abc"),
            // 非 allowlist / 凭据 / 框架 / hop-by-hop：一律不进上游。
            ("x-trace-id", "client-1"),
            ("cookie", "session=abc"),
            ("host", "evil.example"),
            ("connection", "keep-alive"),
        ],
    )
    .await;
    assert_eq!(status, 200, "{text}");

    let headers = captured_headers.lock().unwrap();
    let upstream = headers
        .iter()
        .find(|h| h.contains_key("traceparent"))
        .unwrap_or_else(|| panic!("应至少有一个上游请求头快照：{headers:?}"));
    assert_eq!(
        header_value(upstream, "traceparent"),
        Some("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01")
    );
    assert_eq!(header_value(upstream, "tracestate"), Some("vendor=abc"));
    // 鉴权头恒为网关生成的 provider key（sk-mock → Bearer sk-mock）。
    assert_eq!(
        header_value(upstream, "authorization"),
        Some("Bearer sk-mock")
    );
    // 凭据 / 框架 / hop-by-hop 不出站。
    assert!(header_value(upstream, "cookie").is_none());
    assert!(
        header_value(upstream, "x-trace-id").is_none(),
        "x-trace-id 不在 allowlist，不应透传"
    );
    // Host 应为上游 base_url 的 authority（mock 是非默认端口，故含端口）。
    let mock_authority = base.trim_start_matches("http://").to_string();
    assert_eq!(
        header_value(upstream, "host"),
        Some(mock_authority.as_str()),
        "Host 应为上游 base_url authority，而非下游 Host"
    );
    assert!(
        header_value(upstream, "connection").is_none(),
        "connection 不应透传"
    );
    // 无重复 authorization/content-type。
    assert_eq!(header_count(upstream, "authorization"), 1);
    assert_eq!(header_count(upstream, "content-type"), 1);
    assert_eq!(header_count(upstream, "content-length"), 1);
}

#[tokio::test]
async fn client_opencode_session_header_is_forwarded_verbatim() {
    let captured_headers = capture_headers();
    let base = spawn_mock_with_headers(capture(), captured_headers.clone()).await;
    // mock 上游非 opencode host：仅验证 allowlist 原值透传，不触发回退注入。
    let (app, _db) = common_setup_with_member(&base, 0, 0, 0).await;

    let (status, text) = send_chat_with_headers(
        &app,
        chat_body("vm-x", false),
        &[("x-opencode-session", "client-session-1")],
    )
    .await;
    assert_eq!(status, 200, "{text}");

    let headers = captured_headers.lock().unwrap();
    let upstream = headers.first().expect("应有上游头快照");
    assert_eq!(
        header_value(upstream, "x-opencode-session"),
        Some("client-session-1")
    );
    assert_eq!(header_count(upstream, "x-opencode-session"), 1);
}

#[tokio::test]
async fn non_opencode_upstream_gets_no_session_fallback() {
    let captured_headers = capture_headers();
    let base = spawn_mock_with_headers(capture(), captured_headers.clone()).await;
    let (app, _db) = common_setup_with_member(&base, 0, 0, 0).await;

    // 下游未带 x-opencode-session 且上游非 opencode host：不得注入回退值。
    let (status, text) = send_chat_with_headers(&app, chat_body("vm-x", false), &[]).await;
    assert_eq!(status, 200, "{text}");

    let headers = captured_headers.lock().unwrap();
    let upstream = headers.first().expect("应有上游头快照");
    assert!(header_value(upstream, "x-opencode-session").is_none());
}

#[tokio::test]
async fn downstream_user_agent_is_forwarded_verbatim() {
    let captured_headers = capture_headers();
    let base = spawn_mock_with_headers(capture(), captured_headers.clone()).await;
    let (app, _db) = common_setup_with_member(&base, 0, 0, 0).await;

    let (status, text) = send_chat_with_headers(
        &app,
        chat_body("vm-x", false),
        &[("user-agent", "zcode/1.2.3")],
    )
    .await;
    assert_eq!(status, 200, "{text}");

    let headers = captured_headers.lock().unwrap();
    let upstream = headers.first().expect("应有上游头快照");
    assert_eq!(header_value(upstream, "user-agent"), Some("zcode/1.2.3"));
    assert_eq!(header_count(upstream, "user-agent"), 1);
}

#[tokio::test]
async fn downstream_allowlist_setting_controls_forwarding() {
    let captured_headers = capture_headers();
    let base = spawn_mock_with_headers(capture(), captured_headers.clone()).await;
    let (app, db) = common_setup_with_member(&base, 0, 0, 0).await;

    // 种子 allowlist 设置行（Json 类型），再经设置接口热更新为仅 user-agent。
    setting::ActiveModel {
        key: Set(llm_gateway::app_settings::KEY_DOWNSTREAM_REQUEST_HEADER_ALLOW_LIST.to_string()),
        value: Set(
            llm_gateway::app_settings::DEFAULT_DOWNSTREAM_REQUEST_HEADER_ALLOW_LIST.to_string(),
        ),
        r#type: Set(4),
        updated_at: Set(chrono::Utc::now()),
    }
    .insert(&db)
    .await
    .unwrap();
    let put = Request::builder()
        .method("PUT")
        .uri("/api/settings/downstream_request_header_allow_list")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"value":"[\"user-agent\"]"}"#))
        .unwrap();
    let response = app.clone().oneshot(put).await.unwrap();
    assert_eq!(response.status().as_u16(), 200);

    let (status, text) = send_chat_with_headers(
        &app,
        chat_body("vm-x", false),
        &[
            (
                "traceparent",
                "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
            ),
            ("user-agent", "zcode/1.2.3"),
        ],
    )
    .await;
    assert_eq!(status, 200, "{text}");

    let headers = captured_headers.lock().unwrap();
    let upstream = headers.first().expect("应有上游头快照");
    assert_eq!(header_value(upstream, "user-agent"), Some("zcode/1.2.3"));
    assert!(
        header_value(upstream, "traceparent").is_none(),
        "allowlist 更新后 traceparent 不应再透传"
    );
}

#[tokio::test]
async fn downstream_authorization_never_reaches_upstream() {
    let captured_headers = capture_headers();
    let base = spawn_mock_with_headers(capture(), captured_headers.clone()).await;
    let (app, _db) = common_setup_with_member(&base, 0, 0, 0).await;

    // 即便下游 Authorization 是攻击者注入的任意 Bearer，上游也只看到网关生成的 key。
    let builder = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        // 合法网关 key 鉴权通过。
        .header("authorization", TEST_BEARER);
    let request = builder
        .body(Body::from(chat_body("vm-x", false).to_string()))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status().as_u16(), 200);

    let headers = captured_headers.lock().unwrap();
    let upstream = headers.first().expect("应有上游头快照");
    let auth = header_value(upstream, "authorization").unwrap_or("");
    assert!(!auth.contains("lg-itest"), "上游不得含下游网关 key：{auth}");
    assert_eq!(auth, "Bearer sk-mock");
    assert_eq!(header_count(upstream, "authorization"), 1);
}

#[tokio::test]
async fn custom_header_cannot_override_protocol_auth_and_is_single_valued() {
    let captured_headers = capture_headers();
    let base = spawn_mock_with_headers(capture(), captured_headers.clone()).await;
    // Anthropic 协议（protocol_type=2）：custom_header 携带同名 x-api-key/anthropic-version
    // 与保留名 content-type/authorization，均不得覆盖网关生成值。
    let (app, _db) = setup_member_with_custom_header(
        &base,
        2,
        r#"{"x-api-key":"custom","anthropic-version":"2099-01-01","authorization":"Bearer custom","content-type":"text/plain","X-A":"b"}"#,
    )
    .await;

    let (status, text) = send_chat(&app, chat_body("vm-x", false)).await;
    assert_eq!(status, 200, "{text}");

    let headers = captured_headers.lock().unwrap();
    let upstream = headers
        .iter()
        .find(|h| h.contains_key("x-api-key"))
        .unwrap_or_else(|| panic!("Anthropic 上游应有 x-api-key：{headers:?}"));
    assert_eq!(header_value(upstream, "x-api-key"), Some("sk-mock"));
    assert_eq!(
        header_value(upstream, "anthropic-version"),
        Some("2023-06-01")
    );
    // authorization / content-type 不进 Anthropic 上游（无同名重复）。
    assert!(header_value(upstream, "authorization").is_none());
    assert_eq!(
        header_value(upstream, "content-type"),
        Some("application/json")
    );
    assert_eq!(
        header_value(upstream, "x-a"),
        Some("b"),
        "普通自定义头应生效"
    );
    assert_eq!(header_count(upstream, "x-api-key"), 1);
    assert_eq!(header_count(upstream, "anthropic-version"), 1);
    assert_eq!(header_count(upstream, "content-type"), 1);
    assert_eq!(header_count(upstream, "content-length"), 1);
}

#[tokio::test]
async fn custom_header_supplemental_headers_reach_upstream() {
    let captured_headers = capture_headers();
    let base = spawn_mock_with_headers(capture(), captured_headers.clone()).await;
    let (app, _db) =
        setup_member_with_custom_header(&base, 0, r#"{"X-Tenant":"t1","X-Env":"prod"}"#).await;

    let (status, text) = send_chat(&app, chat_body("vm-x", false)).await;
    assert_eq!(status, 200, "{text}");

    let headers = captured_headers.lock().unwrap();
    let upstream = headers
        .iter()
        .find(|h| h.contains_key("x-tenant"))
        .unwrap_or_else(|| panic!("应透传 custom_header：{headers:?}"));
    assert_eq!(header_value(upstream, "x-tenant"), Some("t1"));
    assert_eq!(header_value(upstream, "x-env"), Some("prod"));
    assert_eq!(
        header_value(upstream, "authorization"),
        Some("Bearer sk-mock")
    );
}

use super::*;

fn provider(proxy_enabled: bool, proxy_addr: &str) -> provider::Model {
    let now = chrono::Utc::now();
    provider::Model {
        id: 1,
        name: "p".to_string(),
        enable: true,
        base_url: "https://api.example.com".to_string(),
        api_key: "enc".to_string(),
        custom_header: "{}".to_string(),
        protocol_type: 0,
        billing_mode: 0,
        extra: "{}".to_string(),
        sort_order: 0,
        proxy_enabled,
        proxy_addr: proxy_addr.to_string(),
        disabled_reason: None,
        created_at: now,
        updated_at: now,
    }
}

fn model(proxy_enabled: bool, proxy_addr: &str) -> provider_model::Model {
    let now = chrono::Utc::now();
    provider_model::Model {
        model_id: 1,
        provider_id: 1,
        provider_model_id: "m".to_string(),
        context_length: 1000,
        max_output_tokens: 1000,
        reasoning: false,
        tool_use: false,
        image_understand: false,
        video_understand: false,
        protocol_type: None,
        proxy_enabled,
        proxy_addr: proxy_addr.to_string(),
        created_at: now,
        updated_at: now,
    }
}

#[test]
fn accumulate_chunks_strips_index_from_tool_calls() {
    // 非流式 chat.completion 的 tool_calls 无 index 字段（流式专属）。
    let chunks = vec![
        chunk_json("chatcmpl-1", "vm-a", json!({"role": "assistant"}), None),
        chunk_json(
            "chatcmpl-1",
            "vm-a",
            json!({"tool_calls": [{"index": 0, "id": "call_1", "type": "function", "function": {"name": "f", "arguments": "{\"a\":"}}]}),
            None,
        ),
        chunk_json(
            "chatcmpl-1",
            "vm-a",
            json!({"tool_calls": [{"index": 0, "function": {"arguments": "1}"}}]}),
            None,
        ),
        chunk_json("chatcmpl-1", "vm-a", json!({}), Some("tool_calls")),
    ];
    let completion = accumulate_chunks(&chunks, &Usage::default());
    let call = &completion["choices"][0]["message"]["tool_calls"][0];
    assert_eq!(call["function"]["name"], "f");
    assert_eq!(call["function"]["arguments"], "{\"a\":1}");
    assert!(
        call.get("index").is_none(),
        "非流式 tool_calls 不应带 index"
    );
}

#[test]
fn resolve_proxy_prefers_model_then_provider_then_direct() {
    // 模型开启 → 用模型地址（即使供应商也开着、地址不同）。
    assert_eq!(
        resolve_proxy(
            &model(true, "http://model:1"),
            &provider(true, "http://p:2")
        ),
        (true, "http://model:1".to_string())
    );
    // 模型关、供应商开 → 回落供应商地址。
    assert_eq!(
        resolve_proxy(&model(false, ""), &provider(true, "http://p:2")),
        (true, "http://p:2".to_string())
    );
    // 模型开但地址空白 → 视为未配置，回落供应商。
    assert_eq!(
        resolve_proxy(&model(true, "  "), &provider(true, "http://p:2")),
        (true, "http://p:2".to_string())
    );
    // 两者都关 → 直连。
    assert_eq!(
        resolve_proxy(&model(false, ""), &provider(false, "")),
        (false, String::new())
    );
}

// ─── 上游出站头组装（四层覆盖 + 剥离）单测 ───

fn member(protocol: Protocol, custom_header: &str) -> Member {
    member_with_base_url(protocol, custom_header, "https://api.example.com/v1")
}

fn member_with_base_url(protocol: Protocol, custom_header: &str, base_url: &str) -> Member {
    Member {
        provider_id: 1,
        model_id: "m".to_string(),
        protocol,
        billing_mode: 0,
        base_url: base_url.to_string(),
        api_key_encrypted: "enc".to_string(),
        custom_header: custom_header.to_string(),
        proxy_enabled: false,
        proxy_addr: String::new(),
    }
}

fn hv(value: &str) -> HeaderValue {
    HeaderValue::from_str(value).unwrap()
}

/// `build_upstream_call` 已异步化（Gemini 远程图片下载），测试用
/// current_thread runtime 同步驱动。
fn build_call_sync(
    member: &Member,
    chat: &Value,
    forwarded: &[(HeaderName, HeaderValue)],
    session: &str,
) -> (UpstreamCall, convert::RequestFlags) {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(build_upstream_call(
            member,
            chat,
            false,
            "sk-provider",
            forwarded,
            "test",
            session,
        ))
        .unwrap()
}

fn names(call: &UpstreamCall) -> Vec<String> {
    let mut seen: Vec<String> = call
        .headers
        .iter()
        .map(|(n, _)| n.as_str().to_ascii_lowercase())
        .collect();
    seen.sort();
    seen
}

fn header_map(entries: &[(&str, &str)]) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (k, v) in entries {
        map.insert(HeaderName::from_bytes(k.as_bytes()).unwrap(), hv(v));
    }
    map
}

#[test]
fn is_never_outbound_covers_credentials_framing_and_hop_by_hop() {
    for reserved in [
        "authorization",
        "cookie",
        "proxy-authorization",
        "x-api-key",
        "x-goog-api-key",
        "connection",
        "keep-alive",
        "proxy-connection",
        "transfer-encoding",
        "te",
        "host",
        "content-length",
        "content-type",
        "accept",
        "expect",
        "x-forwarded-for",
        "forwarded",
        "via",
        "x-real-ip",
    ] {
        let name = HeaderName::from_bytes(reserved.as_bytes()).unwrap();
        assert!(is_never_outbound(&name), "{reserved} 应被剥离");
    }
    for allowed in ["traceparent", "tracestate", "x-trace-id", "anthropic-beta"] {
        let name = HeaderName::from_bytes(allowed.as_bytes()).unwrap();
        assert!(!is_never_outbound(&name), "{allowed} 不应被剥离");
    }
}

#[test]
fn select_forwardable_passes_allowlist_and_blocks_blacklist() {
    let map = header_map(&[
        (
            "traceparent",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
        ),
        ("tracestate", "vendor=abc"),
        ("x-trace-id", "client-1"),
        ("authorization", "Bearer lg-secret"),
        ("host", "evil.example"),
    ]);
    let out = select_forwardable_headers(&map, &allowlist(&["traceparent", "tracestate"]));
    let got: Vec<(String, String)> = out
        .iter()
        .map(|(n, v)| (n.as_str().to_string(), v.to_str().unwrap().to_string()))
        .collect();
    // trace 头透传；x-trace-id 不在 allowlist 不透传；凭据/框架头即使被
    // allowlist 点名也不透传（黑名单优先）。
    assert_eq!(
        got,
        vec![
            (
                "traceparent".to_string(),
                "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".to_string()
            ),
            ("tracestate".to_string(), "vendor=abc".to_string()),
        ]
    );

    // allowlist 里点名黑名单项也不会放行。
    let allow_forced = [
        HeaderName::from_static("authorization"),
        HeaderName::from_static("traceparent"),
    ];
    let out2 = select_forwardable_headers(&map, &allow_forced);
    assert_eq!(out2.len(), 1);
    assert_eq!(out2[0].0.as_str(), "traceparent");
}

#[test]
fn custom_header_cannot_override_protocol_auth_or_framing() {
    // Anthropic custom_header 携带 x-api-key / anthropic-version 同名、以及
    // authorization/content-type 保留名：全部被跳过，协议头保留网关值。
    let m = member(
        Protocol::Anthropic,
        r#"{"x-api-key":"custom","anthropic-version":"2099-01-01","authorization":"Bearer custom","content-type":"text/plain","X-A":"b"}"#,
    );
    let chat = json!({"model":"m","messages":[{"role":"user","content":"hi"}]});
    let (call, _) = build_call_sync(&m, &chat, &[], "fb-sess");
    let map: std::collections::HashMap<&str, &str> = call
        .headers
        .iter()
        .map(|(n, v)| (n.as_str(), v.to_str().unwrap()))
        .collect();
    assert_eq!(map.get("x-api-key").copied(), Some("sk-provider"));
    assert_eq!(map.get("anthropic-version").copied(), Some("2023-06-01"));
    assert!(
        !map.contains_key("authorization"),
        "authorization 不进 Anthropic 上游"
    );
    assert!(
        !map.contains_key("content-type"),
        "content-type 由发送端框架头生成"
    );
    assert_eq!(map.get("x-a").copied(), Some("b"), "普通自定义头应生效");
    // 无同名重复。
    assert!(!has_duplicate_names(&call));
}

#[test]
fn openai_compat_auth_header_uses_provider_key_and_drops_custom() {
    let m = member(
        Protocol::OpenAiCompat,
        r#"{"authorization":"Bearer stale","X-Tenant":"t1"}"#,
    );
    let chat = json!({"model":"m","messages":[{"role":"user","content":"hi"}]});
    let (call, _) = build_call_sync(&m, &chat, &[], "fb-sess");
    let map: std::collections::HashMap<&str, &str> = call
        .headers
        .iter()
        .map(|(n, v)| (n.as_str(), v.to_str().unwrap()))
        .collect();
    assert_eq!(
        map.get("authorization").copied(),
        Some("Bearer sk-provider")
    );
    assert_eq!(map.get("x-tenant").copied(), Some("t1"));
    assert!(!has_duplicate_names(&call));
}

#[test]
fn forwarded_headers_beat_custom_header_on_same_name() {
    let m = member(
        Protocol::OpenAiCompat,
        r#"{"traceparent":"custom-tp","X-A":"b"}"#,
    );
    let chat = json!({"model":"m","messages":[{"role":"user","content":"hi"}]});
    let forwarded = vec![
        (
            HeaderName::from_static("traceparent"),
            hv("00-downstream-tp"),
        ),
        (HeaderName::from_static("x-trace-id"), hv("client-1")),
    ];
    let (call, _) = build_call_sync(&m, &chat, &forwarded, "fb-sess");
    let map: std::collections::HashMap<&str, &str> = call
        .headers
        .iter()
        .map(|(n, v)| (n.as_str(), v.to_str().unwrap()))
        .collect();
    // 同名时下游透传值优先，custom_header 只补缺（新合并语义）。
    assert_eq!(map.get("traceparent").copied(), Some("00-downstream-tp"));
    assert_eq!(map.get("x-trace-id").copied(), Some("client-1"));
    assert_eq!(map.get("x-a").copied(), Some("b"));
    assert!(!has_duplicate_names(&call));
}

#[test]
fn gemini_auth_header_is_x_goog_api_key() {
    let m = member(Protocol::Gemini, r#"{"x-goog-api-key":"custom","X-A":"b"}"#);
    let chat = json!({"model":"m","contents":[{"role":"user","parts":[{"text":"hi"}]}]});
    let (call, _) = build_call_sync(&m, &chat, &[], "fb-sess");
    let map: std::collections::HashMap<&str, &str> = call
        .headers
        .iter()
        .map(|(n, v)| (n.as_str(), v.to_str().unwrap()))
        .collect();
    assert_eq!(map.get("x-goog-api-key").copied(), Some("sk-provider"));
    assert_eq!(map.get("x-a").copied(), Some("b"));
    assert!(!has_duplicate_names(&call));
}

#[test]
fn invalid_custom_header_is_ignored() {
    for raw in ["not-json", "[]", r#"{"x":123}"#, r#"{"x":"v","y":1}"#] {
        let m = member(Protocol::OpenAiCompat, raw);
        let chat = json!({"model":"m","messages":[{"role":"user","content":"hi"}]});
        let (call, _) = build_call_sync(&m, &chat, &[], "fb-sess");
        // 非对象 JSON / 非字符串值整体跳过：只剩协议鉴权头。
        let map: std::collections::HashMap<&str, &str> = call
            .headers
            .iter()
            .map(|(n, v)| (n.as_str(), v.to_str().unwrap()))
            .collect();
        assert_eq!(
            map.get("authorization").copied(),
            Some("Bearer sk-provider")
        );
    }
}

#[test]
fn opencode_session_fallback_is_stable_uuid_per_key() {
    let a = opencode_session_fallback("itest-key");
    assert_eq!(
        a,
        opencode_session_fallback("itest-key"),
        "同 Key 派生应稳定"
    );
    assert_ne!(a, opencode_session_fallback("other-key"), "换 Key 应换会话");
    assert!(Uuid::parse_str(&a).is_ok());
}

#[test]
fn opencode_member_injects_session_fallback() {
    let m = member_with_base_url(
        Protocol::OpenAiCompat,
        "{}",
        "https://opencode.ai/zen/go/v1",
    );
    let chat = json!({"model":"m","messages":[{"role":"user","content":"hi"}]});
    let (call, _) = build_call_sync(&m, &chat, &[], "sess-a");
    let map: std::collections::HashMap<&str, &str> = call
        .headers
        .iter()
        .map(|(n, v)| (n.as_str(), v.to_str().unwrap()))
        .collect();
    assert_eq!(map.get("x-opencode-session").copied(), Some("sess-a"));
    assert!(!has_duplicate_names(&call));
}

#[test]
fn opencode_session_client_and_custom_values_win_over_fallback() {
    let chat = json!({"model":"m","messages":[{"role":"user","content":"hi"}]});
    let forwarded = vec![(
        HeaderName::from_static("x-opencode-session"),
        hv("from-client"),
    )];
    // 客户端自带值优先于回退（回退仅在四层组装后仍无该头时注入）。
    let m = member_with_base_url(
        Protocol::OpenAiCompat,
        "{}",
        "https://opencode.ai/zen/go/v1",
    );
    let (call, _) = build_call_sync(&m, &chat, &forwarded, "fb");
    let map: std::collections::HashMap<&str, &str> = call
        .headers
        .iter()
        .map(|(n, v)| (n.as_str(), v.to_str().unwrap()))
        .collect();
    assert_eq!(map.get("x-opencode-session").copied(), Some("from-client"));

    // custom_header（第 3 层）不覆盖客户端透传（同名下游值优先），但仍优先于回退。
    let m2 = member_with_base_url(
        Protocol::OpenAiCompat,
        r#"{"x-opencode-session":"from-custom"}"#,
        "https://opencode.ai/zen/go/v1",
    );
    let (call2, _) = build_call_sync(&m2, &chat, &forwarded, "fb");
    let map2: std::collections::HashMap<&str, &str> = call2
        .headers
        .iter()
        .map(|(n, v)| (n.as_str(), v.to_str().unwrap()))
        .collect();
    assert_eq!(map2.get("x-opencode-session").copied(), Some("from-client"));

    // 无客户端值时 custom_header 仍优先于回退。
    let (call3, _) = build_call_sync(&m2, &chat, &[], "fb");
    let map3: std::collections::HashMap<&str, &str> = call3
        .headers
        .iter()
        .map(|(n, v)| (n.as_str(), v.to_str().unwrap()))
        .collect();
    assert_eq!(map3.get("x-opencode-session").copied(), Some("from-custom"));
}

#[test]
fn non_opencode_member_does_not_inject_session_fallback() {
    let m = member(Protocol::OpenAiCompat, "{}");
    let chat = json!({"model":"m","messages":[{"role":"user","content":"hi"}]});
    let (call, _) = build_call_sync(&m, &chat, &[], "fb-sess");
    let joined = names(&call).join(",");
    assert!(
        !joined.contains("x-opencode-session"),
        "非 opencode 上游不应注入回退会话头：{joined}"
    );
}

#[test]
fn template_default_user_agent_fills_opencode_and_kimi_hosts() {
    let chat = json!({"model":"m","messages":[{"role":"user","content":"hi"}]});
    let user_agent = |base_url: &str| {
        let m = member_with_base_url(Protocol::OpenAiCompat, "{}", base_url);
        let (call, _) = build_call_sync(&m, &chat, &[], "fb");
        let map: std::collections::HashMap<&str, &str> = call
            .headers
            .iter()
            .map(|(n, v)| (n.as_str(), v.to_str().unwrap()))
            .collect();
        map.get("user-agent").copied().unwrap_or("").to_string()
    };
    // OpenCode：pi 同款动态 UA（内核版本随宿主机变化，只断言形状）。
    let opencode_ua = user_agent("https://opencode.ai/zen/go/v1");
    assert!(opencode_ua.starts_with("pi ("), "{opencode_ua}");
    assert!(opencode_ua.ends_with(')'), "{opencode_ua}");
    // Kimi For Coding：官方 kimi-cli 当前版本 UA。
    assert_eq!(
        user_agent("https://api.kimi.com/coding/v1"),
        provider_template::KIMI_CODE_USER_AGENT
    );
    // 非模板 host 不注入默认 UA。
    assert_eq!(user_agent("https://api.example.com/v1"), "");
}

#[test]
fn custom_and_forwarded_user_agent_beat_template_default() {
    let chat = json!({"model":"m","messages":[{"role":"user","content":"hi"}]});
    let m = member_with_base_url(
        Protocol::OpenAiCompat,
        r#"{"User-Agent":"custom/9"}"#,
        "https://opencode.ai/zen/go/v1",
    );
    let (call, _) = build_call_sync(&m, &chat, &[], "fb");
    let map: std::collections::HashMap<&str, &str> = call
        .headers
        .iter()
        .map(|(n, v)| (n.as_str(), v.to_str().unwrap()))
        .collect();
    // custom_header 优先于模板默认头。
    assert_eq!(map.get("user-agent").copied(), Some("custom/9"));

    // 下游透传又优先于 custom_header。
    let forwarded = vec![(HeaderName::from_static("user-agent"), hv("from-client"))];
    let (call2, _) = build_call_sync(&m, &chat, &forwarded, "fb");
    let map2: std::collections::HashMap<&str, &str> = call2
        .headers
        .iter()
        .map(|(n, v)| (n.as_str(), v.to_str().unwrap()))
        .collect();
    assert_eq!(map2.get("user-agent").copied(), Some("from-client"));
    assert!(!has_duplicate_names(&call));
    assert!(!has_duplicate_names(&call2));
}

/// 测试用 allowlist（与设置项 `downstream_request_header_allow_list` 的
/// 解析产物同构）。
fn allowlist(names: &[&'static str]) -> Vec<HeaderName> {
    names.iter().map(|n| HeaderName::from_static(n)).collect()
}

#[test]
fn forward_allowlist_includes_opencode_session() {
    let map = header_map(&[("x-opencode-session", "client-sess"), ("x-other", "v")]);
    let out = select_forwardable_headers(&map, &allowlist(&["traceparent", "x-opencode-session"]));
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].0.as_str(), "x-opencode-session");
    assert_eq!(out[0].1, hv("client-sess"));
}

#[test]
fn forward_allowlist_forwards_downstream_user_agent() {
    let map = header_map(&[("user-agent", "zcode/1.2.3"), ("x-other", "v")]);
    let out = select_forwardable_headers(&map, &allowlist(&["user-agent"]));
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].0.as_str(), "user-agent");
    assert_eq!(out[0].1, hv("zcode/1.2.3"));
}

/// 断言出站头无同名重复。
fn has_duplicate_names(call: &UpstreamCall) -> bool {
    let mut seen = std::collections::HashSet::new();
    call.headers
        .iter()
        .any(|(n, _)| !seen.insert(n.as_str().to_ascii_lowercase()))
}

#[test]
fn never_outbound_headers_never_reach_upstream_call_headers() {
    // 通过 merge_custom_headers 直接验证剥离清单在 custom_header 层生效。
    let m = member(
        Protocol::OpenAiCompat,
        r#"{"connection":"keep-alive","host":"evil","x-custom":"ok"}"#,
    );
    let chat = json!({"model":"m","messages":[{"role":"user","content":"hi"}]});
    let (call, _) = build_call_sync(&m, &chat, &[], "fb-sess");
    let joined = names(&call).join(",");
    assert!(
        !joined.contains("connection"),
        "connection 应被剥离：{joined}"
    );
    assert!(!joined.contains("host"), "host 应被剥离：{joined}");
    assert!(joined.contains("x-custom"), "x-custom 应保留：{joined}");
    assert!(!has_duplicate_names(&call));
}

// ─── 原生透传（build_native_upstream_call）头组装单测 ───

/// 原生透传臂：改写 model、URL 取端点子路径，头组装走同一四层。
fn build_native_sync(
    endpoint: NativeEndpoint,
    member: &Member,
    body: &Value,
    forwarded: &[(HeaderName, HeaderValue)],
    session: &str,
) -> UpstreamCall {
    build_native_upstream_call(
        endpoint,
        member,
        body,
        "sk-provider",
        forwarded,
        "test",
        session,
    )
    .unwrap()
}

#[test]
fn native_call_rewrites_model_and_uses_endpoint_sub_path() {
    let m = member_with_base_url(Protocol::Anthropic, "{}", "https://api.anthropic.com/v1");
    let body = json!({"model":"vm-name","messages":[{"role":"user","content":"hi"}]});
    let call = build_native_sync(NativeEndpoint::AnthropicMessages, &m, &body, &[], "fb-sess");
    assert_eq!(call.url, "https://api.anthropic.com/v1/messages");
    let sent: Value = serde_json::from_slice(&call.body).unwrap();
    assert_eq!(sent["model"], "m", "model 应改写为成员真实模型 ID");
    assert_eq!(sent["messages"][0]["role"], "user", "其余字段原样透传");

    let call = build_native_sync(
        NativeEndpoint::OpenAiResponses,
        &member_with_base_url(Protocol::OpenAiResponses, "{}", "https://api.openai.com/v1"),
        &body,
        &[],
        "fb-sess",
    );
    assert_eq!(call.url, "https://api.openai.com/v1/responses");
}

#[test]
fn native_call_assembles_headers_with_same_four_layers() {
    // 下游透传 + custom_header + 协议鉴权：同名以下游为准，鉴权头覆盖两者。
    let m = member(
        Protocol::Anthropic,
        r#"{"x-api-key":"custom","anthropic-version":"2099-01-01","X-A":"b"}"#,
    );
    let body = json!({"model":"vm-name","messages":[]});
    let forwarded = vec![
        (
            HeaderName::from_static("traceparent"),
            hv("00-downstream-tp"),
        ),
        (HeaderName::from_static("x-api-key"), hv("downstream-key")),
    ];
    let call = build_native_sync(
        NativeEndpoint::AnthropicMessages,
        &m,
        &body,
        &forwarded,
        "fb-sess",
    );
    let map: std::collections::HashMap<&str, &str> = call
        .headers
        .iter()
        .map(|(n, v)| (n.as_str(), v.to_str().unwrap()))
        .collect();
    assert_eq!(map.get("traceparent").copied(), Some("00-downstream-tp"));
    assert_eq!(map.get("x-a").copied(), Some("b"), "自定义头补缺");
    assert_eq!(
        map.get("x-api-key").copied(),
        Some("sk-provider"),
        "协议鉴权头覆盖下游与自定义值"
    );
    assert_eq!(map.get("anthropic-version").copied(), Some("2023-06-01"));
    assert!(!has_duplicate_names(&call));
}

#[test]
fn native_call_injects_opencode_session_for_opencode_host() {
    let m = member_with_base_url(Protocol::OpenAiCompat, "{}", "https://opencode.ai/api/v1");
    let body = json!({"model":"vm-name","messages":[]});
    let call = build_native_sync(NativeEndpoint::OpenAiResponses, &m, &body, &[], "fb-sess");
    let map: std::collections::HashMap<&str, &str> = call
        .headers
        .iter()
        .map(|(n, v)| (n.as_str(), v.to_str().unwrap()))
        .collect();
    assert_eq!(
        map.get("x-opencode-session").copied(),
        Some("fb-sess"),
        "opencode host 缺省注入会话头"
    );

    // 下游已带同名头时不覆盖。
    let forwarded = vec![(
        HeaderName::from_static("x-opencode-session"),
        hv("client-sess"),
    )];
    let call2 = build_native_sync(
        NativeEndpoint::OpenAiResponses,
        &m,
        &body,
        &forwarded,
        "fb-sess",
    );
    let map2: std::collections::HashMap<&str, &str> = call2
        .headers
        .iter()
        .map(|(n, v)| (n.as_str(), v.to_str().unwrap()))
        .collect();
    assert_eq!(map2.get("x-opencode-session").copied(), Some("client-sess"));
    assert!(!has_duplicate_names(&call2));
}

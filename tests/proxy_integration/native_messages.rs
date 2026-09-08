use super::*;

#[tokio::test]
async fn test_messages_passthrough_non_stream() {
    let captured = capture();
    let base = spawn_mock(captured.clone()).await;
    let (app, db) = common_setup_native(&base, 2, 2, "vm-msgs").await;

    let (status, text, content_type) =
        send_native(&app, "/v1/messages", messages_body("vm-msgs", false), &[]).await;
    assert_eq!(status, 200, "{text}");
    assert!(content_type.contains("application/json"));
    // 响应原样（mock 的 Anthropic JSON，不经转换）。
    let parsed: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(parsed["id"], "msg_1");
    assert_eq!(parsed["content"][0]["type"], "text");
    assert_eq!(parsed["stop_reason"], "end_turn");

    // 上游请求体：仅 model 改写为成员真实 ID，其余字段原样保留。
    let upstream = captured.lock().unwrap().last().unwrap().clone();
    assert_eq!(upstream["model"], "m-1");
    assert_eq!(upstream["system"], "be nice");
    assert_eq!(upstream["max_tokens"], 99);
    assert_eq!(upstream["stream"], json!(false));

    // request 表记录用量（input = 10 + cache_read 3 + cache_creation 2）。
    let rows = wait_for_records(&db, 1).await;
    assert_eq!(rows.len(), 1);
    assert!(rows[0].success);
    assert!(!rows[0].stream);
    assert_eq!(rows[0].input_tokens, Some(15));
    assert_eq!(rows[0].input_cache_tokens, 3);
    assert_eq!(rows[0].output_tokens, Some(5));
}

#[tokio::test]
async fn test_messages_passthrough_stream_relay_and_headers() {
    let captured = capture();
    let captured_headers = capture_headers();
    let base = spawn_mock_with_headers(captured.clone(), captured_headers.clone()).await;
    let (app, db) = common_setup_native(&base, 2, 2, "vm-msgs").await;

    let (status, text, content_type) = send_native(
        &app,
        "/v1/messages",
        messages_body("vm-msgs", true),
        &[("anthropic-beta", "prompt-caching-2024")],
    )
    .await;
    assert_eq!(status, 200, "{text}");
    assert!(content_type.contains("text/event-stream"));
    // 原始 SSE 帧直通：事件与 mock 产出逐字节同构（data: 行原样）。
    assert!(text.starts_with("data: {"));
    assert!(text.contains("\"type\":\"message_start\""));
    assert!(text.contains("\"type\":\"content_block_delta\""));
    assert!(text.contains("\"type\":\"message_stop\""));

    // 上游头：feature 头透传、网关注入 x-api-key、客户端鉴权头被剥离。
    let headers = captured_headers.lock().unwrap().last().unwrap().clone();
    assert_eq!(
        headers.get("anthropic-beta").and_then(|v| v.to_str().ok()),
        Some("prompt-caching-2024")
    );
    assert_eq!(
        headers.get("x-api-key").and_then(|v| v.to_str().ok()),
        Some("sk-mock")
    );
    assert!(headers.get("authorization").is_none());

    // request 表记录流式用量。
    let rows = wait_for_records(&db, 1).await;
    assert_eq!(rows.len(), 1);
    assert!(rows[0].stream);
    assert_eq!(rows[0].input_tokens, Some(15));
    assert_eq!(rows[0].output_tokens, Some(5));
}

#[tokio::test]
async fn test_messages_rejects_non_messages_interface() {
    let base = spawn_mock(capture()).await;
    // OpenAI Compatible（0）与 Full Compatible（4）都不被 /v1/messages 接受。
    for interface_type in [0, 4] {
        let (app, _db) = common_setup_native(&base, 2, interface_type, "vm-msgs").await;
        let (status, text, _) =
            send_native(&app, "/v1/messages", messages_body("vm-msgs", false), &[]).await;
        assert_eq!(status, 404, "interface={interface_type} text={text}");
        let parsed: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["type"], "error");
        assert_eq!(parsed["error"]["type"], "not_found_error");
    }
}

#[tokio::test]
async fn test_messages_x_api_key_auth() {
    let captured = capture();
    let base = spawn_mock(captured.clone()).await;
    // 用未注入凭证的 build_app：验证 x-api-key 自身的鉴权行为。
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    scheduler.start().await.unwrap();
    let app = common::build_app(db.clone(), scheduler, log_tx);
    let active = llm_gateway::entity::api_key::ActiveModel {
        name: Set("xkey".to_string()),
        key: Set(llm_gateway::crypto::encrypt(common::TEST_API_KEY_PLAIN)),
        key_hash: Set(Some(llm_gateway::auth::hash_token(
            common::TEST_API_KEY_PLAIN,
        ))),
        enable: Set(true),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    active.insert(&db).await.unwrap();
    seed_native_fixture(&db, &base, 2, 2, "vm-msgs").await;

    let body = messages_body("vm-msgs", false);

    // x-api-key 单凭证可用。
    let request = Request::builder()
        .method("POST")
        .uri("/v1/messages")
        .header("content-type", "application/json")
        .header("x-api-key", common::TEST_API_KEY_PLAIN)
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status().as_u16();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&bytes));

    // 无效凭证 401（Anthropic 错误格式）。
    let request = Request::builder()
        .method("POST")
        .uri("/v1/messages")
        .header("content-type", "application/json")
        .header("x-api-key", "lg-wrong-key")
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status().as_u16();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let parsed: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    assert_eq!(status, 401, "body={parsed}");
    assert_eq!(parsed["type"], "error");
    assert_eq!(parsed["error"]["type"], "authentication_error");
}

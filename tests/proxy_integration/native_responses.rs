use super::*;

#[tokio::test]
async fn test_responses_passthrough_stream() {
    let captured = capture();
    let base = spawn_mock(captured.clone()).await;
    let (app, db) = common_setup_native(&base, 1, 1, "vm-resp").await;

    let (status, text, content_type) =
        send_native(&app, "/v1/responses", responses_body("vm-resp"), &[]).await;
    assert_eq!(status, 200, "{text}");
    assert!(content_type.contains("text/event-stream"));
    // 原始 SSE 帧直通。
    assert!(text.starts_with("data: {"));
    assert!(text.contains("\"type\":\"response.created\""));
    assert!(text.contains("\"type\":\"response.completed\""));

    // 上游请求体：仅 model 改写，input 原样保留。
    let upstream = captured.lock().unwrap().last().unwrap().clone();
    assert_eq!(upstream["model"], "m-1");
    assert_eq!(upstream["input"][0]["content"][0]["text"], "hi");

    // request 表记录 usage（cached_tokens → 缓存命中，reasoning 计入输出）。
    let rows = wait_for_records(&db, 1).await;
    assert_eq!(rows.len(), 1);
    assert!(rows[0].stream);
    assert_eq!(rows[0].input_tokens, Some(12));
    assert_eq!(rows[0].input_cache_tokens, 5);
    assert_eq!(rows[0].output_tokens, Some(6));
}

#[tokio::test]
async fn test_responses_rejects_non_responses_interface() {
    let base = spawn_mock(capture()).await;
    for interface_type in [0, 2, 4] {
        let (app, _db) = common_setup_native(&base, 1, interface_type, "vm-resp").await;
        let (status, text, _) =
            send_native(&app, "/v1/responses", responses_body("vm-resp"), &[]).await;
        assert_eq!(status, 404, "interface={interface_type} text={text}");
        let parsed: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["error"]["type"], "invalid_request_error");
    }
}

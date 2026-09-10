use super::*;

#[tokio::test]
async fn reasoning_exclude_strips_thinking_from_response() {
    let base = spawn_mock(capture()).await;
    let (app, _) = common_setup_with_member(&base, 2, 0, 0).await;

    // Anthropic 非流式：thinking 块照常产出但被剥除，正文不受影响。
    let mut body = chat_body("vm-x", false);
    body["messages"][0]["content"] = json!("think-signature");
    body["reasoning"] = json!({"effort": "high", "exclude": true});
    let (status, text) = send_chat(&app, body).await;
    assert_eq!(status, 200, "{text}");
    let completion: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(completion["choices"][0]["message"]["content"], "你好");
    assert!(
        completion["choices"][0]["message"]
            .get("reasoning_content")
            .is_none()
    );
    assert!(
        completion["choices"][0]["message"]
            .get("reasoning_details")
            .is_none()
    );

    // Responses 流式：reasoning item 的 summary 增量同样被剥除。
    let (app, _) = common_setup_with_member(&base, 1, 0, 0).await;
    let (status, text) = send_chat(
        &app,
        json!({
            "model": "vm-x",
            "stream": true,
            "messages": [{"role": "user", "content": "final-only"}],
            "max_tokens": 128,
            "reasoning": {"exclude": true},
        }),
    )
    .await;
    assert_eq!(status, 200, "{text}");
    assert!(!text.contains("reasoning_content"));
    assert!(!text.contains("reasoning_details"));
    assert!(text.contains("\"content\":\"最终内容\""));
}

// ---------- 用例 ----------

#[tokio::test]
async fn openai_passthrough_non_stream_and_record() {
    let captured = capture();
    let base = spawn_mock(captured).await;
    let (app, db) = common_setup_with_member(&base, 0, 0, 0).await;

    let (status, text) = send_chat(&app, chat_body("vm-x", false)).await;
    assert_eq!(status, 200, "{text}");
    let body: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(body["object"], "chat.completion");
    assert_eq!(body["choices"][0]["message"]["content"], "你好");
    assert_eq!(body["usage"]["prompt_tokens"], 10);

    let rows = wait_for_records(&db, 1).await;
    assert_eq!(rows.len(), 1);
    let record = &rows[0];
    assert_eq!(record.success, true);
    assert_eq!(record.stream, false);
    assert_eq!(record.input_tokens, Some(10));
    assert_eq!(record.output_tokens, Some(5));
    assert_eq!(record.input_cache_tokens, 4);
    assert_eq!(record.total_tokens, Some(15));
    assert_eq!(record.api_key_name, "itest-key");
    assert_eq!(record.request_time, record.end_time - record.start_time);
    assert!(record.fail_reason.is_none());
}

#[tokio::test]
async fn openai_passthrough_stream_with_usage_injection() {
    let captured = capture();
    let base = spawn_mock(captured.clone()).await;
    let (app, db) = common_setup_with_member(&base, 0, 0, 0).await;

    let (status, text) = send_chat(&app, chat_body("vm-x", true)).await;
    assert_eq!(status, 200, "{text}");
    assert!(text.contains("data: [DONE]"));
    assert!(text.contains("你好"));

    // 上游请求被注入 stream_options.include_usage。
    let upstream_bodies = captured.lock().unwrap();
    assert_eq!(upstream_bodies[0]["stream_options"]["include_usage"], true);

    let rows = wait_for_records(&db, 1).await;
    let record = &rows[0];
    assert_eq!(record.stream, true);
    assert_eq!(record.input_tokens, Some(11));
    assert_eq!(record.output_tokens, Some(3));
    assert!(record.ttft.is_some(), "流式应有 ttft");
}

#[tokio::test]
async fn anthropic_stream_includes_cached_tokens_in_usage_chunk() {
    // usage chunk 与非流式口径一致：缓存命中明细（prompt_tokens_details）必须保留。
    let base = spawn_mock(capture()).await;
    let (app, _) = common_setup_with_member(&base, 2, 0, 0).await;

    let (status, text) = send_chat(
        &app,
        json!({
            "model": "vm-x",
            "stream": true,
            "stream_options": {"include_usage": true},
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 128,
        }),
    )
    .await;
    assert_eq!(status, 200, "{text}");
    // mock 上游：cache_read 3（creation 2 只计入总输入，不算命中）→ cached_tokens 3。
    assert!(
        text.contains(r#""prompt_tokens_details":{"cached_tokens":3}"#),
        "{text}"
    );
    assert!(text.contains("data: [DONE]"));
}

#[tokio::test]
async fn openai_passthrough_stream_hides_usage_chunk_unless_requested() {
    // include_usage 注入只为统计指标；客户端未请求时 usage 尾块（choices 空
    // 数组）必须过滤，不透给客户端（OpenAI 规范语义）。
    let captured = capture();
    let base = spawn_mock(captured.clone()).await;
    let (app, db) = common_setup_with_member(&base, 0, 0, 0).await;

    let (status, text) = send_chat(&app, chat_body("vm-x", true)).await;
    assert_eq!(status, 200, "{text}");
    let data_lines: Vec<&str> = text
        .lines()
        .filter(|line| line.starts_with("data: "))
        .collect();
    assert!(
        data_lines.iter().all(|line| !line.contains("\"usage\"")),
        "未请求 include_usage 时不应透出 usage chunk：{text}"
    );
    assert!(text.contains("data: [DONE]"));

    // 客户端显式请求 include_usage：usage chunk 必须保留。
    let mut body = chat_body("vm-x", true);
    body["stream_options"] = json!({"include_usage": true});
    let (status, text) = send_chat(&app, body).await;
    assert_eq!(status, 200, "{text}");
    assert!(text.contains("\"usage\""), "{text}");

    // 过滤不影响指标统计：两次请求都应记录到 usage。
    let rows = wait_for_records(&db, 2).await;
    for record in &rows {
        assert_eq!(record.input_tokens, Some(11));
        assert_eq!(record.output_tokens, Some(3));
    }
}

#[tokio::test]
async fn anthropic_non_stream_converts_and_merges_cache_tokens() {
    let captured = capture();
    let base = spawn_mock(captured.clone()).await;
    let (app, db) = common_setup_with_member(&base, 2, 0, 0).await;

    let (status, text) = send_chat(&app, chat_body("vm-x", false)).await;
    assert_eq!(status, 200, "{text}");
    let body: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(body["object"], "chat.completion");
    assert_eq!(body["choices"][0]["message"]["content"], "你好");
    assert_eq!(body["choices"][0]["finish_reason"], "stop");
    assert_eq!(body["choices"][0]["native_finish_reason"], "end_turn");
    // 缓存命中明细只算 cache_read（3）；cache_creation（2）不计入命中率口径。
    assert_eq!(body["usage"]["prompt_tokens_details"]["cached_tokens"], 3);

    // 上游请求为 Anthropic 形状。
    let upstream_bodies = captured.lock().unwrap();
    assert_eq!(upstream_bodies[0]["max_tokens"], 128);
    assert_eq!(upstream_bodies[0]["messages"][0]["role"], "user");

    let rows = wait_for_records(&db, 1).await;
    let record = &rows[0];
    // input = 10 + cache_read 3 + cache_creation 2 = 15（含缓存总输入）。
    assert_eq!(record.input_tokens, Some(15));
    assert_eq!(record.input_cache_tokens, 3);
    assert_eq!(record.output_tokens, Some(5));
}

#[tokio::test]
async fn anthropic_stream_convert_failure_records_failure() {
    // 回归：流中畸形事件（parse 失败）必须按失败落库——曾与 Responses 臂
    // 口径不一致：Anthropic/Gemini 臂漏记转换错误而记假成功。
    let base = spawn_mock(capture()).await;
    let (app, db) = common_setup_with_member(&base, 2, 0, 0).await;

    let (status, text) = send_chat(
        &app,
        json!({
            "model": "vm-x",
            "stream": true,
            "messages": [{"role": "user", "content": "malformed-stream"}],
        }),
    )
    .await;
    assert_eq!(status, 200, "{text}");
    assert!(text.contains("api_error"), "应发 error 帧: {text}");
    assert!(text.contains("data: [DONE]"), "{text}");

    let rows = wait_for_records(&db, 1).await;
    let record = &rows[0];
    assert_eq!(record.stream, true);
    assert!(!record.success, "转换失败必须记失败，不得假成功");
    assert!(
        record
            .fail_reason
            .as_deref()
            .unwrap_or_default()
            .contains("解析"),
        "fail_reason={:?}",
        record.fail_reason
    );
}

#[tokio::test]
async fn anthropic_stream_inband_error_sends_error_frame_and_records_failure() {
    // 03-01 回归：200 SSE 流内的错误事件（Anthropic `error`）此前只置转换器
    // error 态、不产错误帧——客户端只收到 [DONE]，把截断内容当完整成功。
    let base = spawn_mock(capture()).await;
    let (app, db) = common_setup_with_member(&base, 2, 0, 0).await;

    let (status, text) = send_chat(
        &app,
        json!({
            "model": "vm-x",
            "stream": true,
            "messages": [{"role": "user", "content": "inband-error"}],
        }),
    )
    .await;
    assert_eq!(status, 200, "{text}");
    assert!(
        text.contains("api_error"),
        "带内错误必须补发 error 帧（不得只发 [DONE]）: {text}"
    );
    assert!(
        text.contains("上游过载"),
        "error 帧应带上游错误信息: {text}"
    );
    assert!(text.contains("data: [DONE]"), "{text}");

    let rows = wait_for_records(&db, 1).await;
    let record = &rows[0];
    assert_eq!(record.stream, true);
    assert!(!record.success, "带内错误必须记失败，不得假成功");
}

#[tokio::test]
async fn anthropic_stream_converts_to_openai_chunks() {
    let base = spawn_mock(capture()).await;
    let (app, db) = common_setup_with_member(&base, 2, 0, 0).await;

    let (status, text) = send_chat(&app, chat_body("vm-x", true)).await;
    assert_eq!(status, 200, "{text}");
    assert!(text.contains("\"role\":\"assistant\""));
    assert!(text.contains("你好"));
    assert!(text.contains("\"finish_reason\":\"stop\""));
    assert!(text.contains(r#""native_finish_reason":"end_turn""#));
    assert!(text.contains("data: [DONE]"));

    let rows = wait_for_records(&db, 1).await;
    let record = &rows[0];
    assert_eq!(record.stream, true);
    assert_eq!(record.input_tokens, Some(15));
    assert!(record.ttft.is_some());
}

#[tokio::test]
async fn responses_final_output_recovers_non_stream_and_stream_content() {
    let base = spawn_mock(capture()).await;
    let (app, _) = common_setup_with_member(&base, 1, 0, 0).await;
    let body = json!({
        "model": "vm-x",
        "messages": [{"role": "user", "content": "final-only"}],
        "max_tokens": 128,
    });

    let (status, text) = send_chat(&app, body.clone()).await;
    assert_eq!(status, 200, "{text}");
    let completion: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(completion["choices"][0]["message"]["content"], "最终内容");
    assert_eq!(
        completion["choices"][0]["message"]["reasoning_content"],
        "最终推理"
    );

    let (status, text) = send_chat(
        &app,
        json!({
            "model": "vm-x",
            "stream": true,
            "messages": [{"role": "user", "content": "final-only"}],
            "max_tokens": 128,
        }),
    )
    .await;
    assert_eq!(status, 200, "{text}");
    assert_eq!(text.matches(r#""content":"最终内容""#).count(), 1);
    assert_eq!(text.matches(r#""reasoning_content":"最终推理""#).count(), 1);
}

#[tokio::test]
async fn responses_forced_stream_aggregates_for_non_stream_client() {
    let captured = capture();
    let base = spawn_mock(captured.clone()).await;
    let (app, db) = common_setup_with_member(&base, 1, 0, 0).await;

    let (status, text) = send_chat(&app, chat_body("vm-x", false)).await;
    assert_eq!(status, 200, "{text}");
    let body: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(body["choices"][0]["message"]["content"], "你好");
    assert_eq!(body["usage"]["prompt_tokens"], 12);
    assert_eq!(body["usage"]["prompt_tokens_details"]["cached_tokens"], 5);

    // 上游被强制 stream: true，且 max_tokens → max_output_tokens。
    let upstream_bodies = captured.lock().unwrap();
    assert_eq!(upstream_bodies[0]["stream"], true);
    assert_eq!(upstream_bodies[0]["max_output_tokens"], 128);
    assert!(upstream_bodies[0].get("max_tokens").is_none());

    let rows = wait_for_records(&db, 1).await;
    let record = &rows[0];
    assert_eq!(record.input_tokens, Some(12));
    assert_eq!(record.input_cache_tokens, 5);
    assert_eq!(record.output_tokens, Some(6));
}

#[tokio::test]
async fn responses_stream_includes_cached_tokens_in_usage_chunk() {
    let base = spawn_mock(capture()).await;
    let (app, _) = common_setup_with_member(&base, 1, 0, 0).await;

    let (status, text) = send_chat(
        &app,
        json!({
            "model": "vm-x",
            "stream": true,
            "stream_options": {"include_usage": true},
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 128,
        }),
    )
    .await;
    assert_eq!(status, 200, "{text}");
    assert!(text.contains(r#""prompt_tokens_details":{"cached_tokens":5}"#));
    let usage_chunk = text
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter(|data| *data != "[DONE]")
        .map(|data| serde_json::from_str::<Value>(data).unwrap())
        .find(|chunk| chunk["choices"].as_array().is_some_and(Vec::is_empty))
        .unwrap();
    assert_eq!(usage_chunk["id"], "chatcmpl-resp_1");
    // model 保持客户端请求的虚拟模型别名（不跟随上游实际模型名）。
    assert_eq!(usage_chunk["model"], "vm-x");
    assert!(text.contains("data: [DONE]"));
}

#[tokio::test]
async fn gemini_non_stream_converts() {
    let captured = capture();
    let base = spawn_mock(captured.clone()).await;
    let (app, db) = common_setup_with_member(&base, 3, 0, 0).await;

    let (status, text) = send_chat(&app, chat_body("vm-x", false)).await;
    assert_eq!(status, 200, "{text}");
    let body: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(body["choices"][0]["message"]["content"], "你好");
    assert_eq!(body["choices"][0]["finish_reason"], "stop");
    assert_eq!(body["choices"][0]["native_finish_reason"], "STOP");
    assert_eq!(body["usage"]["prompt_tokens_details"]["cached_tokens"], 6);

    let upstream_bodies = captured.lock().unwrap();
    assert_eq!(
        upstream_bodies[0]["generationConfig"]["maxOutputTokens"],
        128
    );
    assert_eq!(upstream_bodies[0]["contents"][0]["role"], "user");

    let rows = wait_for_records(&db, 1).await;
    let record = &rows[0];
    assert_eq!(record.input_tokens, Some(10));
    assert_eq!(record.input_cache_tokens, 6);
    // 输出 = candidates 4 + thoughts 2（含思考）。
    assert_eq!(record.output_tokens, Some(6));
}

#[tokio::test]
async fn anthropic_round_trips_thinking_signature_details() {
    let captured = capture();
    let base = spawn_mock(captured.clone()).await;
    let (app, _) = common_setup_with_member(&base, 2, 0, 0).await;

    // 第一轮：上游 thinking+signature → 下游 reasoning_details 原样透出。
    let (status, text) = send_chat(
        &app,
        json!({
            "model": "vm-x",
            "messages": [{"role": "user", "content": "think-signature"}],
            "max_tokens": 128,
        }),
    )
    .await;
    assert_eq!(status, 200, "{text}");
    let completion: Value = serde_json::from_str(&text).unwrap();
    let details = &completion["choices"][0]["message"]["reasoning_details"];
    assert_eq!(details[0]["type"], "reasoning.text");
    assert_eq!(details[0]["text"], "想一想");
    assert_eq!(details[0]["signature"], "sig-abc");
    assert_eq!(details[0]["format"], "anthropic-claude-v1");

    // 第二轮：客户端回传 details → 上游收到原样 thinking 块，且 tool_use 轮
    // 不再触发禁 thinking 降级。
    let echo = json!({
        "model": "vm-x",
        "messages": [
            {"role": "user", "content": "think-signature"},
            {"role": "assistant", "content": "", "tool_calls": [{"id": "call_1", "type": "function", "function": {"name": "get_weather", "arguments": "{\"city\":\"sf\"}"}}], "reasoning_details": [
                {"type": "reasoning.text", "text": "想一想", "signature": "sig-abc", "id": null, "format": "anthropic-claude-v1", "index": 0}
            ]},
            {"role": "tool", "tool_call_id": "call_1", "content": "sunny"}
        ],
        "reasoning_effort": "high",
        "max_tokens": 4096,
    });
    let (status, text) = send_chat(&app, echo).await;
    assert_eq!(status, 200, "{text}");
    let upstream_bodies = captured.lock().unwrap();
    let second = &upstream_bodies[1];
    assert_eq!(second["thinking"]["type"], "enabled");
    let assistant = second.pointer("/messages/1").unwrap();
    assert_eq!(assistant["content"][0]["type"], "thinking");
    assert_eq!(assistant["content"][0]["thinking"], "想一想");
    assert_eq!(assistant["content"][0]["signature"], "sig-abc");
    assert_eq!(assistant["content"][1]["type"], "tool_use");
}

#[tokio::test]
async fn anthropic_thinking_dropped_sets_response_header() {
    // 工单 08：客户端不回传签名块（ZCode 类客户端）时 thinking 被丢弃，
    // 响应头 x-llm-gateway-thinking-dropped: history 透出降级信号。
    let captured = capture();
    let base = spawn_mock(captured.clone()).await;
    let (app, _) = common_setup_with_member(&base, 2, 0, 0).await;

    let body = json!({
        "model": "vm-x",
        "messages": [
            {"role": "user", "content": "hi"},
            {"role": "assistant", "content": "", "tool_calls": [{"id": "call_1", "type": "function", "function": {"name": "f", "arguments": "{}"}}]},
            {"role": "tool", "tool_call_id": "call_1", "content": "ok"}
        ],
        "reasoning_effort": "high",
        "max_tokens": 4096,
    });
    let request = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .header("authorization", TEST_BEARER)
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(
        response
            .headers()
            .get("x-llm-gateway-thinking-dropped")
            .and_then(|v| v.to_str().ok()),
        Some("history")
    );
    let upstream_bodies = captured.lock().unwrap();
    assert!(upstream_bodies[0].get("thinking").is_none());
}

#[tokio::test]
async fn anthropic_accepts_thinking_toggle_form() {
    // 工单 04：thinking:{type:"enabled"}（ZCode 对未知模型的默认形态）
    // 在 Anthropic 方向归一为 thinking 预算，且正常开启时不带降级标记头。
    let captured = capture();
    let base = spawn_mock(captured.clone()).await;
    let (app, _) = common_setup_with_member(&base, 2, 0, 0).await;

    let body = json!({
        "model": "vm-x",
        "messages": [{"role": "user", "content": "hi"}],
        "thinking": {"type": "enabled"},
        "max_tokens": 4096,
    });
    let request = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .header("authorization", TEST_BEARER)
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status().as_u16(), 200);
    assert!(
        response
            .headers()
            .get("x-llm-gateway-thinking-dropped")
            .is_none()
    );
    let upstream_bodies = captured.lock().unwrap();
    assert_eq!(upstream_bodies[0]["thinking"]["type"], "enabled");
    assert_eq!(upstream_bodies[0]["thinking"]["budget_tokens"], 2048);
}

#[tokio::test]
async fn responses_round_trips_encrypted_reasoning_details() {
    let captured = capture();
    let base = spawn_mock(captured.clone()).await;
    let (app, _) = common_setup_with_member(&base, 1, 0, 0).await;

    // 第一轮：reasoning item（含 encrypted_content）→ 下游 reasoning_details。
    let (status, text) = send_chat(
        &app,
        json!({
            "model": "vm-x",
            "messages": [{"role": "user", "content": "encrypted-only"}],
            "max_tokens": 128,
        }),
    )
    .await;
    assert_eq!(status, 200, "{text}");
    let completion: Value = serde_json::from_str(&text).unwrap();
    let details = &completion["choices"][0]["message"]["reasoning_details"];
    assert_eq!(details[0]["type"], "reasoning.encrypted");
    assert_eq!(details[0]["data"], "gAAA-enc");
    assert_eq!(details[0]["id"], "rs_enc");
    assert_eq!(details[0]["format"], "openai-responses-v1");

    // 第二轮：回传 → reasoning item 注入 function_call 之前 + include 参数。
    let echo = json!({
        "model": "vm-x",
        "messages": [
            {"role": "user", "content": "hi"},
            {"role": "assistant", "content": "", "tool_calls": [{"id": "call_1", "type": "function", "function": {"name": "f", "arguments": "{}"}}], "reasoning_details": [
                {"type": "reasoning.encrypted", "data": "gAAA-enc", "id": "rs_enc", "format": "openai-responses-v1", "index": 0}
            ]},
            {"role": "tool", "tool_call_id": "call_1", "content": "ok"}
        ],
        "max_tokens": 128,
    });
    let (status, text) = send_chat(&app, echo).await;
    assert_eq!(status, 200, "{text}");
    let upstream_bodies = captured.lock().unwrap();
    let second = &upstream_bodies[1];
    assert_eq!(second["include"], json!(["reasoning.encrypted_content"]));
    let input = second["input"].as_array().unwrap();
    // input[0]=user；reasoning 紧贴 function_call 之前。
    assert_eq!(input[1]["type"], "reasoning");
    assert_eq!(input[1]["id"], "rs_enc");
    assert_eq!(input[1]["encrypted_content"], "gAAA-enc");
    assert_eq!(input[2]["type"], "function_call");
    assert_eq!(input[3]["type"], "function_call_output");
}

#[tokio::test]
async fn gemini_round_trips_thought_signature_details() {
    let captured = capture();
    let base = spawn_mock(captured.clone()).await;
    let (app, _) = common_setup_with_member(&base, 3, 0, 0).await;

    // 第一轮：functionCall part 带 thoughtSignature → 下游 reasoning_details。
    let (status, text) = send_chat(
        &app,
        json!({
            "model": "vm-x",
            "messages": [{"role": "user", "content": "tool-call-please"}],
            "max_tokens": 128,
        }),
    )
    .await;
    assert_eq!(status, 200, "{text}");
    let completion: Value = serde_json::from_str(&text).unwrap();
    let details = &completion["choices"][0]["message"]["reasoning_details"];
    assert_eq!(details[0]["type"], "reasoning.encrypted");
    assert_eq!(details[0]["data"], "sig-gem");
    assert_eq!(details[0]["format"], "google-gemini-v1");
    assert_eq!(details[0]["index"], 0);
    let call_id = completion["choices"][0]["message"]["tool_calls"][0]["id"].clone();

    // 第二轮：回传 → thoughtSignature 按 tool_calls 下标挂回 functionCall part。
    let echo = json!({
        "model": "vm-x",
        "messages": [
            {"role": "user", "content": "tool-call-please"},
            {"role": "assistant", "content": "", "tool_calls": [{"id": call_id, "type": "function", "function": {"name": "get_weather", "arguments": "{\"city\":\"sf\"}"}}], "reasoning_details": [
                {"type": "reasoning.encrypted", "data": "sig-gem", "id": null, "format": "google-gemini-v1", "index": 0}
            ]},
            {"role": "tool", "tool_call_id": call_id, "content": "{\"temp\":20}"}
        ],
        "max_tokens": 128,
    });
    let (status, text) = send_chat(&app, echo).await;
    assert_eq!(status, 200, "{text}");
    let upstream_bodies = captured.lock().unwrap();
    let parts = upstream_bodies[1]
        .pointer("/contents/1/parts")
        .unwrap()
        .as_array()
        .unwrap();
    let function_call = parts
        .iter()
        .find(|part| part.get("functionCall").is_some())
        .unwrap();
    assert_eq!(function_call["thoughtSignature"], "sig-gem");
}

#[tokio::test]
async fn cross_format_details_are_dropped_not_forwarded() {
    // anthropic 格式 details 发给 gemini 上游：不注入、请求成功
    // （failover 换供应商时的预期行为，加密签名不互通）。
    let captured = capture();
    let base = spawn_mock(captured.clone()).await;
    let (app, _) = common_setup_with_member(&base, 3, 0, 0).await;
    let (status, text) = send_chat(
        &app,
        json!({
            "model": "vm-x",
            "messages": [
                {"role": "user", "content": "hi"},
                {"role": "assistant", "content": "", "tool_calls": [{"id": "call_1", "type": "function", "function": {"name": "f", "arguments": "{}"}}], "reasoning_details": [
                    {"type": "reasoning.encrypted", "data": "blob", "id": null, "format": "anthropic-claude-v1", "index": 0}
                ]},
                {"role": "tool", "tool_call_id": "call_1", "content": "ok"}
            ],
            "max_tokens": 128,
        }),
    )
    .await;
    assert_eq!(status, 200, "{text}");
    let upstream_bodies = captured.lock().unwrap();
    let parts = upstream_bodies[0]
        .pointer("/contents/1/parts")
        .unwrap()
        .as_array()
        .unwrap();
    assert!(
        parts
            .iter()
            .all(|part| part.get("thoughtSignature").is_none())
    );
}

#[tokio::test]
async fn gemini_stream_includes_cached_tokens_in_usage_chunk() {
    let base = spawn_mock(capture()).await;
    let (app, _) = common_setup_with_member(&base, 3, 0, 0).await;

    let (status, text) = send_chat(
        &app,
        json!({
            "model": "vm-x",
            "stream": true,
            "stream_options": {"include_usage": true},
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 128,
        }),
    )
    .await;
    assert_eq!(status, 200, "{text}");
    assert!(text.contains(r#""prompt_tokens_details":{"cached_tokens":6}"#));
    assert!(text.contains("data: [DONE]"));
}

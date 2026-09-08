
use super::*;
use serde_json::from_str;

#[test]
fn encodes_contents_and_tools() {
    let chat = from_str::<Value>(
            r#"{
                "model": "vm",
                "messages": [
                    {"role": "system", "content": "be nice"},
                    {"role": "user", "content": "hi"},
                    {"role": "assistant", "tool_calls": [{"id": "call_1", "type": "function", "function": {"name": "f", "arguments": "{\"a\":1}"}}]},
                    {"role": "tool", "tool_call_id": "call_1", "content": "{\"ok\":true}"}
                ],
                "tools": [{"type": "function", "function": {"name": "f", "parameters": {"type": "object", "properties": {"a": {"type": "string", "format": "email"}}}}}],
                "max_tokens": 256,
                "stop": ["END"],
                "tool_choice": "required"
            }"#,
        )
        .unwrap();
    let body = build_request_body(&chat, "gemini-x").unwrap();
    assert_eq!(body["systemInstruction"]["parts"][0]["text"], "be nice");
    let contents = body["contents"].as_array().unwrap();
    assert_eq!(contents.len(), 3);
    assert_eq!(contents[0]["role"], "user");
    assert_eq!(contents[1]["role"], "model");
    assert_eq!(contents[1]["parts"][0]["functionCall"]["name"], "f");
    assert_eq!(contents[2]["role"], "user");
    assert_eq!(contents[2]["parts"][0]["functionResponse"]["name"], "f");
    assert_eq!(
        contents[2]["parts"][0]["functionResponse"]["response"]["ok"],
        true
    );
    assert_eq!(body["generationConfig"]["maxOutputTokens"], 256);
    assert_eq!(body["generationConfig"]["stopSequences"], json!(["END"]));
    assert_eq!(body["tools"][0]["functionDeclarations"][0]["name"], "f");
    assert_eq!(
        body["tools"][0]["functionDeclarations"][0]["parameters"]["properties"]["a"]["type"],
        "STRING"
    );
    // email 不是 Gemini 允许的 format，应被移除。
    assert!(
        body["tools"][0]["functionDeclarations"][0]["parameters"]["properties"]["a"]
            .get("format")
            .is_none()
    );
    assert_eq!(body["toolConfig"]["functionCallingConfig"]["mode"], "ANY");
}

#[test]
fn maps_finish_reason_table() {
    assert_eq!(map_finish_reason("STOP", false), ("stop", Some("STOP")));
    assert_eq!(
        map_finish_reason("MAX_TOKENS", false),
        ("length", Some("MAX_TOKENS"))
    );
    assert_eq!(
        map_finish_reason("SAFETY", false),
        ("content_filter", Some("SAFETY"))
    );
    assert_eq!(
        map_finish_reason("RECITATION", false),
        ("content_filter", Some("RECITATION"))
    );
    assert_eq!(
        map_finish_reason("MALFORMED_FUNCTION_CALL", false),
        ("stop", Some("MALFORMED_FUNCTION_CALL"))
    );
    assert_eq!(
        map_finish_reason("STOP", true),
        ("tool_calls", Some("STOP"))
    );
    assert_eq!(
        map_finish_reason("UNEXPECTED_TOOL_CALL", false),
        ("stop", Some("UNEXPECTED_TOOL_CALL"))
    );
    assert_eq!(
        map_finish_reason("NO_IMAGE", false),
        ("stop", Some("NO_IMAGE"))
    );
    assert_eq!(
        map_finish_reason("IMAGE_OTHER", false),
        ("content_filter", Some("IMAGE_OTHER"))
    );
    assert_eq!(
        map_finish_reason("IMAGE_RECITATION", false),
        ("content_filter", Some("IMAGE_RECITATION"))
    );
}

#[test]
fn tool_response_non_object_json_is_wrapped() {
    // 官方要求 functionResponse.response 必须是 JSON 对象；数组/数字等
    // 非对象值必须包装后发送，否则 400。
    let chat = from_str::<Value>(
            r#"{"model":"m","messages":[
                {"role":"user","content":"hi"},
                {"role":"assistant","tool_calls":[{"id":"call_1","type":"function","function":{"name":"f","arguments":"{}"}}]},
                {"role":"tool","tool_call_id":"call_1","content":"[1,2,3]"}
            ]}"#,
        )
        .unwrap();
    let body = build_request_body(&chat, "gemini-x").unwrap();
    let response = &body["contents"][2]["parts"][0]["functionResponse"]["response"];
    assert_eq!(response["result"], json!([1, 2, 3]));

    let chat = from_str::<Value>(
            r#"{"model":"m","messages":[
                {"role":"user","content":"hi"},
                {"role":"assistant","tool_calls":[{"id":"call_1","type":"function","function":{"name":"f","arguments":"{}"}}]},
                {"role":"tool","tool_call_id":"call_1","content":"42"}
            ]}"#,
        )
        .unwrap();
    let body = build_request_body(&chat, "gemini-x").unwrap();
    let response = &body["contents"][2]["parts"][0]["functionResponse"]["response"];
    assert_eq!(response["result"], 42);
}

#[test]
fn tool_call_non_object_arguments_become_empty_object() {
    // functionCall.args 同样必须是对象。
    let chat = from_str::<Value>(
            r#"{"model":"m","messages":[
                {"role":"user","content":"hi"},
                {"role":"assistant","tool_calls":[{"id":"call_1","type":"function","function":{"name":"f","arguments":"\"abc\""}}]}
            ]}"#,
        )
        .unwrap();
    let body = build_request_body(&chat, "gemini-x").unwrap();
    assert_eq!(
        body["contents"][1]["parts"][0]["functionCall"]["args"],
        json!({})
    );
}

#[test]
fn reasoning_object_maps_to_thinking_config() {
    let chat = from_str::<Value>(
        r#"{"model":"m","messages":[{"role":"user","content":"x"}],"reasoning":{"effort":"high"}}"#,
    )
    .unwrap();
    let body = build_request_body(&chat, "gemini-x").unwrap();
    assert_eq!(
        body["generationConfig"]["thinkingConfig"]["thinkingBudget"],
        4096
    );
    assert!(body["generationConfig"].get("reasoning_effort").is_none());
}

#[test]
fn reasoning_max_tokens_maps_to_thinking_budget() {
    let chat = from_str::<Value>(
            r#"{"model":"m","messages":[{"role":"user","content":"x"}],"reasoning":{"max_tokens":2000}}"#,
        )
        .unwrap();
    let body = build_request_body(&chat, "gemini-x").unwrap();
    assert_eq!(
        body["generationConfig"]["thinkingConfig"]["thinkingBudget"],
        2000
    );
}

#[test]
fn top_k_maps_to_generation_config() {
    let chat =
        from_str::<Value>(r#"{"model":"m","messages":[{"role":"user","content":"x"}],"top_k":40}"#)
            .unwrap();
    let body = build_request_body(&chat, "gemini-x").unwrap();
    assert_eq!(body["generationConfig"]["topK"], 40);
}

#[test]
fn reasoning_effort_maps_to_thinking_config() {
    let chat = from_str::<Value>(
        r#"{"model":"m","messages":[{"role":"user","content":"x"}],"reasoning_effort":"high"}"#,
    )
    .unwrap();
    let body = build_request_body(&chat, "gemini-x").unwrap();
    assert_eq!(
        body["generationConfig"]["thinkingConfig"]["thinkingBudget"],
        4096
    );
    assert_eq!(
        body["generationConfig"]["thinkingConfig"]["includeThoughts"],
        true
    );

    // 明确关闭：Gemini 缺省动态思考仍开启，必须显式 thinkingBudget=0。
    let chat = from_str::<Value>(
        r#"{"model":"m","messages":[{"role":"user","content":"x"}],"reasoning_effort":"none"}"#,
    )
    .unwrap();
    let body = build_request_body(&chat, "gemini-x").unwrap();
    assert_eq!(
        body["generationConfig"]["thinkingConfig"],
        json!({"thinkingBudget": 0})
    );

    // 开关形态（ZCode deepseek 家族 off 档）同样归一为关闭。
    let chat = from_str::<Value>(
            r#"{"model":"m","messages":[{"role":"user","content":"x"}],"thinking":{"type":"disabled"}}"#,
        )
        .unwrap();
    let body = build_request_body(&chat, "gemini-x").unwrap();
    assert_eq!(
        body["generationConfig"]["thinkingConfig"],
        json!({"thinkingBudget": 0})
    );

    // 开关开启档：medium 兜底预算。
    let chat = from_str::<Value>(
        r#"{"model":"m","messages":[{"role":"user","content":"x"}],"enable_thinking":true}"#,
    )
    .unwrap();
    let body = build_request_body(&chat, "gemini-x").unwrap();
    assert_eq!(
        body["generationConfig"]["thinkingConfig"],
        json!({"thinkingBudget": 2048, "includeThoughts": true})
    );
}

#[test]
fn penalties_map_into_generation_config() {
    let chat = from_str::<Value>(
            r#"{"model":"m","messages":[{"role":"user","content":"x"}],"presence_penalty":0.3,"frequency_penalty":-0.5}"#,
        )
        .unwrap();
    let body = build_request_body(&chat, "gemini-x").unwrap();
    assert_eq!(body["generationConfig"]["presencePenalty"], 0.3);
    assert_eq!(body["generationConfig"]["frequencyPenalty"], -0.5);
}

#[test]
fn blocked_prompt_maps_to_content_filter() {
    let upstream = from_str::<Value>(r#"{"promptFeedback":{"blockReason":"SAFETY"}}"#).unwrap();
    let (completion, _) = convert_response(&upstream, "req-1", "vm-a").unwrap();
    assert_eq!(completion["choices"][0]["finish_reason"], "content_filter");
}

fn body_with_remote_image(url: &str) -> Value {
    json!({"contents": [{"role": "user", "parts": [
        {"text": "hi"},
        {"fileData": {"fileUri": url}},
    ]}]})
}

#[tokio::test]
async fn remote_image_url_is_downloaded_and_inlined() {
    // Gemini 的 fileData 仅接受 GCS / Files API URI；任意 http(s) 图片 URL
    // 必须下载后转 inlineData，否则上游 400（LiteLLM 同款策略）。
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (mut sock, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 2048];
        let _ = sock.read(&mut buf).await;
        let body = b"\x89PNG-fake-bytes";
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = sock.write_all(head.as_bytes()).await;
        let _ = sock.write_all(body).await;
        let _ = sock.shutdown().await;
    });
    let url = format!("http://{addr}/cat.png");
    let mut body = body_with_remote_image(&url);
    inline_remote_images(&mut body, None, "req-1").await;
    server.await.unwrap();
    let part = &body["contents"][0]["parts"][1];
    assert_eq!(part["inlineData"]["mimeType"], "image/png");
    assert_eq!(
        part["inlineData"]["data"],
        base64::engine::general_purpose::STANDARD.encode(b"\x89PNG-fake-bytes")
    );
}

#[tokio::test]
async fn remote_image_failure_drops_part() {
    let mut body = body_with_remote_image("http://127.0.0.1:1/cat.png");
    inline_remote_images(&mut body, None, "req-1").await;
    let parts = body["contents"][0]["parts"].as_array().unwrap();
    assert!(parts.iter().all(|part| part.get("fileData").is_none()));
    assert_eq!(parts[0]["text"], "hi");
}

#[tokio::test]
async fn gs_and_files_api_uris_are_untouched() {
    let mut body = json!({"contents": [{"role": "user", "parts": [
        {"fileData": {"fileUri": "gs://bucket/cat.png"}},
        {"fileData": {"fileUri": "https://generativelanguage.googleapis.com/v1beta/files/abc"}},
    ]}]});
    let before = body.clone();
    inline_remote_images(&mut body, None, "req-1").await;
    assert_eq!(body, before);
}

#[test]
fn stream_block_reason_sets_content_filter_finish() {
    let mut converter = GeminiStreamConverter::new("req-1", "vm-a");
    converter
        .convert_event(r#"{"promptFeedback":{"blockReason":"SAFETY"}}"#)
        .unwrap();
    let finish = converter.final_chunk().unwrap();
    assert_eq!(finish["choices"][0]["finish_reason"], "content_filter");
}

#[test]
fn extracts_usage_with_thoughts_and_cache() {
    let usage = extract_usage(&from_str::<Value>(
            r#"{"promptTokenCount":100,"candidatesTokenCount":20,"thoughtsTokenCount":5,"cachedContentTokenCount":30,"totalTokenCount":125}"#,
        )
        .unwrap());
    assert_eq!(usage.input_tokens, Some(100));
    assert_eq!(usage.cache_tokens, 30);
    assert_eq!(usage.output_tokens, Some(25));

    let usage = extract_usage(
        &from_str::<Value>(r#"{"promptTokenCount":100,"totalTokenCount":110}"#).unwrap(),
    );
    assert_eq!(usage.output_tokens, Some(10));
}

#[test]
fn converts_stream_chunk() {
    let mut converter = GeminiStreamConverter::new("req-1", "vm-a");
    let mut chunks = Vec::new();
    for event in [
        r#"{"candidates":[{"content":{"parts":[{"text":"he"},{"text":"","thought":true}]}}],"modelVersion":"gemini-2.5"}"#,
        r#"{"candidates":[{"content":{"parts":[{"text":"llo"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":9,"candidatesTokenCount":2,"cachedContentTokenCount":4}}"#,
    ] {
        chunks.extend(converter.convert_event(event).unwrap());
    }
    if let Some(finish) = converter.final_chunk() {
        chunks.push(finish);
    }
    assert_eq!(chunks[0]["choices"][0]["delta"]["role"], "assistant");
    assert_eq!(chunks[1]["choices"][0]["delta"]["content"], "he");
    assert_eq!(chunks[2]["choices"][0]["delta"]["content"], "llo");
    assert_eq!(chunks[3]["choices"][0]["finish_reason"], "stop");
    let usage = converter.usage().unwrap();
    assert_eq!(usage.cache_tokens, 4);
}

#[test]
fn replays_thought_signature_on_function_call() {
    // 回传的签名按下标挂到对应 functionCall part 上（Gemini 3 强制校验）。
    let chat = from_str::<Value>(
            r#"{"model":"m","messages":[{"role":"user","content":"x"},{"role":"assistant","tool_calls":[{"id":"call_1","type":"function","function":{"name":"f","arguments":"{}"}},{"id":"call_2","type":"function","function":{"name":"g","arguments":"{}"}}],"reasoning_details":[{"type":"reasoning.encrypted","data":"sig-2","id":null,"format":"google-gemini-v1","index":1}]},{"role":"tool","tool_call_id":"call_1","content":"ok"},{"role":"tool","tool_call_id":"call_2","content":"ok"}]}"#,
        )
        .unwrap();
    let body = build_request_body(&chat, "gemini-x").unwrap();
    let parts = &body["contents"][1]["parts"];
    let function_calls: Vec<&Value> = parts
        .as_array()
        .unwrap()
        .iter()
        .filter(|part| part.get("functionCall").is_some())
        .collect();
    assert_eq!(function_calls.len(), 2);
    assert!(function_calls[0].get("thoughtSignature").is_none());
    assert_eq!(function_calls[1]["thoughtSignature"], "sig-2");
}

#[test]
fn extra_content_signatures_are_injected_by_tool_call_index() {
    // AI SDK 客户端（ZCode）回传的签名载体：tool_calls[i].extra_content。
    let chat = from_str::<Value>(
            r#"{"model":"m","messages":[{"role":"user","content":"x"},{"role":"assistant","tool_calls":[{"id":"call_1","type":"function","function":{"name":"f","arguments":"{}"},"extra_content":{"google":{"thought_signature":"sig-a"}}},{"id":"call_2","type":"function","function":{"name":"g","arguments":"{}"}}]}]}"#,
        )
        .unwrap();
    let body = build_request_body(&chat, "gemini-x").unwrap();
    let parts = &body["contents"][1]["parts"];
    let calls: Vec<&Value> = parts
        .as_array()
        .unwrap()
        .iter()
        .filter(|part| part.get("functionCall").is_some())
        .collect();
    assert_eq!(calls[0]["thoughtSignature"], "sig-a");
    assert!(calls[1].get("thoughtSignature").is_none());
}

#[test]
fn extra_content_wins_over_reasoning_details() {
    // 双来源同下标时 extra_content 优先，reasoning_details 补缺。
    let chat = from_str::<Value>(
            r#"{"model":"m","messages":[{"role":"user","content":"x"},{"role":"assistant","tool_calls":[{"id":"call_1","type":"function","function":{"name":"f","arguments":"{}"},"extra_content":{"google":{"thought_signature":"sig-extra"}}}],"reasoning_details":[{"type":"reasoning.encrypted","data":"sig-detail","id":null,"format":"google-gemini-v1","index":0}]}]}"#,
        )
        .unwrap();
    let body = build_request_body(&chat, "gemini-x").unwrap();
    let parts = &body["contents"][1]["parts"];
    let call = parts
        .as_array()
        .unwrap()
        .iter()
        .find(|part| part.get("functionCall").is_some())
        .unwrap();
    assert_eq!(call["thoughtSignature"], "sig-extra");
}

#[test]
fn non_stream_tool_calls_dual_write_signature() {
    let upstream = from_str::<Value>(
        r#"{
                "candidates": [{
                    "content": {"parts": [
                        {"functionCall": {"name": "f", "args": {}}, "thoughtSignature": "sig-a"}
                    ]}
                }],
                "usageMetadata": {"promptTokenCount": 10, "candidatesTokenCount": 3}
            }"#,
    )
    .unwrap();
    let (completion, _) = convert_response(&upstream, "req-1", "vm-a").unwrap();
    let call = &completion["choices"][0]["message"]["tool_calls"][0];
    assert_eq!(
        call["extra_content"]["google"]["thought_signature"],
        "sig-a"
    );
    // reasoning_details 保留（OpenRouter 风格客户端不受影响）。
    assert_eq!(
        completion["choices"][0]["message"]["reasoning_details"][0]["data"],
        "sig-a"
    );
}

#[test]
fn stream_tool_calls_dual_write_signature() {
    let mut converter = GeminiStreamConverter::new("req-1", "vm-a");
    let chunks = converter
            .convert_event(
                r#"{"candidates":[{"content":{"parts":[{"functionCall":{"name":"f","args":{}},"thoughtSignature":"sig-1"}]}}]}"#,
            )
            .unwrap();
    let call = chunks
        .iter()
        .find_map(|chunk| chunk.pointer("/choices/0/delta/tool_calls/0"))
        .unwrap();
    assert_eq!(
        call["extra_content"]["google"]["thought_signature"],
        "sig-1"
    );
}

#[test]
fn cross_format_signatures_are_not_injected() {
    let chat = from_str::<Value>(
            r#"{"model":"m","messages":[{"role":"user","content":"x"},{"role":"assistant","tool_calls":[{"id":"call_1","type":"function","function":{"name":"f","arguments":"{}"}}],"reasoning_details":[{"type":"reasoning.encrypted","data":"blob","id":null,"format":"anthropic-claude-v1","index":0}]},{"role":"tool","tool_call_id":"call_1","content":"ok"}]}"#,
        )
        .unwrap();
    let body = build_request_body(&chat, "gemini-x").unwrap();
    let parts = body["contents"][1]["parts"].as_array().unwrap();
    assert!(
        parts
            .iter()
            .all(|part| part.get("thoughtSignature").is_none())
    );
}

#[test]
fn converts_non_stream_response() {
    let upstream = from_str::<Value>(
            r#"{
                "candidates": [{
                    "content": {"parts": [{"text": "hi"}, {"text": "think", "thought": true}, {"functionCall": {"name": "f", "args": {"a": 1}}}]}
                }],
                "usageMetadata": {"promptTokenCount": 10, "candidatesTokenCount": 3, "thoughtsTokenCount": 2, "cachedContentTokenCount": 4}
            }"#,
        )
        .unwrap();
    let (completion, usage) = convert_response(&upstream, "req-1", "vm-a").unwrap();
    assert_eq!(completion["choices"][0]["message"]["content"], "hi");
    assert_eq!(
        completion["choices"][0]["message"]["reasoning_content"],
        "think"
    );
    assert_eq!(
        completion["choices"][0]["message"]["tool_calls"][0]["function"]["name"],
        "f"
    );
    assert_eq!(completion["choices"][0]["finish_reason"], "tool_calls");
    assert_eq!(
        completion["usage"]["prompt_tokens_details"]["cached_tokens"],
        4
    );
    assert_eq!(usage.output_tokens, Some(5));
}

#[test]
fn non_stream_captures_thought_signatures() {
    let upstream = from_str::<Value>(
            r#"{
                "candidates": [{
                    "content": {"parts": [
                        {"functionCall": {"name": "f", "args": {"a": 1}}, "thoughtSignature": "sig-a"},
                        {"functionCall": {"name": "g", "args": {}}}
                    ]}
                }],
                "usageMetadata": {"promptTokenCount": 10, "candidatesTokenCount": 3}
            }"#,
        )
        .unwrap();
    let (completion, _) = convert_response(&upstream, "req-1", "vm-a").unwrap();
    let details = completion["choices"][0]["message"]["reasoning_details"]
        .as_array()
        .unwrap();
    assert_eq!(details.len(), 1);
    assert_eq!(details[0]["type"], "reasoning.encrypted");
    assert_eq!(details[0]["data"], "sig-a");
    assert_eq!(details[0]["format"], "google-gemini-v1");
    assert_eq!(details[0]["index"], 0);
}

#[test]
fn stream_captures_thought_signatures() {
    let mut converter = GeminiStreamConverter::new("req-1", "vm-a");
    let chunks = converter
            .convert_event(
                r#"{"candidates":[{"content":{"parts":[{"functionCall":{"name":"f","args":{}},"thoughtSignature":"sig-1"}]}}]}"#,
            )
            .unwrap();
    let details: Vec<&Value> = chunks
        .iter()
        .filter_map(|chunk| {
            chunk
                .pointer("/choices/0/delta/reasoning_details")
                .and_then(Value::as_array)
        })
        .flat_map(|items| items.iter())
        .collect();
    assert_eq!(details.len(), 1);
    assert_eq!(details[0]["data"], "sig-1");
    // detail 的 index 与 tool_calls 下标对齐。
    let tool_index = chunks
        .iter()
        .find_map(|chunk| {
            chunk
                .pointer("/choices/0/delta/tool_calls/0/index")
                .and_then(Value::as_i64)
        })
        .unwrap();
    assert_eq!(details[0]["index"], tool_index);
}

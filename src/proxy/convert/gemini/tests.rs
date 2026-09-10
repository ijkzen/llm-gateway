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

// ─── sanitize_gemini_schema（05-06） ───

/// 类型数组取首个非 null 并置 nullable。
#[test]
fn sanitize_schema_type_array_takes_first_non_null() {
    let mut schema = json!({"type": ["string", "null"], "description": "d"});
    sanitize_gemini_schema(&mut schema);
    assert_eq!(schema["type"], "STRING");
    assert_eq!(schema["nullable"], true);
    assert_eq!(schema["description"], "d");
}

/// 类型名大写（OpenAPI → Gemini）。
#[test]
fn sanitize_schema_uppercases_type_names() {
    let mut schema = json!({"type": "object", "properties": {"a": {"type": "integer"}}});
    sanitize_gemini_schema(&mut schema);
    assert_eq!(schema["type"], "OBJECT");
    assert_eq!(schema["properties"]["a"]["type"], "INTEGER");
}

/// format 按类型白名单保留：STRING 只认 enum/date-time，数字类只认 float/double/int32/int64。
#[test]
fn sanitize_schema_format_whitelist_by_type() {
    let mut ok = json!({"type": "string", "format": "date-time"});
    sanitize_gemini_schema(&mut ok);
    assert_eq!(ok["format"], "date-time", "STRING + date-time 应保留");

    let mut bad = json!({"type": "string", "format": "email"});
    sanitize_gemini_schema(&mut bad);
    assert!(bad.get("format").is_none(), "STRING + email 应摘除");

    let mut num = json!({"type": "number", "format": "double"});
    sanitize_gemini_schema(&mut num);
    assert_eq!(num["format"], "double");

    let mut weird = json!({"type": "boolean", "format": "whatever"});
    sanitize_gemini_schema(&mut weird);
    assert!(weird.get("format").is_none(), "非白名单类型一律摘除");
}

/// 未知键被 retain 摘除（如 $schema / additionalProperties）。
#[test]
fn sanitize_schema_drops_unknown_keys() {
    let mut schema = json!({
        "type": "object",
        "additionalProperties": false,
        "$schema": "http://json-schema.org/draft-07/schema#",
        "description": "keep",
        "properties": {"a": {"type": "string", "unevaluatedProperties": true}}
    });
    sanitize_gemini_schema(&mut schema);
    assert!(schema.get("additionalProperties").is_none());
    assert!(schema.get("$schema").is_none());
    assert_eq!(schema["description"], "keep");
    assert!(
        schema["properties"]["a"]
            .get("unevaluatedProperties")
            .is_none()
    );
}

/// 空 properties 摘除（Gemini 报错源）；非空保留。
#[test]
fn sanitize_schema_removes_empty_properties() {
    let mut schema = json!({"type": "object", "properties": {}});
    sanitize_gemini_schema(&mut schema);
    assert!(schema.get("properties").is_none(), "空 properties 应摘除");

    let mut kept = json!({"type": "object", "properties": {"a": {"type": "string"}}});
    sanitize_gemini_schema(&mut kept);
    assert!(kept.get("properties").is_some());
}

/// anyOf 分支递归清洗；items 递归清洗。
#[test]
fn sanitize_schema_recurses_into_any_of_and_items() {
    let mut schema = json!({
        "anyOf": [
            {"type": "string", "format": "email"},
            {"type": ["integer", "null"]}
        ],
        "items": {"type": "object", "properties": {}}
    });
    sanitize_gemini_schema(&mut schema);
    assert!(
        schema["anyOf"][0].get("format").is_none(),
        "anyOf 分支应递归"
    );
    assert_eq!(schema["anyOf"][1]["type"], "INTEGER");
    assert_eq!(schema["anyOf"][1]["nullable"], true);
    assert!(
        schema["items"].get("properties").is_none(),
        "items 应递归且空 properties 摘除"
    );
}

/// 深度超过 16 层停止清洗（防栈溢出/超大 schema）。
#[test]
fn sanitize_schema_stops_beyond_depth_limit() {
    // 构造 20 层嵌套 properties。
    let mut leaf = json!({"type": "string", "format": "email"});
    for _ in 0..20 {
        leaf = json!({"type": "object", "properties": {"n": leaf}});
    }
    let mut schema = leaf;
    sanitize_gemini_schema(&mut schema);
    // 逐层下钻到第 17 层附近：应仍有未清洗的 lowercase type 残留。
    let mut node = &schema;
    let mut depth = 0;
    while let Some(next) = node.pointer("/properties/n") {
        node = next;
        depth += 1;
    }
    assert!(depth > 16, "至少构造 16 层以上：{depth}");
    assert_eq!(
        node["type"], "string",
        "超过深度上限的叶子不应被清洗（保持小写原样）"
    );
}

// ─── 05-05：Gemini 流式转换器不再保留 model 死字段（编译期保证；此处锁别名口径） ───

/// 所有输出 chunk 的 model 一律是请求别名（requested_model），与上游 modelVersion 无关。
#[test]
fn stream_chunks_always_use_requested_model_alias() {
    use crate::proxy::convert::gemini::GeminiStreamConverter;
    let mut converter = GeminiStreamConverter::new("req-1", "my-alias");
    let mut out = Vec::new();
    converter
        .convert_event(
            &json!({"candidates":[{"content":{"parts":[{"text":"hi"}],"role":"model"}}],"modelVersion":"gemini-2.5-pro-001"})
                .to_string(),
        )
        .map(|chunks| out.extend(chunks))
        .unwrap();
    assert!(!out.is_empty());
    for chunk in &out {
        assert_eq!(
            chunk["model"], "my-alias",
            "chunk.model 应为请求别名而非上游 modelVersion"
        );
    }
}

// ─── 05-04：JSON 模式 preamble 不丢弃（非流式） ───

/// 非流式 json 模式：模型先输出 preamble 文本再调 json 工具时，文本并入 content 前缀。
#[test]
fn anthropic_json_mode_keeps_preamble_text() {
    let upstream = json!({
        "content": [
            {"type": "text", "text": "以下是结果："},
            {"type": "tool_use", "name": "__structured_output__", "input": {"a": 1}}
        ],
        "stop_reason": "tool_use",
        "usage": {"input_tokens": 1, "output_tokens": 1}
    });
    let (completion, _) =
        super::super::anthropic::convert_response(&upstream, "req-1", "alias", true).unwrap();
    let content = completion["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or_default();
    assert!(
        content.contains("以下是结果："),
        "preamble 不得丢弃：{content}"
    );
    assert!(content.contains("\"a\":1"), "json 输出应保留：{content}");
}

/// 05-07：非白名单 mime 拒绝（移除 part，不产生 inlineData）。
#[tokio::test]
async fn remote_image_rejects_unsupported_mime() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 2048];
        let _ = sock.read(&mut buf).await;
        let body = b"<svg/>";
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: image/svg+xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = sock.write_all(head.as_bytes()).await;
        let _ = sock.write_all(body).await;
        let _ = sock.shutdown().await;
    });
    let mut body = body_with_remote_image(&format!("http://{addr}/x.svg"));
    inline_remote_images(&mut body, None, "req-1").await;
    server.await.unwrap();
    let parts = body["contents"][0]["parts"].as_array().unwrap();
    assert!(
        parts.iter().all(|part| part.get("fileData").is_none()),
        "非白名单类型应移除 part"
    );
    assert!(
        parts.iter().all(|part| part.get("inlineData").is_none()),
        "非白名单类型不得产生 inlineData"
    );
}

/// 05-07：多图混合（成功 + 失败）时按倒序应用，下标不位移——成功图保留、
/// 失败图移除、前后文本 part 原位不动。
#[tokio::test]
async fn remote_images_mixed_results_keep_indexes_stable() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 2048];
        let _ = sock.read(&mut buf).await;
        let body = b"PNG";
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = sock.write_all(head.as_bytes()).await;
        let _ = sock.write_all(body).await;
        let _ = sock.shutdown().await;
    });
    let ok_url = format!("http://{addr}/a.png");
    let mut body = json!({"contents": [{"role": "user", "parts": [
        {"text": "first"},
        {"fileData": {"fileUri": ok_url}},
        {"text": "middle"},
        {"fileData": {"fileUri": "http://127.0.0.1:1/bad.png"}},
        {"text": "last"},
    ]}]});
    inline_remote_images(&mut body, None, "req-1").await;
    server.await.unwrap();
    let parts = body["contents"][0]["parts"].as_array().unwrap();
    // 失败的图被移除（原下标 3），成功图（原下标 1）转 inlineData，文本原位。
    let texts: Vec<&str> = parts.iter().filter_map(|p| p["text"].as_str()).collect();
    assert_eq!(texts, vec!["first", "middle", "last"], "文本顺序与位置不变");
    let inline_count = parts
        .iter()
        .filter(|p| p.get("inlineData").is_some())
        .count();
    assert_eq!(inline_count, 1, "成功图应转 inlineData");
    assert!(
        parts.iter().all(|p| p.get("fileData").is_none()),
        "fileData 应全部被替换或移除"
    );
}

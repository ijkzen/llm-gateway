
use super::*;
use serde_json::from_str;

#[test]
fn encodes_system_tools_and_tool_results() {
    let chat = from_str::<Value>(
            r#"{
                "model": "vm",
                "messages": [
                    {"role": "system", "content": "be nice"},
                    {"role": "user", "content": "hi"},
                    {"role": "assistant", "tool_calls": [{"id": "call_1", "type": "function", "function": {"name": "get_weather", "arguments": "{\"city\":\"sf\"}"}}]},
                    {"role": "tool", "tool_call_id": "call_1", "content": "sunny"}
                ],
                "tools": [{"type": "function", "function": {"name": "get_weather", "description": "d", "parameters": {"type": "object", "properties": {"city": {"type": "string"}}}}}],
                "max_tokens": 512
            }"#,
        )
        .unwrap();
    let (body, flags) = build_request_body(&chat, "claude-x").unwrap();
    assert!(!flags.json_mode_tool);
    assert_eq!(body["model"], "claude-x");
    assert_eq!(body["max_tokens"], 512);
    assert_eq!(body["system"][0]["text"], "be nice");
    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[1]["content"][0]["type"], "tool_use");
    assert_eq!(messages[1]["content"][0]["input"]["city"], "sf");
    assert_eq!(messages[2]["role"], "user");
    assert_eq!(messages[2]["content"][0]["type"], "tool_result");
    assert_eq!(messages[2]["content"][0]["tool_use_id"], "call_1");
    assert_eq!(body["tools"][0]["name"], "get_weather");
    assert_eq!(body["tools"][0]["input_schema"]["type"], "object");
}

#[test]
fn empty_tool_result_content_gets_placeholder() {
    // Anthropic 拒绝空 text 内容：工具无输出时 tool_result 填占位符。
    let chat = from_str::<Value>(
            r#"{"model":"m","messages":[
                {"role":"user","content":"x"},
                {"role":"assistant","tool_calls":[{"id":"call_1","type":"function","function":{"name":"f","arguments":"{}"}}]},
                {"role":"tool","tool_call_id":"call_1","content":""}
            ]}"#,
        )
        .unwrap();
    let (body, _) = build_request_body(&chat, "claude-x").unwrap();
    assert_eq!(body["messages"][2]["content"][0]["content"], " ");

    // 空数组（content: []）同样兜底。
    let chat = from_str::<Value>(
            r#"{"model":"m","messages":[
                {"role":"user","content":"x"},
                {"role":"assistant","tool_calls":[{"id":"call_1","type":"function","function":{"name":"f","arguments":"{}"}}]},
                {"role":"tool","tool_call_id":"call_1","content":[]}
            ]}"#,
        )
        .unwrap();
    let (body, _) = build_request_body(&chat, "claude-x").unwrap();
    assert_eq!(body["messages"][2]["content"][0]["content"], " ");

    // 非空内容不受影响。
    let chat = from_str::<Value>(
            r#"{"model":"m","messages":[
                {"role":"user","content":"x"},
                {"role":"assistant","tool_calls":[{"id":"call_1","type":"function","function":{"name":"f","arguments":"{}"}}]},
                {"role":"tool","tool_call_id":"call_1","content":"ok"}
            ]}"#,
        )
        .unwrap();
    let (body, _) = build_request_body(&chat, "claude-x").unwrap();
    assert_eq!(body["messages"][2]["content"][0]["content"], "ok");
}

#[test]
fn maps_stop_tool_choice_and_thinking() {
    let chat = from_str::<Value>(
            r#"{"model":"m","messages":[{"role":"user","content":"x"}],"stop":["END", "  "],"tool_choice":"required","parallel_tool_calls":false,"reasoning_effort":"high","max_tokens":4096}"#,
        )
        .unwrap();
    let (body, _) = build_request_body(&chat, "claude-x").unwrap();
    assert_eq!(body["stop_sequences"], json!(["END"]));
    // thinking 与强制 tool_choice 互斥，required 降级为 auto。
    assert_eq!(body["tool_choice"]["type"], "auto");
    assert_eq!(body["tool_choice"]["disable_parallel_tool_use"], true);
    assert_eq!(body["thinking"]["budget_tokens"], 4095);
}

#[test]
fn reasoning_object_enables_thinking() {
    let chat = from_str::<Value>(
            r#"{"model":"m","messages":[{"role":"user","content":"x"}],"reasoning":{"effort":"high"},"max_tokens":4096}"#,
        )
        .unwrap();
    let (body, _) = build_request_body(&chat, "claude-x").unwrap();
    assert_eq!(body["thinking"]["budget_tokens"], 4095);
}

#[test]
fn reasoning_max_tokens_caps_thinking_budget() {
    let chat = from_str::<Value>(
            r#"{"model":"m","messages":[{"role":"user","content":"x"}],"reasoning":{"max_tokens":2000},"max_tokens":4096}"#,
        )
        .unwrap();
    let (body, _) = build_request_body(&chat, "claude-x").unwrap();
    assert_eq!(body["thinking"]["budget_tokens"], 2000);

    // 预算仍受官方上界约束（budget_tokens < max_tokens）。
    let chat = from_str::<Value>(
            r#"{"model":"m","messages":[{"role":"user","content":"x"}],"reasoning":{"max_tokens":8000},"max_tokens":4096}"#,
        )
        .unwrap();
    let (body, _) = build_request_body(&chat, "claude-x").unwrap();
    assert_eq!(body["thinking"]["budget_tokens"], 4095);
}

#[test]
fn reasoning_max_tokens_wins_over_effort() {
    // OpenRouter 语义：effort 与 max_tokens 二选一；同传时显式预算优先。
    let chat = from_str::<Value>(
            r#"{"model":"m","messages":[{"role":"user","content":"x"}],"reasoning":{"effort":"high","max_tokens":2000},"max_tokens":4096}"#,
        )
        .unwrap();
    let (body, _) = build_request_body(&chat, "claude-x").unwrap();
    assert_eq!(body["thinking"]["budget_tokens"], 2000);
}

#[test]
fn thinking_drops_incompatible_sampling_params() {
    // 官方：thinking 启用时 temperature 只能不传或 =1，top_p 不可用。
    let chat = from_str::<Value>(
            r#"{"model":"m","messages":[{"role":"user","content":"x"}],"reasoning_effort":"high","max_tokens":4096,"temperature":0.7,"top_p":0.9}"#,
        )
        .unwrap();
    let (body, _) = build_request_body(&chat, "claude-x").unwrap();
    assert!(body["thinking"].is_object());
    assert!(body.get("temperature").is_none());
    assert!(body.get("top_p").is_none());
}

#[test]
fn top_k_passthrough_and_thinking_conflict() {
    let chat = from_str::<Value>(
        r#"{"model":"m","messages":[{"role":"user","content":"x"}],"top_k":40,"max_tokens":4096}"#,
    )
    .unwrap();
    let (body, _) = build_request_body(&chat, "claude-x").unwrap();
    assert_eq!(body["top_k"], 40);

    // thinking 启用时 top_k 与 temperature/top_p 同规则丢弃。
    let chat = from_str::<Value>(
            r#"{"model":"m","messages":[{"role":"user","content":"x"}],"top_k":40,"reasoning":{"effort":"high"},"max_tokens":4096}"#,
        )
        .unwrap();
    let (body, _) = build_request_body(&chat, "claude-x").unwrap();
    assert!(body["thinking"].is_object());
    assert!(body.get("top_k").is_none());
}

#[test]
fn thinking_keeps_temperature_one() {
    let chat = from_str::<Value>(
            r#"{"model":"m","messages":[{"role":"user","content":"x"}],"reasoning_effort":"high","max_tokens":4096,"temperature":1}"#,
        )
        .unwrap();
    let (body, _) = build_request_body(&chat, "claude-x").unwrap();
    assert_eq!(body["temperature"], 1);
}

#[test]
fn thinking_without_forced_tool_choice_keeps_any() {
    // 未启用 thinking 时强制 tool_choice 不受影响。
    let chat = from_str::<Value>(
        r#"{"model":"m","messages":[{"role":"user","content":"x"}],"tool_choice":"required"}"#,
    )
    .unwrap();
    let (body, _) = build_request_body(&chat, "claude-x").unwrap();
    assert_eq!(body["tool_choice"]["type"], "any");
}

#[test]
fn thinking_dropped_when_history_has_tool_calls_without_thinking_blocks() {
    // assistant 历史永远没有有效 signature 的 thinking 块（转换层不回传
    // reasoning_content），此时启用 thinking 会触发 Anthropic
    // "Expected thinking or redacted_thinking" 400；与 LiteLLM 同款直接丢弃。
    let chat = from_str::<Value>(
            r#"{"model":"m","messages":[{"role":"user","content":"x"},{"role":"assistant","tool_calls":[{"id":"call_1","type":"function","function":{"name":"f","arguments":"{}"}}]},{"role":"tool","tool_call_id":"call_1","content":"ok"}],"reasoning_effort":"high","max_tokens":4096}"#,
        )
        .unwrap();
    let (body, _) = build_request_body(&chat, "claude-x").unwrap();
    assert!(body.get("thinking").is_none());
}

#[test]
fn drops_thinking_when_max_tokens_too_small() {
    let chat = from_str::<Value>(
            r#"{"model":"m","messages":[{"role":"user","content":"x"}],"reasoning_effort":"high","max_tokens":1000}"#,
        )
        .unwrap();
    let (body, _) = build_request_body(&chat, "claude-x").unwrap();
    assert!(body.get("thinking").is_none());
}

#[test]
fn clamps_minimal_effort_budget_to_official_minimum() {
    // Anthropic 官方要求 budget_tokens >= 1024，更小的值会被 400 拒绝。
    let chat = from_str::<Value>(
            r#"{"model":"m","messages":[{"role":"user","content":"x"}],"reasoning_effort":"minimal","max_tokens":4096}"#,
        )
        .unwrap();
    let (body, _) = build_request_body(&chat, "claude-x").unwrap();
    assert_eq!(body["thinking"]["budget_tokens"], 1024);
}

#[test]
fn defaults_max_tokens_to_4096() {
    let chat =
        from_str::<Value>(r#"{"model":"m","messages":[{"role":"user","content":"x"}]}"#).unwrap();
    let (body, _) = build_request_body(&chat, "claude-x").unwrap();
    assert_eq!(body["max_tokens"], 4096);
}

#[test]
fn response_format_becomes_synthetic_tool() {
    let chat = from_str::<Value>(
            r#"{"model":"m","messages":[{"role":"user","content":"x"}],"response_format":{"type":"json_schema","json_schema":{"schema":{"type":"object","properties":{"a":{"type":"string"}}}}}}"#,
        )
        .unwrap();
    let (body, flags) = build_request_body(&chat, "claude-x").unwrap();
    assert!(flags.json_mode_tool);
    assert_eq!(body["tools"][0]["name"], JSON_TOOL_NAME);
    // thinking 模式拒绝 tool_choice，因此 json 模式不锁定 tool_choice，
    // 改为 system 强指令引导调用。
    assert!(body.get("tool_choice").is_none());
    let system_texts: Vec<&str> = body["system"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|block| block["text"].as_str())
        .collect();
    assert!(system_texts.iter().any(|t| t.contains(JSON_TOOL_NAME)));
}

#[test]
fn json_mode_works_with_thinking() {
    let chat = from_str::<Value>(
            r#"{"model":"m","messages":[{"role":"user","content":"x"}],"reasoning_effort":"low","response_format":{"type":"json_object"}}"#,
        )
        .unwrap();
    let (body, flags) = build_request_body(&chat, "claude-x").unwrap();
    assert!(flags.json_mode_tool);
    assert!(body["thinking"].is_object(), "thinking 与 json 模式可共存");
    assert!(body.get("tool_choice").is_none());
}

#[test]
fn thinking_kept_when_tool_turn_has_thinking_details() {
    // 客户端原样回传带签名的 thinking 块时，tool_use 轮保留 thinking 并注入块。
    let chat = from_str::<Value>(
            r#"{"model":"m","messages":[{"role":"user","content":"x"},{"role":"assistant","content":"需要查询","tool_calls":[{"id":"call_1","type":"function","function":{"name":"f","arguments":"{}"}}],"reasoning_details":[{"type":"reasoning.text","text":"想一想","signature":"sig-1","id":null,"format":"anthropic-claude-v1","index":0}]},{"role":"tool","tool_call_id":"call_1","content":"ok"}],"reasoning_effort":"high","max_tokens":4096}"#,
        )
        .unwrap();
    let (body, _) = build_request_body(&chat, "claude-x").unwrap();
    assert!(body["thinking"].is_object());
    let assistant = &body["messages"][1];
    assert_eq!(assistant["content"][0]["type"], "thinking");
    assert_eq!(assistant["content"][0]["thinking"], "想一想");
    assert_eq!(assistant["content"][0]["signature"], "sig-1");
    assert_eq!(assistant["content"][1]["type"], "text");
    assert_eq!(assistant["content"][2]["type"], "tool_use");
}

#[test]
fn redacted_thinking_detail_is_injected_verbatim() {
    let chat = from_str::<Value>(
            r#"{"model":"m","messages":[{"role":"user","content":"x"},{"role":"assistant","tool_calls":[{"id":"call_1","type":"function","function":{"name":"f","arguments":"{}"}}],"reasoning_details":[{"type":"reasoning.encrypted","data":"blob","id":null,"format":"anthropic-claude-v1","index":0}]},{"role":"tool","tool_call_id":"call_1","content":"ok"}],"reasoning_effort":"high","max_tokens":4096}"#,
        )
        .unwrap();
    let (body, _) = build_request_body(&chat, "claude-x").unwrap();
    assert!(body["thinking"].is_object());
    assert_eq!(
        body["messages"][1]["content"][0]["type"],
        "redacted_thinking"
    );
    assert_eq!(body["messages"][1]["content"][0]["data"], "blob");
}

#[test]
fn thinking_dropped_when_tool_turn_details_are_other_format() {
    // 其他厂商格式（加密签名不互通）无法用于 Anthropic，维持禁用规避。
    let chat = from_str::<Value>(
            r#"{"model":"m","messages":[{"role":"user","content":"x"},{"role":"assistant","tool_calls":[{"id":"call_1","type":"function","function":{"name":"f","arguments":"{}"}}],"reasoning_details":[{"type":"reasoning.encrypted","data":"gAAA","id":null,"format":"google-gemini-v1","index":0}]},{"role":"tool","tool_call_id":"call_1","content":"ok"}],"reasoning_effort":"high","max_tokens":4096}"#,
        )
        .unwrap();
    let (body, _) = build_request_body(&chat, "claude-x").unwrap();
    assert!(body.get("thinking").is_none());
}

#[test]
fn thinking_details_without_signature_are_skipped() {
    // 无签名的 text detail 无法通过官方校验，按缺块处理（维持禁用规避）。
    let chat = from_str::<Value>(
            r#"{"model":"m","messages":[{"role":"user","content":"x"},{"role":"assistant","tool_calls":[{"id":"call_1","type":"function","function":{"name":"f","arguments":"{}"}}],"reasoning_details":[{"type":"reasoning.text","text":"想","signature":null,"id":null,"format":"anthropic-claude-v1","index":0}]},{"role":"tool","tool_call_id":"call_1","content":"ok"}],"reasoning_effort":"high","max_tokens":4096}"#,
        )
        .unwrap();
    let (body, _) = build_request_body(&chat, "claude-x").unwrap();
    assert!(body.get("thinking").is_none());
}

#[test]
fn thinking_details_not_injected_without_thinking_enabled() {
    // 未申请思考时不注入块（官方约束：input 带 thinking 块必须开 thinking）。
    let chat = from_str::<Value>(
            r#"{"model":"m","messages":[{"role":"user","content":"x"},{"role":"assistant","content":"答","reasoning_details":[{"type":"reasoning.text","text":"想","signature":"sig-1","id":null,"format":"anthropic-claude-v1","index":0}]}]}"#,
        )
        .unwrap();
    let (body, _) = build_request_body(&chat, "claude-x").unwrap();
    assert!(body.get("thinking").is_none());
    let assistant = &body["messages"][1];
    assert_eq!(assistant["content"][0]["type"], "text");
}

#[test]
fn maps_extended_stop_reasons() {
    // 官方 StopReason 新枚举：context window 耗尽与 compaction 语义为
    // length；pause_turn 是可续传的中断，按 stop 透出；原生值随 native_finish_reason 透传。
    assert_eq!(
        normalize_stop_reason("model_context_window_exceeded", false),
        ("length", Some("model_context_window_exceeded"))
    );
    assert_eq!(
        normalize_stop_reason("compaction", false),
        ("length", Some("compaction"))
    );
    assert_eq!(
        normalize_stop_reason("pause_turn", false),
        ("stop", Some("pause_turn"))
    );
    assert_eq!(normalize_stop_reason("", false), ("stop", None));
}

#[test]
fn user_param_becomes_metadata_user_id() {
    let chat = from_str::<Value>(
        r#"{"model":"m","messages":[{"role":"user","content":"x"}],"user":"user-123"}"#,
    )
    .unwrap();
    let (body, _) = build_request_body(&chat, "claude-x").unwrap();
    assert_eq!(body["metadata"]["user_id"], "user-123");
}

#[test]
fn converts_non_stream_response() {
    let upstream = from_str::<Value>(
            r#"{
                "id": "msg_1",
                "content": [
                    {"type": "thinking", "thinking": "hmm"},
                    {"type": "text", "text": "hello"},
                    {"type": "tool_use", "id": "toolu_1", "name": "get_weather", "input": {"city": "sf"}}
                ],
                "stop_reason": "tool_use",
                "usage": {"input_tokens": 10, "output_tokens": 5, "cache_read_input_tokens": 3, "cache_creation_input_tokens": 2}
            }"#,
        )
        .unwrap();
    let (completion, usage) = convert_response(&upstream, "req-1", "vm-a", false).unwrap();
    assert_eq!(completion["object"], "chat.completion");
    assert_eq!(completion["model"], "vm-a");
    assert_eq!(completion["choices"][0]["finish_reason"], "tool_calls");
    assert_eq!(completion["choices"][0]["native_finish_reason"], "tool_use");
    assert_eq!(completion["choices"][0]["message"]["content"], "hello");
    assert_eq!(
        completion["choices"][0]["message"]["reasoning_content"],
        "hmm"
    );
    assert_eq!(
        completion["choices"][0]["message"]["tool_calls"][0]["id"],
        "toolu_1"
    );
    assert_eq!(usage.input_tokens, Some(15));
    assert_eq!(usage.cache_tokens, 3);
    assert_eq!(usage.output_tokens, Some(5));
    // 客户端 usage 带缓存命中明细（与其他协议路径一致）。
    assert_eq!(
        completion["usage"]["prompt_tokens_details"]["cached_tokens"],
        3
    );
}

#[test]
fn converts_stream_events_to_chunks() {
    let mut converter = AnthropicStreamConverter::new("req-1", "vm-a", false);
    let mut chunks = Vec::new();
    for event in [
        r#"{"type":"message_start","message":{"id":"msg_1","usage":{"input_tokens":10}}}"#,
        r#"{"type":"content_block_start","index":0,"content_block":{"type":"text"}}"#,
        r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hi"}}"#,
        r#"{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_1","name":"f"}}"#,
        r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"a\":"}}"#,
        r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":7}}"#,
        r#"{"type":"message_stop"}"#,
    ] {
        chunks.extend(converter.convert_event(event).unwrap());
    }
    assert!(converter.is_finished());
    assert_eq!(chunks[0]["choices"][0]["delta"]["role"], "assistant");
    assert_eq!(chunks[1]["choices"][0]["delta"]["content"], "hi");
    let tool_start = chunks[2]["choices"][0]["delta"]["tool_calls"][0].clone();
    assert_eq!(tool_start["index"], 0);
    assert_eq!(tool_start["function"]["name"], "f");
    let tool_args = chunks[3]["choices"][0]["delta"]["tool_calls"][0].clone();
    assert_eq!(tool_args["index"], 0);
    assert_eq!(tool_args["function"]["arguments"], "{\"a\":");
    let finish = chunks[4]["choices"][0]["finish_reason"].as_str().unwrap();
    assert_eq!(finish, "tool_calls");
    let usage = converter.usage().unwrap();
    assert_eq!(usage.input_tokens, Some(10));
    assert_eq!(usage.output_tokens, Some(7));
}

#[test]
fn json_mode_tool_output_becomes_content() {
    let mut converter = AnthropicStreamConverter::new("req-1", "vm-a", true);
    let mut chunks = Vec::new();
    for event in [
        r#"{"type":"message_start","message":{"id":"msg_1","usage":{"input_tokens":10}}}"#,
        r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_1","name":"__structured_output__"}}"#,
        r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"a\":1}"}}"#,
        r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":3}}"#,
        r#"{"type":"message_stop"}"#,
    ] {
        chunks.extend(converter.convert_event(event).unwrap());
    }
    let content = chunks
        .iter()
        .filter_map(|c| c["choices"][0]["delta"]["content"].as_str())
        .collect::<String>();
    assert_eq!(content, "{\"a\":1}");
    assert_eq!(
        chunks.last().unwrap()["choices"][0]["finish_reason"],
        "stop"
    );
}

#[test]
fn non_stream_captures_thinking_signature_details() {
    let upstream = from_str::<Value>(
        r#"{
                "id": "msg_1",
                "content": [
                    {"type": "thinking", "thinking": "hmm", "signature": "EqQBCkYI"},
                    {"type": "redacted_thinking", "data": "encrypted-blob"},
                    {"type": "text", "text": "hello"}
                ],
                "stop_reason": "end_turn",
                "usage": {"input_tokens": 10, "output_tokens": 5}
            }"#,
    )
    .unwrap();
    let (completion, _) = convert_response(&upstream, "req-1", "vm-a", false).unwrap();
    let details = completion["choices"][0]["message"]["reasoning_details"]
        .as_array()
        .unwrap();
    assert_eq!(details.len(), 2);
    assert_eq!(details[0]["type"], "reasoning.text");
    assert_eq!(details[0]["text"], "hmm");
    assert_eq!(details[0]["signature"], "EqQBCkYI");
    assert_eq!(details[0]["format"], "anthropic-claude-v1");
    assert_eq!(details[0]["index"], 0);
    assert_eq!(details[1]["type"], "reasoning.encrypted");
    assert_eq!(details[1]["data"], "encrypted-blob");
    assert_eq!(details[1]["format"], "anthropic-claude-v1");
    assert_eq!(details[1]["index"], 1);
}

#[test]
fn stream_captures_thinking_signature_details() {
    let mut converter = AnthropicStreamConverter::new("req-1", "vm-a", false);
    let mut chunks = Vec::new();
    for event in [
        r#"{"type":"message_start","message":{"id":"msg_1","usage":{"input_tokens":10}}}"#,
        r#"{"type":"content_block_start","index":0,"content_block":{"type":"thinking"}}"#,
        r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"想"}}"#,
        r#"{"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig-1"}}"#,
        r#"{"type":"content_block_stop","index":0}"#,
        r#"{"type":"content_block_start","index":1,"content_block":{"type":"redacted_thinking","data":"blob"}}"#,
        r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":3}}"#,
        r#"{"type":"message_stop"}"#,
    ] {
        chunks.extend(converter.convert_event(event).unwrap());
    }
    let details: Vec<&Value> = chunks
        .iter()
        .filter_map(|chunk| {
            chunk
                .pointer("/choices/0/delta/reasoning_details")
                .and_then(Value::as_array)
        })
        .flat_map(|items| items.iter())
        .collect();
    assert_eq!(details.len(), 2);
    assert_eq!(details[0]["type"], "reasoning.text");
    assert_eq!(details[0]["text"], "想");
    assert_eq!(details[0]["signature"], "sig-1");
    assert_eq!(details[0]["index"], 0);
    assert_eq!(details[1]["type"], "reasoning.encrypted");
    assert_eq!(details[1]["data"], "blob");
    // thinking 文本增量照旧透出 reasoning_content。
    let reasoning: String = chunks
        .iter()
        .filter_map(|chunk| {
            chunk
                .pointer("/choices/0/delta/reasoning_content")
                .and_then(Value::as_str)
        })
        .collect();
    assert_eq!(reasoning, "想");
}

#[test]
fn passthrough_scanner_merges_stream_usage() {
    let mut scanner = AnthropicStreamUsageScanner::default();
    scanner.feed(b"data: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"usage\":{\"input_tokens\":10,\"cache_read_input_tokens\":3,\"cache_creation_input_tokens\":2}}}\n\n");
    scanner.feed("event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"你\"}}\n\n".as_bytes());
    scanner.feed(b"data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":5}}\n\n");
    scanner.feed(b"data: {\"type\":\"message_stop\"}\n\n");
    assert!(scanner.take_content_seen());
    let usage = scanner.usage().expect("usage should be captured");
    assert_eq!(usage.input_tokens, Some(15));
    assert_eq!(usage.cache_tokens, 3);
    assert_eq!(usage.output_tokens, Some(5));
}

#[test]
fn passthrough_scanner_handles_split_feeds_and_no_usage() {
    let mut scanner = AnthropicStreamUsageScanner::default();
    let text = "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"你好\"}}\n\ndata: {\"type\":\"message_stop\"}\n\n";
    let bytes = text.as_bytes();
    let (a, b) = bytes.split_at(bytes.len() / 2);
    scanner.feed(a);
    scanner.feed(b);
    assert!(scanner.take_content_seen());
    assert!(scanner.usage().is_none());
}

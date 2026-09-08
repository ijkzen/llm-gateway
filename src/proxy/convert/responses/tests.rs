
use super::*;
use serde_json::from_str;

#[test]
fn encodes_input_items_and_tools() {
    let chat = from_str::<Value>(
            r#"{
                "model": "vm",
                "messages": [
                    {"role": "system", "content": "be terse"},
                    {"role": "user", "content": "hi"},
                    {"role": "assistant", "tool_calls": [{"id": "call_1", "type": "function", "function": {"name": "f", "arguments": "{\"a\":1}"}}]},
                    {"role": "tool", "tool_call_id": "call_1", "content": "ok"}
                ],
                "tools": [{"type": "function", "function": {"name": "f", "parameters": {"type": "object", "properties": {"a": {"type": "string"}}}}}],
                "max_tokens": 256,
                "tool_choice": {"type": "function", "function": {"name": "f"}}
            }"#,
        )
        .unwrap();
    let body = build_request_body(&chat, "gpt-5").unwrap();
    assert_eq!(body["model"], "gpt-5");
    assert_eq!(body["stream"], true);
    assert_eq!(body["store"], false);
    assert_eq!(body["instructions"], "be terse");
    assert_eq!(body["max_output_tokens"], 256);
    let input = body["input"].as_array().unwrap();
    assert_eq!(input[0]["type"], "message");
    assert_eq!(input[0]["content"][0]["type"], "input_text");
    assert_eq!(input[1]["type"], "function_call");
    assert_eq!(input[1]["call_id"], "call_1");
    assert_eq!(input[2]["type"], "function_call_output");
    assert_eq!(input[2]["output"], "ok");
    assert_eq!(body["tools"][0]["type"], "function");
    assert_eq!(body["tools"][0]["name"], "f");
    assert_eq!(
        body["tool_choice"],
        json!({"type": "function", "name": "f"})
    );
}

#[test]
fn drops_chat_only_params() {
    let chat = from_str::<Value>(
            r#"{"model":"m","messages":[{"role":"user","content":"x"}],"stop":["END"],"seed":1,"frequency_penalty":0.5,"logit_bias":{}}"#,
        )
        .unwrap();
    let body = build_request_body(&chat, "gpt-5").unwrap();
    assert!(body.get("stop").is_none());
    assert!(body.get("seed").is_none());
    assert!(body.get("frequency_penalty").is_none());
}

#[test]
fn reasoning_effort_clamped_to_responses_enum() {
    // max 不是 OpenAI Responses 合法枚举，钳到 xhigh；合法档原样透传。
    let chat = from_str::<Value>(
        r#"{"model":"m","messages":[{"role":"user","content":"x"}],"reasoning_effort":"max"}"#,
    )
    .unwrap();
    let body = build_request_body(&chat, "gpt-5").unwrap();
    assert_eq!(body["reasoning"]["effort"], "xhigh");

    let chat = from_str::<Value>(
        r#"{"model":"m","messages":[{"role":"user","content":"x"}],"reasoning_effort":"high"}"#,
    )
    .unwrap();
    let body = build_request_body(&chat, "gpt-5").unwrap();
    assert_eq!(body["reasoning"]["effort"], "high");
}

#[test]
fn disabled_reasoning_writes_effort_none() {
    // 明确关闭：Responses 缺省按默认档位思考，必须显式写 none。
    let chat = from_str::<Value>(
        r#"{"model":"m","messages":[{"role":"user","content":"x"}],"reasoning_effort":"none"}"#,
    )
    .unwrap();
    let body = build_request_body(&chat, "gpt-5").unwrap();
    assert_eq!(body["reasoning"]["effort"], "none");

    // 未指定：不写 reasoning 字段（行为与旧版一致）。
    let chat =
        from_str::<Value>(r#"{"model":"m","messages":[{"role":"user","content":"x"}]}"#).unwrap();
    let body = build_request_body(&chat, "gpt-5").unwrap();
    assert!(body.get("reasoning").is_none());
}

#[test]
fn function_call_output_sets_tool_calls_finish() {
    // 审计 C1：output 以 function_call 收尾时 finish_reason 必须是
    // tool_calls，否则 OpenAI 客户端工具循环误判为最终回答。
    let mut converter = ResponsesStreamConverter::new("req-1", "vm-a");
    let chunks = converter
            .convert_event(
                r#"{"type":"response.completed","response":{"status":"completed","output":[{"type":"function_call","call_id":"call_1","name":"f","arguments":"{}"}]}}"#,
            )
            .unwrap();
    assert_eq!(
        chunks.last().unwrap()["choices"][0]["finish_reason"],
        "tool_calls"
    );
}

#[test]
fn no_system_message_omits_instructions() {
    // instructions 可选：客户端没写 system 时不得凭空注入默认系统提示。
    let chat =
        from_str::<Value>(r#"{"model":"m","messages":[{"role":"user","content":"x"}]}"#).unwrap();
    let body = build_request_body(&chat, "gpt-5").unwrap();
    assert!(body.get("instructions").is_none());
}

#[test]
fn refusal_deltas_become_content_without_replay_duplication() {
    let mut converter = ResponsesStreamConverter::new("req-1", "vm-a");
    let mut chunks = Vec::new();
    for event in [
        r#"{"type":"response.created","response":{"id":"resp_1"}}"#,
        r#"{"type":"response.refusal.delta","output_index":0,"delta":"抱歉"}"#,
        r#"{"type":"response.refusal.done","output_index":0,"refusal":"抱歉，不行"}"#,
        r#"{"type":"response.completed","response":{"status":"completed","output":[{"type":"message","content":[{"type":"refusal","refusal":"抱歉，不行"}]}]}}"#,
    ] {
        chunks.extend(converter.convert_event(event).unwrap());
    }
    let content: Vec<&str> = chunks
        .iter()
        .filter_map(|c| {
            c.pointer("/choices/0/delta/content")
                .and_then(Value::as_str)
        })
        .collect();
    // delta 与 done 只补差额；completed 回放不再重复。
    assert_eq!(content, ["抱歉", "，不行"]);
}

#[test]
fn converts_stream_events() {
    let mut converter = ResponsesStreamConverter::new("req-1", "vm-a");
    let mut chunks = Vec::new();
    for event in [
        r#"{"type":"response.created","response":{"id":"resp_1","model":"gpt-5"}}"#,
        r#"{"type":"response.output_text.delta","delta":"hi"}"#,
        r#"{"type":"response.output_item.added","output_index":1,"item":{"type":"function_call","call_id":"call_9","name":"f"}}"#,
        r#"{"type":"response.function_call_arguments.delta","output_index":1,"delta":"{\"a\":"}"#,
        r#"{"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":12,"output_tokens":4,"input_tokens_details":{"cached_tokens":6}}}}"#,
    ] {
        chunks.extend(converter.convert_event(event).unwrap());
    }
    assert!(converter.is_finished());
    // chunk 的 model 保持客户端请求的虚拟模型别名，与 Anthropic/Gemini 路径一致。
    assert_eq!(chunks[0]["model"], "vm-a");
    assert_eq!(chunks[0]["choices"][0]["delta"]["role"], "assistant");
    assert_eq!(chunks[1]["choices"][0]["delta"]["content"], "hi");
    assert_eq!(
        chunks[2]["choices"][0]["delta"]["tool_calls"][0]["id"],
        "call_9"
    );
    assert_eq!(
        chunks[2]["choices"][0]["delta"]["tool_calls"][0]["index"],
        0
    );
    assert_eq!(chunks[4]["choices"][0]["finish_reason"], "stop");
    let usage = converter.usage().unwrap();
    assert_eq!(usage.input_tokens, Some(12));
    assert_eq!(usage.cache_tokens, 6);
    assert_eq!(usage.output_tokens, Some(4));
}

#[test]
fn recovers_final_output_without_deltas() {
    let mut converter = ResponsesStreamConverter::new("req-1", "vm-a");
    let mut chunks = Vec::new();
    for event in [
        r#"{"type":"response.created","response":{"id":"resp_1","model":"gpt-5"}}"#,
        r#"{"type":"response.output_item.done","output_index":0,"item":{"type":"message","content":[{"type":"output_text","text":"你好"},{"type":"reasoning","summary":[{"type":"summary_text","text":"思考"}]}]}}"#,
        r#"{"type":"response.output_item.done","output_index":1,"item":{"type":"function_call","arguments":"{\"a\":1}"}}"#,
        r#"{"type":"response.completed","response":{"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"你好"},{"type":"reasoning","summary":[{"type":"summary_text","text":"思考"}]}]},{"type":"function_call","arguments":"{\"a\":1}"}]}}"#,
    ] {
        chunks.extend(converter.convert_event(event).unwrap());
    }

    let content: Vec<&Value> = chunks
        .iter()
        .filter(|chunk| chunk.pointer("/choices/0/delta/content").is_some())
        .collect();
    assert_eq!(content.len(), 1);
    assert_eq!(content[0]["choices"][0]["delta"]["content"], "你好");
    let reasoning: Vec<&Value> = chunks
        .iter()
        .filter(|chunk| {
            chunk
                .pointer("/choices/0/delta/reasoning_content")
                .is_some()
        })
        .collect();
    assert_eq!(reasoning.len(), 1);
    assert_eq!(
        reasoning[0]["choices"][0]["delta"]["reasoning_content"],
        "思考"
    );
    let arguments: Vec<&Value> = chunks
        .iter()
        .filter(|chunk| {
            chunk
                .pointer("/choices/0/delta/tool_calls/0/function/arguments")
                .is_some()
        })
        .collect();
    assert_eq!(arguments.len(), 1);
    assert_eq!(
        arguments[0]["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"],
        "{\"a\":1}"
    );
}

#[test]
fn final_output_only_emits_missing_argument_suffix() {
    let mut converter = ResponsesStreamConverter::new("req-1", "vm-a");
    let mut chunks = Vec::new();
    for event in [
        r#"{"type":"response.function_call_arguments.delta","output_index":0,"delta":"{\"a\":"}"#,
        r#"{"type":"response.output_item.done","output_index":0,"item":{"type":"function_call","arguments":"{\"a\":1}"}}"#,
    ] {
        chunks.extend(converter.convert_event(event).unwrap());
    }

    let arguments: Vec<&str> = chunks
        .iter()
        .filter_map(|chunk| {
            chunk
                .pointer("/choices/0/delta/tool_calls/0/function/arguments")
                .and_then(Value::as_str)
        })
        .collect();
    assert_eq!(arguments, ["{\"a\":", "1}"]);
}

#[test]
fn final_output_only_emits_missing_delta_suffix() {
    let mut converter = ResponsesStreamConverter::new("req-1", "vm-a");
    let mut chunks = Vec::new();
    for event in [
        r#"{"type":"response.output_text.delta","output_index":0,"delta":"hel"}"#,
        r#"{"type":"response.output_item.done","output_index":0,"item":{"type":"message","content":[{"type":"output_text","text":"hello"}]}}"#,
        r#"{"type":"response.completed","response":{"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"hello"}]}]}}"#,
    ] {
        chunks.extend(converter.convert_event(event).unwrap());
    }

    let text: Vec<&str> = chunks
        .iter()
        .filter_map(|chunk| {
            chunk
                .pointer("/choices/0/delta/content")
                .and_then(Value::as_str)
        })
        .collect();
    assert_eq!(text, ["hel", "lo"]);
}

#[test]
fn replays_reasoning_items_before_their_outputs() {
    let chat = from_str::<Value>(
            r#"{"model":"m","messages":[{"role":"user","content":"x"},{"role":"assistant","content":"hi","tool_calls":[{"id":"call_1","type":"function","function":{"name":"f","arguments":"{}"}}],"reasoning_details":[{"type":"reasoning.encrypted","data":"gAAA","id":"rs_1","format":"openai-responses-v1","index":0}]},{"role":"tool","tool_call_id":"call_1","content":"ok"}]}"#,
        )
        .unwrap();
    let body = build_request_body(&chat, "gpt-5").unwrap();
    let input = body["input"].as_array().unwrap();
    // input[0] 是 user 消息；reasoning item 紧贴其产出项之前。
    assert_eq!(input[1]["type"], "reasoning");
    assert_eq!(input[1]["id"], "rs_1");
    assert_eq!(input[1]["encrypted_content"], "gAAA");
    assert_eq!(input[1]["summary"], json!([]));
    assert_eq!(input[2]["type"], "message");
    assert_eq!(input[3]["type"], "function_call");
    assert_eq!(input[4]["type"], "function_call_output");
}

#[test]
fn cross_format_reasoning_details_are_not_injected() {
    // 其他厂商格式不注入（加密签名不互通，failover 换供应商时属预期）。
    let chat = from_str::<Value>(
            r#"{"model":"m","messages":[{"role":"user","content":"x"},{"role":"assistant","content":"hi","reasoning_details":[{"type":"reasoning.text","text":"想","signature":"sig","id":null,"format":"anthropic-claude-v1","index":0}]}]}"#,
        )
        .unwrap();
    let body = build_request_body(&chat, "gpt-5").unwrap();
    let input = body["input"].as_array().unwrap();
    assert!(input.iter().all(|item| item["type"] != "reasoning"));
}

#[test]
fn maps_incomplete_status_to_length() {
    let mut converter = ResponsesStreamConverter::new("req-1", "vm-a");
    let chunks = converter
            .convert_event(
                r#"{"type":"response.incomplete","response":{"status":"incomplete","incomplete_details":{"reason":"max_output_tokens"},"usage":{"input_tokens":1,"output_tokens":2}}}"#,
            )
            .unwrap();
    assert_eq!(
        chunks.last().unwrap()["choices"][0]["finish_reason"],
        "length"
    );
}

#[test]
fn captures_encrypted_reasoning_details_with_replay_dedupe() {
    let mut converter = ResponsesStreamConverter::new("req-1", "vm-a");
    let mut chunks = Vec::new();
    for event in [
        r#"{"type":"response.created","response":{"id":"resp_1"}}"#,
        r#"{"type":"response.output_item.done","output_index":0,"item":{"type":"reasoning","id":"rs_1","summary":[{"type":"summary_text","text":"思考"}],"encrypted_content":"gAAA"}}"#,
        r#"{"type":"response.completed","response":{"status":"completed","output":[{"type":"reasoning","id":"rs_1","summary":[{"type":"summary_text","text":"思考"}],"encrypted_content":"gAAA"}]}}"#,
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
    // output_item.done 与 completed 回放只捕获一次。
    assert_eq!(details.len(), 1);
    assert_eq!(details[0]["type"], "reasoning.encrypted");
    assert_eq!(details[0]["data"], "gAAA");
    assert_eq!(details[0]["id"], "rs_1");
    assert_eq!(details[0]["format"], "openai-responses-v1");
    // 无 encrypted_content 的 reasoning item 不产出 detail。
    let mut converter = ResponsesStreamConverter::new("req-2", "vm-a");
    let chunks = converter
            .convert_event(
                r#"{"type":"response.output_item.done","output_index":0,"item":{"type":"reasoning","id":"rs_2","summary":[]}}"#,
            )
            .unwrap();
    assert!(chunks.iter().all(|chunk| {
        chunk
            .pointer("/choices/0/delta/reasoning_details")
            .is_none()
    }));
}

#[test]
fn requests_encrypted_reasoning_include() {
    let chat =
        from_str::<Value>(r#"{"model":"m","messages":[{"role":"user","content":"x"}]}"#).unwrap();
    let body = build_request_body(&chat, "gpt-5").unwrap();
    assert_eq!(body["include"], json!(["reasoning.encrypted_content"]));
}

#[test]
fn reasoning_object_sets_effort() {
    let chat = from_str::<Value>(
        r#"{"model":"m","messages":[{"role":"user","content":"x"}],"reasoning":{"effort":"high"}}"#,
    )
    .unwrap();
    let body = build_request_body(&chat, "gpt-5").unwrap();
    assert_eq!(body["reasoning"]["effort"], "high");

    let chat =
        from_str::<Value>(r#"{"model":"m","messages":[{"role":"user","content":"x"}]}"#).unwrap();
    let body = build_request_body(&chat, "gpt-5").unwrap();
    assert!(body.get("reasoning").is_none());
}

#[test]
fn reasoning_max_tokens_is_dropped_for_responses() {
    // Responses 的 reasoning 只接受 effort；max_tokens 无对应参数，按「不支持即忽略」丢弃。
    let chat = from_str::<Value>(
            r#"{"model":"m","messages":[{"role":"user","content":"x"}],"reasoning":{"max_tokens":2000}}"#,
        )
        .unwrap();
    let body = build_request_body(&chat, "gpt-5").unwrap();
    assert_eq!(body["reasoning"]["effort"], "medium");
    assert!(body["reasoning"].get("max_tokens").is_none());
}

#[test]
fn top_k_passthrough_for_responses() {
    let chat =
        from_str::<Value>(r#"{"model":"m","messages":[{"role":"user","content":"x"}],"top_k":40}"#)
            .unwrap();
    let body = build_request_body(&chat, "gpt-5").unwrap();
    assert_eq!(body["top_k"], 40);
}

#[test]
fn passthrough_scanner_captures_completed_usage_and_content() {
    let mut scanner = ResponsesStreamUsageScanner::default();
    scanner.feed(b"data: {\"type\":\"response.created\",\"response\":{\"id\":\"r\"}}\n\n");
    scanner.feed("data: {\"type\":\"response.output_text.delta\",\"delta\":\"你\"}\n\n".as_bytes());
    scanner.feed(b"data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":12,\"output_tokens\":6,\"input_tokens_details\":{\"cached_tokens\":5},\"output_tokens_details\":{\"reasoning_tokens\":3}}}}\n\n");
    assert!(scanner.take_content_seen());
    let usage = scanner.usage().expect("usage should be captured");
    assert_eq!(usage.input_tokens, Some(12));
    assert_eq!(usage.cache_tokens, 5);
    assert_eq!(usage.output_tokens, Some(6));
    assert_eq!(usage.reasoning_tokens, Some(3));
}

#[test]
fn passthrough_scanner_without_usage_yields_none() {
    let mut scanner = ResponsesStreamUsageScanner::default();
    scanner.feed(b"data: {\"type\":\"response.created\",\"response\":{\"id\":\"r\"}}\n\n");
    assert!(!scanner.take_content_seen());
    assert!(scanner.usage().is_none());
}

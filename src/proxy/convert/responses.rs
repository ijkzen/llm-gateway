//! OpenAI chat → OpenAI Responses API 转换。
//!
//! 上游始终强制流式（`stream: true`，与 nyro/LiteLLM 一致：部分 Responses
//! 后端仅支持 SSE）；客户端请求非流式时由管线把 chunk 聚合回单个 JSON。

use std::collections::{HashMap, HashSet};

use serde_json::{Value, json};
use uuid::Uuid;

use super::{chat_max_tokens, chat_messages, collect_tool_call_names, inline_defs, message_text};
use crate::proxy::metrics::Usage;

/// 编码发往 Responses API 的请求体。
pub fn build_request_body(chat: &Value, actual_model: &str) -> Result<Value, String> {
    let tool_names = collect_tool_call_names(chat);
    let mut instructions: Vec<String> = Vec::new();
    let mut input: Vec<Value> = Vec::new();

    for message in chat_messages(chat) {
        let role = message.get("role").and_then(Value::as_str).unwrap_or("");
        let content = message.get("content");
        match role {
            "system" | "developer" => {
                let text = message_text(content);
                if !text.trim().is_empty() {
                    instructions.push(text);
                }
            }
            "user" => {
                input.push(json!({
                    "type": "message",
                    "role": "user",
                    "content": user_content(content),
                }));
            }
            "assistant" => {
                let text = message_text(content);
                if !text.is_empty() {
                    input.push(json!({
                        "type": "message",
                        "role": "assistant",
                        "content": [{"type": "output_text", "text": text}],
                    }));
                }
                if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
                    for call in tool_calls {
                        input.push(json!({
                            "type": "function_call",
                            "call_id": call.get("id").cloned().unwrap_or_else(|| json!(format!("call_{}", Uuid::new_v4()))),
                            "name": call.pointer("/function/name").cloned().unwrap_or_else(|| json!("")),
                            "arguments": call.pointer("/function/arguments").cloned().unwrap_or_else(|| json!("{}")),
                        }));
                    }
                }
            }
            "tool" => {
                let call_id = message
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .unwrap_or("call_unknown");
                input.push(json!({
                    "type": "function_call_output",
                    "call_id": call_id,
                    "output": message_text(content),
                }));
            }
            _ => {}
        }
    }
    let _ = tool_names;

    let mut body = json!({
        "model": actual_model,
        "input": Value::Array(input),
        "instructions": if instructions.is_empty() {
            json!("You are a helpful assistant.")
        } else {
            json!(instructions.join("\n\n"))
        },
        // Responses 后端普遍只支持 SSE；客户端非流式时由管线聚合。
        "stream": true,
        "store": false,
    });
    let object = body.as_object_mut().expect("object body");

    if let Some(max_tokens) = chat_max_tokens(chat) {
        object.insert("max_output_tokens".to_string(), json!(max_tokens));
    }
    if let Some(temperature) = chat.get("temperature") {
        object.insert("temperature".to_string(), temperature.clone());
    }
    if let Some(top_p) = chat.get("top_p") {
        object.insert("top_p".to_string(), top_p.clone());
    }
    if let Some(effort) = chat.get("reasoning_effort").and_then(Value::as_str)
        && !effort.is_empty()
        && effort != "none"
    {
        object.insert("reasoning".to_string(), json!({"effort": effort}));
    }
    if let Some(tools) = chat.get("tools").and_then(Value::as_array) {
        let converted: Vec<Value> = tools
            .iter()
            .filter_map(|tool| {
                let function = tool.get("function")?;
                let mut parameters = function.get("parameters").cloned().unwrap_or_else(|| json!({"type": "object"}));
                inline_defs(&mut parameters);
                Some(json!({
                    "type": "function",
                    "name": function.get("name").cloned()?,
                    "description": function.get("description").cloned().unwrap_or_else(|| json!("")),
                    "parameters": parameters,
                }))
            })
            .collect();
        if !converted.is_empty() {
            object.insert("tools".to_string(), Value::Array(converted));
        }
    }
    if let Some(choice) = chat.get("tool_choice") {
        let normalized = match choice {
            Value::String(_) => choice.clone(),
            Value::Object(_) => match choice.pointer("/function/name") {
                Some(name) => json!({"type": "function", "name": name}),
                None => Value::Null,
            },
            _ => Value::Null,
        };
        if !normalized.is_null() {
            object.insert("tool_choice".to_string(), normalized);
        }
    }
    if let Some(parallel) = chat.get("parallel_tool_calls") {
        object.insert("parallel_tool_calls".to_string(), parallel.clone());
    }
    if let Some(response_format) = chat.get("response_format")
        && response_format.is_object()
    {
        let format_type = response_format
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("");
        let format = match format_type {
            "json_object" => Some(json!({"type": "json_object"})),
            "json_schema" => {
                let mut schema = response_format
                    .pointer("/json_schema/schema")
                    .cloned()
                    .unwrap_or_else(|| json!({"type": "object"}));
                inline_defs(&mut schema);
                Some(json!({
                    "type": "json_schema",
                    "name": response_format.pointer("/json_schema/name").and_then(Value::as_str).unwrap_or("response"),
                    "schema": schema,
                }))
            }
            _ => None,
        };
        if let Some(format) = format {
            object.insert("text".to_string(), json!({"format": format}));
        }
    }
    Ok(body)
}

fn user_content(content: Option<&Value>) -> Value {
    match content {
        Some(Value::String(text)) => json!([{"type": "input_text", "text": text}]),
        Some(Value::Array(parts)) => {
            let converted: Vec<Value> = parts
                .iter()
                .filter_map(|part| match part.get("type").and_then(Value::as_str) {
                    Some("text") => part
                        .get("text")
                        .and_then(Value::as_str)
                        .map(|text| json!({"type": "input_text", "text": text})),
                    Some("image_url") => part
                        .pointer("/image_url/url")
                        .and_then(Value::as_str)
                        .map(|url| json!({"type": "input_image", "image_url": url})),
                    _ => None,
                })
                .collect();
            json!(converted)
        }
        _ => json!([]),
    }
}

/// Responses status/incomplete → OpenAI finish_reason。
fn finish_from_status(status: &str, incomplete_reason: Option<&str>) -> &'static str {
    match status {
        "completed" => "stop",
        "incomplete" => match incomplete_reason {
            Some("content_filter") => "content_filter",
            _ => "length",
        },
        _ => "stop",
    }
}

/// Responses SSE → OpenAI chunk 流转换器。
#[derive(Debug)]
pub struct ResponsesStreamConverter {
    id: String,
    model: String,
    started: bool,
    output_to_openai_index: HashMap<i64, i64>,
    next_tool_index: i64,
    streamed_args: HashSet<i64>,
    finish_reason: Option<&'static str>,
    usage: Option<Usage>,
    finished: bool,
    finish_emitted: bool,
    error: Option<String>,
}

impl ResponsesStreamConverter {
    pub fn new(_request_id: &str, requested_model: &str) -> Self {
        Self {
            id: format!("chatcmpl-{}", Uuid::new_v4()),
            model: requested_model.to_string(),
            started: false,
            output_to_openai_index: HashMap::new(),
            next_tool_index: 0,
            streamed_args: HashSet::new(),
            finish_reason: None,
            usage: None,
            finished: false,
            finish_emitted: false,
            error: None,
        }
    }

    fn ensure_started(&mut self, out: &mut Vec<Value>) {
        if !self.started {
            self.started = true;
            out.push(super::chunk_json(
                &self.id,
                &self.model,
                json!({"role": "assistant"}),
                None,
            ));
        }
    }

    fn openai_index(&mut self, output_index: i64) -> i64 {
        *self
            .output_to_openai_index
            .entry(output_index)
            .or_insert_with(|| {
                let index = self.next_tool_index;
                self.next_tool_index += 1;
                index
            })
    }

    fn emit_tool_start(
        &mut self,
        output_index: i64,
        call_id: Option<&str>,
        name: &str,
        out: &mut Vec<Value>,
    ) {
        self.ensure_started(out);
        let index = self.openai_index(output_index);
        out.push(super::chunk_json(
            &self.id,
            &self.model,
            json!({
                "tool_calls": [{
                    "index": index,
                    "id": call_id.unwrap_or("").to_string(),
                    "type": "function",
                    "function": {"name": name, "arguments": ""},
                }],
            }),
            None,
        ));
    }

    pub fn convert_event(&mut self, data: &str) -> Result<Vec<Value>, String> {
        if data == "[DONE]" {
            self.finished = true;
            return Ok(Vec::new());
        }
        let value: Value =
            serde_json::from_str(data).map_err(|e| format!("解析 Responses SSE 失败：{e}"))?;
        let mut out = Vec::new();
        match value.get("type").and_then(Value::as_str) {
            Some("response.created") | Some("response.in_progress") => {
                if let Some(id) = value.pointer("/response/id").and_then(Value::as_str) {
                    self.id = format!("chatcmpl-{id}");
                }
                if let Some(model) = value.pointer("/response/model").and_then(Value::as_str) {
                    self.model = model.to_string();
                }
                self.ensure_started(&mut out);
            }
            Some("response.output_text.delta") => {
                let text = value.get("delta").and_then(Value::as_str).unwrap_or("");
                if !text.is_empty() {
                    self.ensure_started(&mut out);
                    out.push(super::chunk_json(
                        &self.id,
                        &self.model,
                        json!({"content": text}),
                        None,
                    ));
                }
            }
            Some("response.reasoning_text.delta")
            | Some("response.reasoning_summary_text.delta") => {
                let text = value.get("delta").and_then(Value::as_str).unwrap_or("");
                if !text.is_empty() {
                    self.ensure_started(&mut out);
                    out.push(super::chunk_json(
                        &self.id,
                        &self.model,
                        json!({"reasoning_content": text}),
                        None,
                    ));
                }
            }
            Some("response.output_item.added") => {
                let item = value.get("item").cloned().unwrap_or(json!({}));
                if item.get("type").and_then(Value::as_str) == Some("function_call") {
                    let output_index = value
                        .get("output_index")
                        .and_then(Value::as_i64)
                        .unwrap_or(0);
                    let call_id = item
                        .get("call_id")
                        .or_else(|| item.get("id"))
                        .and_then(Value::as_str);
                    let name = item.get("name").and_then(Value::as_str).unwrap_or("");
                    self.emit_tool_start(output_index, call_id, name, &mut out);
                }
            }
            Some("response.function_call_arguments.delta") => {
                let output_index = value
                    .get("output_index")
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                let arguments = value.get("delta").and_then(Value::as_str).unwrap_or("");
                if !arguments.is_empty() {
                    self.ensure_started(&mut out);
                    self.streamed_args.insert(output_index);
                    let index = self.openai_index(output_index);
                    out.push(super::chunk_json(
                        &self.id,
                        &self.model,
                        json!({"tool_calls": [{"index": index, "function": {"arguments": arguments}}]}),
                        None,
                    ));
                }
            }
            Some("response.output_item.done") => {
                let item = value.get("item").cloned().unwrap_or(json!({}));
                if item.get("type").and_then(Value::as_str) == Some("function_call") {
                    let output_index = value
                        .get("output_index")
                        .and_then(Value::as_i64)
                        .unwrap_or(0);
                    // 没有 delta 流式输出时补发完整 arguments。
                    if !self.streamed_args.contains(&output_index)
                        && let Some(arguments) = item.get("arguments").and_then(Value::as_str)
                        && !arguments.is_empty()
                    {
                        self.ensure_started(&mut out);
                        let index = self.openai_index(output_index);
                        out.push(super::chunk_json(
                            &self.id,
                            &self.model,
                            json!({"tool_calls": [{"index": index, "function": {"arguments": arguments}}]}),
                            None,
                        ));
                    }
                }
            }
            Some("response.completed") => {
                if let Some(usage) = value.pointer("/response/usage") {
                    self.capture_usage(usage);
                }
                let status = value
                    .pointer("/response/status")
                    .and_then(Value::as_str)
                    .unwrap_or("completed");
                let incomplete_reason = value
                    .pointer("/response/incomplete_details/reason")
                    .and_then(Value::as_str);
                self.ensure_started(&mut out);
                self.finish_reason = Some(finish_from_status(status, incomplete_reason));
                self.finish_emitted = true;
                out.push(super::chunk_json(
                    &self.id,
                    &self.model,
                    json!({}),
                    self.finish_reason,
                ));
                self.finished = true;
            }
            Some("response.incomplete") => {
                let incomplete_reason = value
                    .pointer("/response/incomplete_details/reason")
                    .and_then(Value::as_str);
                if let Some(usage) = value.pointer("/response/usage") {
                    self.capture_usage(usage);
                }
                self.ensure_started(&mut out);
                self.finish_emitted = true;
                out.push(super::chunk_json(
                    &self.id,
                    &self.model,
                    json!({}),
                    Some(finish_from_status("incomplete", incomplete_reason)),
                ));
                self.finished = true;
            }
            Some("response.failed") => {
                let message = value
                    .pointer("/response/error/message")
                    .and_then(Value::as_str)
                    .unwrap_or("Responses upstream failed");
                self.error = Some(message.to_string());
                self.finished = true;
            }
            Some("error") => {
                self.error = Some(super::extract_error_message(data));
                self.finished = true;
            }
            _ => {}
        }
        Ok(out)
    }

    fn capture_usage(&mut self, usage: &Value) {
        let input = usage.get("input_tokens").and_then(Value::as_i64);
        let output = usage.get("output_tokens").and_then(Value::as_i64);
        let cache = usage
            .pointer("/input_tokens_details/cached_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        if input.is_some() || output.is_some() {
            self.usage = Some(Usage {
                input_tokens: input,
                cache_tokens: cache.max(0),
                output_tokens: output,
            });
        }
    }

    pub fn usage(&self) -> Option<&Usage> {
        self.usage.as_ref()
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    pub fn error(&self) -> Option<&String> {
        self.error.as_ref()
    }

    pub fn has_finish(&self) -> bool {
        self.finish_emitted
    }

    pub fn completion_id(&self) -> &str {
        &self.id
    }
}

#[cfg(test)]
mod tests {
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
}

//! OpenAI chat → Anthropic Messages 转换。
//!
//! 映射参考 nyro 与 LiteLLM：system 合并为顶层 `system`；tool_calls → tool_use；
//! role=tool → tool_result；stop → stop_sequences；reasoning_effort → thinking 预算；
//! response_format(json) → 合成 JSON 工具；max_tokens 缺省 4096。

use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value, json};
use uuid::Uuid;

use super::{
    ANTHROPIC_DEFAULT_MAX_TOKENS, chat_max_tokens, chat_messages, client_usage_json,
    collect_tool_call_names, inline_defs, message_text, reasoning_budget,
};
use crate::proxy::metrics::Usage;

/// response_format 注入的合成 JSON 工具名。
pub const JSON_TOOL_NAME: &str = "__structured_output__";

/// 编码发往 Anthropic 的请求体。返回 (body, 是否注入了 JSON 模式合成工具)。
pub fn build_request_body(chat: &Value, actual_model: &str) -> Result<(Value, bool), String> {
    let stream = chat.get("stream").and_then(Value::as_bool).unwrap_or(false);
    let max_tokens = chat_max_tokens(chat).unwrap_or(ANTHROPIC_DEFAULT_MAX_TOKENS);
    let tool_names = collect_tool_call_names(chat);

    let mut system_blocks: Vec<Value> = Vec::new();
    let mut messages: Vec<(String, Vec<Value>)> = Vec::new();

    let push_message = |messages: &mut Vec<(String, Vec<Value>)>, role: String, blocks: Vec<Value>| {
        if let Some((last_role, last_blocks)) = messages.last_mut()
            && *last_role == role
        {
            last_blocks.extend(blocks);
        } else {
            messages.push((role, blocks));
        }
    };

    for message in chat_messages(chat) {
        let role = message.get("role").and_then(Value::as_str).unwrap_or("");
        let content = message.get("content");
        match role {
            "system" | "developer" => {
                let text = message_text(content);
                if !text.trim().is_empty() {
                    system_blocks.push(json!({"type": "text", "text": text}));
                }
            }
            "user" => {
                let mut blocks = Vec::new();
                for block in user_content_blocks(content) {
                    blocks.push(block);
                }
                if blocks.is_empty() {
                    blocks.push(json!({"type": "text", "text": " "}));
                }
                push_message(&mut messages, "user".to_string(), blocks);
            }
            "assistant" => {
                let mut blocks = Vec::new();
                let text = message_text(content);
                if !text.is_empty() {
                    blocks.push(json!({"type": "text", "text": text}));
                }
                // assistant 的 reasoning_content 不回传：thinking 块需要有效的 signature。
                if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
                    for call in tool_calls {
                        let name = call.pointer("/function/name").and_then(Value::as_str).unwrap_or("");
                        let arguments = call
                            .pointer("/function/arguments")
                            .and_then(Value::as_str)
                            .unwrap_or("{}");
                        let input: Value =
                            serde_json::from_str(arguments).unwrap_or_else(|_| json!({}));
                        blocks.push(json!({
                            "type": "tool_use",
                            "id": call.get("id").cloned().unwrap_or_else(|| json!("toolu_unknown")),
                            "name": name,
                            "input": input,
                        }));
                    }
                }
                if !blocks.is_empty() {
                    push_message(&mut messages, "assistant".to_string(), blocks);
                }
            }
            "tool" => {
                let tool_use_id = message
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .unwrap_or("tool_result");
                // tool_result 仅需 tool_use_id；工具名反查保留映射能力（Anthropic 以 id 关联）。
                let _ = tool_names;
                let block = json!({
                    "type": "tool_result",
                    "tool_use_id": tool_use_id,
                    "content": message_text(content),
                });
                push_message(&mut messages, "user".to_string(), vec![block]);
            }
            _ => {}
        }
    }

    let messages: Vec<Value> = messages
        .into_iter()
        .map(|(role, blocks)| json!({"role": role, "content": blocks}))
        .collect();

    let mut tools: Vec<Value> = Vec::new();
    if let Some(list) = chat.get("tools").and_then(Value::as_array) {
        for tool in list {
            let function = tool.get("function");
            let name = function
                .and_then(|f| f.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("");
            if name.is_empty() {
                continue;
            }
            let mut input_schema = function
                .and_then(|f| f.get("parameters"))
                .cloned()
                .unwrap_or_else(|| json!({"type": "object", "properties": {}}));
            inline_defs(&mut input_schema);
            if !input_schema.is_object() {
                input_schema = json!({"type": "object", "properties": {}});
            }
            let mut schema_object: Map<String, Value> = input_schema
                .as_object()
                .cloned()
                .unwrap_or_default();
            schema_object
                .entry("type".to_string())
                .or_insert_with(|| json!("object"));
            schema_object
                .entry("properties".to_string())
                .or_insert_with(|| json!({}));
            let description = function
                .and_then(|f| f.get("description"))
                .and_then(Value::as_str)
                .unwrap_or("");
            tools.push(json!({
                "name": name,
                "description": description,
                "input_schema": Value::Object(schema_object),
            }));
        }
    }

    let mut tool_choice = map_tool_choice(chat);
    let thinking = map_thinking(chat, max_tokens);
    let mut json_mode_tool = false;

    if let Some(response_format) = chat.get("response_format") {
        let format_type = response_format.get("type").and_then(Value::as_str).unwrap_or("");
        if format_type == "json_object" || format_type == "json_schema" {
            let schema = match format_type {
                "json_object" => json!({"type": "object"}),
                _ => {
                    let mut schema = response_format
                        .pointer("/json_schema/schema")
                        .cloned()
                        .unwrap_or_else(|| json!({"type": "object"}));
                    inline_defs(&mut schema);
                    schema
                }
            };
            tools.push(json!({
                "name": JSON_TOOL_NAME,
                "description": "Respond with a JSON object matching the requested schema.",
                "input_schema": schema,
            }));
            // 不用 tool_choice 锁定：Anthropic 协议的 thinking 模式（部分上游默认开启）
            // 拒绝 tool_choice，改为 system 强指令引导模型调用该工具。
            tool_choice = None;
            system_blocks.push(json!({
                "type": "text",
                "text": format!(
                    "You must call the tool \"{JSON_TOOL_NAME}\" and provide your final answer as its arguments. Do not answer with plain text."
                ),
            }));
            json_mode_tool = true;
        }
    }

    let mut body = Map::new();
    body.insert("model".to_string(), json!(actual_model));
    body.insert("max_tokens".to_string(), json!(max_tokens));
    body.insert("messages".to_string(), Value::Array(messages));
    body.insert("stream".to_string(), json!(stream));
    if !system_blocks.is_empty() {
        body.insert("system".to_string(), Value::Array(system_blocks));
    }
    if let Some(temperature) = chat.get("temperature") {
        body.insert("temperature".to_string(), temperature.clone());
    }
    if let Some(top_p) = chat.get("top_p") {
        body.insert("top_p".to_string(), top_p.clone());
    }
    if let Some(stop_sequences) = map_stop(chat) {
        body.insert("stop_sequences".to_string(), Value::Array(stop_sequences));
    }
    if !tools.is_empty() {
        body.insert("tools".to_string(), Value::Array(tools));
    }
    if let Some(choice) = tool_choice {
        body.insert("tool_choice".to_string(), choice);
    }
    if let Some(thinking) = thinking {
        body.insert("thinking".to_string(), thinking);
    }
    Ok((Value::Object(body), json_mode_tool))
}

fn map_tool_choice(chat: &Value) -> Option<Value> {
    let mut choice = match chat.get("tool_choice") {
        Some(Value::String(s)) => match s.as_str() {
            "auto" => Some(json!({"type": "auto"})),
            "required" => Some(json!({"type": "any"})),
            "none" => Some(json!({"type": "none"})),
            _ => None,
        },
        Some(value) => {
            let name = value.pointer("/function/name").and_then(Value::as_str);
            match name {
                Some(name) => Some(json!({"type": "tool", "name": name})),
                None => None,
            }
        }
        None => None,
    };
    let parallel = chat
        .get("parallel_tool_calls")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    if !parallel {
        let base = choice.take().unwrap_or_else(|| json!({"type": "auto"}));
        let mut object = base.as_object().cloned().unwrap_or_default();
        object.insert("disable_parallel_tool_use".to_string(), json!(true));
        choice = Some(Value::Object(object));
    }
    choice
}

fn map_thinking(chat: &Value, max_tokens: i64) -> Option<Value> {
    let effort = chat.get("reasoning_effort").and_then(Value::as_str)?;
    if effort == "none" || effort.is_empty() {
        return None;
    }
    if max_tokens <= 1024 {
        return None;
    }
    let budget = reasoning_budget(effort).min(max_tokens - 1);
    Some(json!({"type": "enabled", "budget_tokens": budget}))
}

fn map_stop(chat: &Value) -> Option<Vec<Value>> {
    let sequences = match chat.get("stop") {
        Some(Value::String(s)) => vec![json!(s)],
        Some(Value::Array(items)) => items.clone(),
        _ => return None,
    };
    let filtered: Vec<Value> = sequences
        .into_iter()
        .filter(|v| v.as_str().is_some_and(|s| !s.trim().is_empty()))
        .collect();
    if filtered.is_empty() { None } else { Some(filtered) }
}

/// image_url → Anthropic image block（data: URL 转 base64，http(s) 直传 url）。
fn image_block(url: &str) -> Value {
    if let Some(rest) = url.strip_prefix("data:")
        && let Some((meta, data)) = rest.split_once(',')
    {
        let media_type = meta.strip_suffix(";base64").unwrap_or(meta);
        if !media_type.is_empty() && !data.is_empty() {
            return json!({
                "type": "image",
                "source": {"type": "base64", "media_type": media_type, "data": data},
            });
        }
    }
    json!({"type": "image", "source": {"type": "url", "url": url}})
}

fn user_content_blocks(content: Option<&Value>) -> Vec<Value> {
    match content {
        Some(Value::String(text)) => vec![json!({"type": "text", "text": text})],
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| match part.get("type").and_then(Value::as_str) {
                Some("text") => part
                    .get("text")
                    .and_then(Value::as_str)
                    .map(|text| json!({"type": "text", "text": text})),
                Some("image_url") => part
                    .pointer("/image_url/url")
                    .and_then(Value::as_str)
                    .map(image_block),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// json 模式合成工具的入参规范化：部分模型会把真正的 JSON 包在
/// `{"parameters": {...}}` 里，顶层仅有该键时解包。
pub fn unwrap_json_tool_output(value: Value) -> Value {
    if let Some(inner) = value
        .as_object()
        .filter(|object| object.len() == 1)
        .and_then(|object| object.get("parameters"))
        .filter(|inner| inner.is_object())
    {
        return inner.clone();
    }
    value
}

pub fn normalize_stop_reason(stop_reason: &str, has_tool_calls: bool) -> &'static str {
    if has_tool_calls {
        return "tool_calls";
    }
    match stop_reason {
        "end_turn" | "stop_sequence" => "stop",
        "max_tokens" => "length",
        "refusal" => "content_filter",
        "" => "stop",
        other => {
            tracing::debug!("unmapped anthropic stop_reason: {other}");
            "stop"
        }
    }
}

/// usage 归一：input_tokens + cache_read + cache_creation（含缓存的总输入）。
pub fn extract_usage(usage: &Value) -> Usage {
    let input = usage.get("input_tokens").and_then(Value::as_i64);
    let output = usage.get("output_tokens").and_then(Value::as_i64);
    let read = usage
        .get("cache_read_input_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let creation = usage
        .get("cache_creation_input_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    Usage {
        input_tokens: input.map(|input| input + read + creation),
        cache_tokens: (read + creation).max(0),
        output_tokens: output,
    }
}

/// Anthropic 非流式响应 → OpenAI chat.completion。
pub fn convert_response(
    upstream: &Value,
    request_id: &str,
    requested_model: &str,
    json_mode_tool: bool,
) -> Result<(Value, Usage), String> {
    if upstream.get("type").and_then(Value::as_str) == Some("error") {
        return Err(super::extract_error_message(
            &upstream.to_string(),
        ));
    }
    let stop_reason = upstream
        .get("stop_reason")
        .and_then(Value::as_str)
        .unwrap_or("");
    let mut text = String::new();
    let mut reasoning_parts: Vec<String> = Vec::new();
    let mut tool_calls: Vec<Value> = Vec::new();
    let mut json_tool_output: Option<String> = None;

    if let Some(blocks) = upstream.get("content").and_then(Value::as_array) {
        for block in blocks {
            match block.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if let Some(part) = block.get("text").and_then(Value::as_str) {
                        text.push_str(part);
                    }
                }
                Some("thinking") => {
                    if let Some(part) = block.get("thinking").and_then(Value::as_str) {
                        reasoning_parts.push(part.to_string());
                    }
                }
                Some("tool_use") => {
                    let name = block.get("name").and_then(Value::as_str).unwrap_or("");
                    if json_mode_tool && name == JSON_TOOL_NAME {
                        let unwrapped = unwrap_json_tool_output(
                            block.get("input").cloned().unwrap_or_else(|| json!({})),
                        );
                        json_tool_output = Some(unwrapped.to_string());
                        continue;
                    }
                    tool_calls.push(json!({
                        "id": block.get("id").cloned().unwrap_or_else(|| json!(format!("call_{}", Uuid::new_v4()))),
                        "type": "function",
                        "function": {
                            "name": name,
                            "arguments": block.get("input").map(|input| input.to_string()).unwrap_or_else(|| "{}".to_string()),
                        },
                    }));
                }
                _ => {}
            }
        }
    }

    let usage = extract_usage(upstream.get("usage").unwrap_or(&Value::Null));
    let message = if let Some(json_output) = json_tool_output {
        json!({"role": "assistant", "content": json_output})
    } else {
        let mut message = Map::new();
        message.insert("role".to_string(), json!("assistant"));
        message.insert(
            "content".to_string(),
            if text.is_empty() && tool_calls.is_empty() {
                Value::Null
            } else {
                json!(text)
            },
        );
        if !tool_calls.is_empty() {
            message.insert("tool_calls".to_string(), Value::Array(tool_calls.clone()));
        }
        if !reasoning_parts.is_empty() {
            message.insert("reasoning_content".to_string(), json!(reasoning_parts.join("\n")));
        }
        Value::Object(message)
    };

    let has_tool_calls = message.get("tool_calls").is_some();
    let completion = json!({
        "id": upstream.get("id").and_then(Value::as_str).map(|s| s.to_string()).unwrap_or_else(|| request_id.to_string()),
        "object": "chat.completion",
        "created": chrono::Utc::now().timestamp(),
        "model": requested_model,
        "choices": [{
            "index": 0,
            "message": message,
            "finish_reason": normalize_stop_reason(stop_reason, has_tool_calls),
        }],
        "usage": client_usage_json(&usage),
    });
    Ok((completion, usage))
}

/// Anthropic SSE → OpenAI chunk 流转换器。
#[derive(Debug)]
pub struct AnthropicStreamConverter {
    id: String,
    model: String,
    started: bool,
    anthropic_to_openai_tool_index: HashMap<i64, i64>,
    next_tool_index: i64,
    json_mode_tool: bool,
    json_mode_indexes: HashSet<i64>,
    json_mode_buffers: HashMap<i64, String>,
    usage: Option<Usage>,
    finished: bool,
    finish_emitted: bool,
    error: Option<String>,
}

impl AnthropicStreamConverter {
    pub fn new(_request_id: &str, requested_model: &str, json_mode_tool: bool) -> Self {
        Self {
            id: format!("chatcmpl-{}", Uuid::new_v4()),
            model: requested_model.to_string(),
            started: false,
            anthropic_to_openai_tool_index: HashMap::new(),
            next_tool_index: 0,
            json_mode_tool,
            json_mode_indexes: HashSet::new(),
            json_mode_buffers: HashMap::new(),
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

    fn openai_tool_index(&mut self, anthropic_index: i64) -> i64 {
        *self
            .anthropic_to_openai_tool_index
            .entry(anthropic_index)
            .or_insert_with(|| {
                let index = self.next_tool_index;
                self.next_tool_index += 1;
                index
            })
    }

    pub fn convert_event(&mut self, data: &str) -> Result<Vec<Value>, String> {
        if data == "[DONE]" {
            self.finished = true;
            return Ok(Vec::new());
        }
        let value: Value = serde_json::from_str(data)
            .map_err(|e| format!("解析 Anthropic SSE 失败：{e}"))?;
        let mut out = Vec::new();
        match value.get("type").and_then(Value::as_str) {
            Some("error") => {
                self.error = Some(super::extract_error_message(data));
                self.finished = true;
            }
            Some("message_start") => {
                if let Some(usage) = value.pointer("/message/usage") {
                    let extracted = extract_usage(usage);
                    if extracted.input_tokens.is_some() {
                        self.usage = Some(extracted);
                    }
                }
                self.ensure_started(&mut out);
            }
            Some("content_block_start") => {
                let block_index = value.get("index").and_then(Value::as_i64).unwrap_or(0);
                let block = value.get("content_block").cloned().unwrap_or(json!({}));
                let block_type = block.get("type").and_then(Value::as_str).unwrap_or("");
                if block_type == "tool_use" {
                    let name = block.get("name").and_then(Value::as_str).unwrap_or("");
                    if self.json_mode_tool && name == JSON_TOOL_NAME {
                        self.json_mode_indexes.insert(block_index);
                        return Ok(out);
                    }
                    self.ensure_started(&mut out);
                    let openai_index = self.openai_tool_index(block_index);
                    out.push(super::chunk_json(
                        &self.id,
                        &self.model,
                        json!({
                            "tool_calls": [{
                                "index": openai_index,
                                "id": block.get("id").cloned().unwrap_or_else(|| json!(format!("call_{}", Uuid::new_v4()))),
                                "type": "function",
                                "function": {"name": name, "arguments": ""},
                            }],
                        }),
                        None,
                    ));
                }
            }
            Some("content_block_delta") => {
                let block_index = value.get("index").and_then(Value::as_i64).unwrap_or(0);
                let delta = value.get("delta").cloned().unwrap_or(json!({}));
                match delta.get("type").and_then(Value::as_str) {
                    Some("text_delta") => {
                        let text = delta.get("text").and_then(Value::as_str).unwrap_or("");
                        if !text.is_empty() {
                            self.ensure_started(&mut out);
                            out.push(super::chunk_json(&self.id, &self.model, json!({"content": text}), None));
                        }
                    }
                    Some("thinking_delta") => {
                        let text = delta.get("thinking").and_then(Value::as_str).unwrap_or("");
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
                    Some("input_json_delta") => {
                        let arguments = delta.get("partial_json").and_then(Value::as_str).unwrap_or("");
                        if !arguments.is_empty() {
                            if self.json_mode_indexes.contains(&block_index) {
                                // json 模式内容缓冲到流结束，解包后再一次性发出。
                                self.json_mode_buffers
                                    .entry(block_index)
                                    .or_default()
                                    .push_str(arguments);
                            } else {
                                self.ensure_started(&mut out);
                                let openai_index = self.openai_tool_index(block_index);
                                out.push(super::chunk_json(
                                    &self.id,
                                    &self.model,
                                    json!({"tool_calls": [{"index": openai_index, "function": {"arguments": arguments}}]}),
                                    None,
                                ));
                            }
                        }
                    }
                    _ => {}
                }
            }
            Some("message_delta") => {
                if let Some(usage) = value.get("usage") {
                    let extracted = extract_usage(usage);
                    // output_tokens 在 message_delta 才出现；与 message_start 的 input 合并。
                    let merged = match self.usage.take() {
                        Some(previous) => Usage {
                            input_tokens: extracted.input_tokens.or(previous.input_tokens),
                            cache_tokens: if extracted.cache_tokens > 0 {
                                extracted.cache_tokens
                            } else {
                                previous.cache_tokens
                            },
                            output_tokens: extracted.output_tokens.or(previous.output_tokens),
                        },
                        None => extracted,
                    };
                    self.usage = Some(merged);
                }
                let stop_reason = value
                    .pointer("/delta/stop_reason")
                    .and_then(Value::as_str)
                    .unwrap_or("end_turn");
                self.ensure_started(&mut out);
                // 输出缓冲的 json 模式内容（解包 parameters 包装）。
                let buffers: Vec<String> = self
                    .json_mode_buffers
                    .values()
                    .map(|buffer| {
                        let parsed: Value =
                            serde_json::from_str(buffer).unwrap_or_else(|_| json!({}));
                        unwrap_json_tool_output(parsed).to_string()
                    })
                    .collect();
                for content in buffers {
                    out.push(super::chunk_json(&self.id, &self.model, json!({"content": content}), None));
                }
                self.finish_emitted = true;
                out.push(super::chunk_json(
                    &self.id,
                    &self.model,
                    json!({}),
                    Some(normalize_stop_reason(stop_reason, self.next_tool_index > 0)),
                ));
            }
            Some("message_stop") => {
                self.finished = true;
            }
            _ => {}
        }
        Ok(out)
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
        let (body, json_mode) = build_request_body(&chat, "claude-x").unwrap();
        assert!(!json_mode);
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
    fn maps_stop_tool_choice_and_thinking() {
        let chat = from_str::<Value>(
            r#"{"model":"m","messages":[{"role":"user","content":"x"}],"stop":["END", "  "],"tool_choice":"required","parallel_tool_calls":false,"reasoning_effort":"high","max_tokens":4096}"#,
        )
        .unwrap();
        let (body, _) = build_request_body(&chat, "claude-x").unwrap();
        assert_eq!(body["stop_sequences"], json!(["END"]));
        assert_eq!(body["tool_choice"]["type"], "any");
        assert_eq!(body["tool_choice"]["disable_parallel_tool_use"], true);
        assert_eq!(body["thinking"]["budget_tokens"], 4095);
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
    fn defaults_max_tokens_to_4096() {
        let chat = from_str::<Value>(r#"{"model":"m","messages":[{"role":"user","content":"x"}]}"#).unwrap();
        let (body, _) = build_request_body(&chat, "claude-x").unwrap();
        assert_eq!(body["max_tokens"], 4096);
    }

    #[test]
    fn response_format_becomes_synthetic_tool() {
        let chat = from_str::<Value>(
            r#"{"model":"m","messages":[{"role":"user","content":"x"}],"response_format":{"type":"json_schema","json_schema":{"schema":{"type":"object","properties":{"a":{"type":"string"}}}}}}"#,
        )
        .unwrap();
        let (body, json_mode) = build_request_body(&chat, "claude-x").unwrap();
        assert!(json_mode);
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
        let (body, json_mode) = build_request_body(&chat, "claude-x").unwrap();
        assert!(json_mode);
        assert!(body["thinking"].is_object(), "thinking 与 json 模式可共存");
        assert!(body.get("tool_choice").is_none());
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
        assert_eq!(completion["choices"][0]["message"]["content"], "hello");
        assert_eq!(completion["choices"][0]["message"]["reasoning_content"], "hmm");
        assert_eq!(completion["choices"][0]["message"]["tool_calls"][0]["id"], "toolu_1");
        assert_eq!(usage.input_tokens, Some(15));
        assert_eq!(usage.cache_tokens, 5);
        assert_eq!(usage.output_tokens, Some(5));
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
        assert_eq!(chunks.last().unwrap()["choices"][0]["finish_reason"], "stop");
    }
}

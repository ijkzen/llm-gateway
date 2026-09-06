//! OpenAI chat → OpenAI Responses API 转换。
//!
//! 上游始终强制流式（`stream: true`，与 nyro/LiteLLM 一致：部分 Responses
//! 后端仅支持 SSE）；客户端请求非流式时由管线把 chunk 聚合回单个 JSON。

use std::collections::{HashMap, HashSet};

use serde_json::{Value, json};
use uuid::Uuid;

use super::{
    ChatReasoning, chat_max_tokens, chat_messages, chat_reasoning, collect_tool_call_names,
    inline_defs, message_text,
};

/// OpenAI Responses 官方 effort 枚举（none/minimal/low/medium/high/xhigh）之外的
/// 值（如 ZCode 风格的 max）原样透传会被上游 400：钳制到最近合法档。
fn clamp_responses_effort(effort: &str) -> &str {
    match effort {
        "none" | "minimal" | "low" | "medium" | "high" | "xhigh" => effort,
        "max" => "xhigh",
        other => {
            tracing::debug!(
                effort = other,
                "未知 reasoning effort，Responses 上游按 high 钳制"
            );
            "high"
        }
    }
}
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
                // 回传的 reasoning item 置于其产出项之前，满足 Responses
                // 「reasoning 后必须紧跟对应输出项」的结构校验。
                for item in responses_reasoning_items(message) {
                    input.push(item);
                }
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
        // Responses 后端普遍只支持 SSE；客户端非流式时由管线聚合。
        "stream": true,
        "store": false,
        // store:false 下不显式申请就拿不到 reasoning 密文，透传回传无从谈起。
        "include": ["reasoning.encrypted_content"],
    });
    let object = body.as_object_mut().expect("object body");
    // instructions 可选：客户端没写 system 时不应注入默认值（LiteLLM 同款）。
    if !instructions.is_empty() {
        object.insert("instructions".to_string(), json!(instructions.join("\n\n")));
    }

    if let Some(max_tokens) = chat_max_tokens(chat) {
        object.insert("max_output_tokens".to_string(), json!(max_tokens));
    }
    if let Some(temperature) = chat.get("temperature") {
        object.insert("temperature".to_string(), temperature.clone());
    }
    if let Some(top_p) = chat.get("top_p") {
        object.insert("top_p".to_string(), top_p.clone());
    }
    if let Some(top_k) = chat.get("top_k") {
        object.insert("top_k".to_string(), top_k.clone());
    }
    match chat_reasoning(chat) {
        ChatReasoning::Enabled(reasoning) => {
            let effort = clamp_responses_effort(&reasoning.effort);
            object.insert("reasoning".to_string(), json!({"effort": effort}));
        }
        // 明确关闭：Responses 缺省按默认档位思考，必须显式写 none 才关得掉。
        ChatReasoning::Disabled => {
            object.insert("reasoning".to_string(), json!({"effort": "none"}));
        }
        ChatReasoning::Unspecified => {}
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

/// assistant 消息上回传的 openai-responses 格式 details → reasoning item。
/// encrypted_content 是 store:false 下唯一可回传载体，缺失即跳过。
fn responses_reasoning_items(message: &Value) -> Vec<Value> {
    super::valid_reasoning_details(message)
        .into_iter()
        .filter(|detail| {
            detail.get("format").and_then(Value::as_str) == Some(super::REASONING_FORMAT_RESPONSES)
        })
        .filter(|detail| detail.get("type").and_then(Value::as_str) == Some("reasoning.encrypted"))
        .filter_map(|detail| {
            let encrypted = detail.get("data").and_then(Value::as_str)?;
            (!encrypted.is_empty()).then(|| {
                json!({
                    "type": "reasoning",
                    "id": detail.get("id").cloned().unwrap_or(Value::Null),
                    "summary": [],
                    "encrypted_content": encrypted,
                })
            })
        })
        .collect()
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

/// 从 response 对象推导 finish_reason：output 含 function_call 项时必须是
/// "tool_calls"（OpenAI 客户端工具循环依赖该语义，审计 C1），否则按 status。
fn finish_from_response(response: &Value) -> &'static str {
    let has_function_call = response
        .get("output")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items
                .iter()
                .any(|item| item.get("type").and_then(Value::as_str) == Some("function_call"))
        });
    if has_function_call {
        return "tool_calls";
    }
    let status = response
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("completed");
    let incomplete_reason = response
        .pointer("/incomplete_details/reason")
        .and_then(Value::as_str);
    finish_from_status(status, incomplete_reason)
}

/// Responses SSE → OpenAI chunk 流转换器。
#[derive(Debug)]
pub struct ResponsesStreamConverter {
    id: String,
    model: String,
    started: bool,
    output_to_openai_index: HashMap<i64, i64>,
    next_tool_index: i64,
    streamed_text: HashMap<i64, String>,
    streamed_reasoning: HashMap<i64, String>,
    streamed_args: HashMap<i64, String>,
    /// 已捕获 encrypted_content detail 的 output_index（output_item.done 与
    /// completed 回放双路径去重）。
    reasoning_detail_captured: HashSet<i64>,
    next_detail_index: i64,
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
            streamed_text: HashMap::new(),
            streamed_reasoning: HashMap::new(),
            streamed_args: HashMap::new(),
            reasoning_detail_captured: HashSet::new(),
            next_detail_index: 0,
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

    fn emit_delta(&mut self, output_index: i64, text: &str, reasoning: bool, out: &mut Vec<Value>) {
        if text.is_empty() {
            return;
        }
        let emitted = if reasoning {
            &mut self.streamed_reasoning
        } else {
            &mut self.streamed_text
        };
        emitted.entry(output_index).or_default().push_str(text);
        self.ensure_started(out);
        let delta = if reasoning {
            json!({"reasoning_content": text})
        } else {
            json!({"content": text})
        };
        out.push(super::chunk_json(&self.id, &self.model, delta, None));
    }

    fn missing_suffix(emitted: &mut HashMap<i64, String>, output_index: i64, text: &str) -> String {
        let previous = emitted.entry(output_index).or_default();
        let missing = text
            .strip_prefix(previous.as_str())
            .map_or_else(|| text.to_string(), str::to_string);
        *previous = text.to_string();
        missing
    }

    fn emit_missing_text(&mut self, output_index: i64, text: &str, out: &mut Vec<Value>) {
        let missing = Self::missing_suffix(&mut self.streamed_text, output_index, text);
        if missing.is_empty() {
            return;
        }
        self.ensure_started(out);
        out.push(super::chunk_json(
            &self.id,
            &self.model,
            json!({"content": missing}),
            None,
        ));
    }

    fn emit_missing_reasoning(&mut self, output_index: i64, text: &str, out: &mut Vec<Value>) {
        let missing = Self::missing_suffix(&mut self.streamed_reasoning, output_index, text);
        if missing.is_empty() {
            return;
        }
        self.ensure_started(out);
        out.push(super::chunk_json(
            &self.id,
            &self.model,
            json!({"reasoning_content": missing}),
            None,
        ));
    }

    fn emit_missing_arguments(&mut self, output_index: i64, item: &Value, out: &mut Vec<Value>) {
        let Some(arguments) = item.get("arguments").and_then(Value::as_str) else {
            return;
        };
        let missing = Self::missing_suffix(&mut self.streamed_args, output_index, arguments);
        if missing.is_empty() {
            return;
        }
        self.ensure_started(out);
        let index = self.openai_index(output_index);
        out.push(super::chunk_json(
            &self.id,
            &self.model,
            json!({"tool_calls": [{"index": index, "function": {"arguments": missing}}]}),
            None,
        ));
    }

    fn reasoning_summary(value: &Value) -> String {
        value
            .get("summary")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|entry| entry.get("text").and_then(Value::as_str))
            .collect()
    }

    fn emit_final_item(&mut self, output_index: i64, item: &Value, out: &mut Vec<Value>) {
        match item.get("type").and_then(Value::as_str) {
            Some("function_call") => self.emit_missing_arguments(output_index, item, out),
            Some("message") => {
                let Some(content) = item.get("content").and_then(Value::as_array) else {
                    return;
                };
                for part in content {
                    match part.get("type").and_then(Value::as_str) {
                        Some("output_text") => {
                            if let Some(text) = part.get("text").and_then(Value::as_str) {
                                self.emit_missing_text(output_index, text, out);
                            }
                        }
                        Some("refusal") => {
                            if let Some(text) = part.get("refusal").and_then(Value::as_str) {
                                self.emit_missing_text(output_index, text, out);
                            }
                        }
                        Some("reasoning") => {
                            let summary = Self::reasoning_summary(part);
                            self.emit_missing_reasoning(output_index, &summary, out);
                        }
                        _ => {}
                    }
                }
            }
            Some("reasoning") => {
                let summary = Self::reasoning_summary(item);
                self.emit_missing_reasoning(output_index, &summary, out);
                // encrypted_content 是 store:false 下回传 reasoning 的唯一载体，
                // 缺失（上游未提供）时跳过，仅保留 summary 展示。
                if let Some(encrypted) = item.get("encrypted_content").and_then(Value::as_str)
                    && !encrypted.is_empty()
                    && self.reasoning_detail_captured.insert(output_index)
                {
                    let detail = super::reasoning_encrypted_detail(
                        super::REASONING_FORMAT_RESPONSES,
                        self.next_detail_index,
                        item.get("id").and_then(Value::as_str),
                        json!(encrypted),
                    );
                    self.next_detail_index += 1;
                    self.ensure_started(out);
                    out.push(super::chunk_json(
                        &self.id,
                        &self.model,
                        json!({"reasoning_details": [detail]}),
                        None,
                    ));
                }
            }
            _ => {}
        }
    }

    fn emit_final_output(&mut self, response: &Value, out: &mut Vec<Value>) {
        let Some(output) = response.get("output").and_then(Value::as_array) else {
            return;
        };
        for (index, item) in output.iter().enumerate() {
            self.emit_final_item(index as i64, item, out);
        }
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
                // model 保持客户端请求的别名，不跟随上游实际模型名
                // （与 Anthropic/Gemini 路径口径一致）。
                self.ensure_started(&mut out);
            }
            Some("response.output_text.delta") => {
                let output_index = value
                    .get("output_index")
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                let text = value.get("delta").and_then(Value::as_str).unwrap_or("");
                self.emit_delta(output_index, text, false, &mut out);
            }
            Some("response.refusal.delta") => {
                let output_index = value
                    .get("output_index")
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                let text = value.get("delta").and_then(Value::as_str).unwrap_or("");
                self.emit_delta(output_index, text, false, &mut out);
            }
            Some("response.refusal.done") => {
                let output_index = value
                    .get("output_index")
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                let text = value.get("refusal").and_then(Value::as_str).unwrap_or("");
                self.emit_missing_text(output_index, text, &mut out);
            }
            Some("response.reasoning_text.delta")
            | Some("response.reasoning_summary_text.delta") => {
                let output_index = value
                    .get("output_index")
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                let text = value.get("delta").and_then(Value::as_str).unwrap_or("");
                self.emit_delta(output_index, text, true, &mut out);
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
                    self.streamed_args
                        .entry(output_index)
                        .or_default()
                        .push_str(arguments);
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
            Some("response.output_item.done") => {
                let output_index = value
                    .get("output_index")
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                let item = value.get("item").cloned().unwrap_or(json!({}));
                self.emit_final_item(output_index, &item, &mut out);
            }
            Some("response.completed") => {
                let response = value.get("response").cloned().unwrap_or(json!({}));
                self.emit_final_output(&response, &mut out);
                if let Some(usage) = response.get("usage") {
                    self.capture_usage(usage);
                }
                self.ensure_started(&mut out);
                self.finish_reason = Some(finish_from_response(&response));
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
                let response = value.get("response").cloned().unwrap_or(json!({}));
                self.emit_final_output(&response, &mut out);
                if let Some(usage) = response.get("usage") {
                    self.capture_usage(usage);
                }
                self.ensure_started(&mut out);
                self.finish_emitted = true;
                out.push(super::chunk_json(
                    &self.id,
                    &self.model,
                    json!({}),
                    Some(finish_from_response(&response)),
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
        if let Some(usage) = Self::extract_usage(usage) {
            self.usage = Some(usage);
        }
    }

    /// 从 Responses `usage` 对象提取归一 usage；无 input/output token 时返回 None。
    pub fn extract_usage(usage: &Value) -> Option<Usage> {
        let input = usage.get("input_tokens").and_then(Value::as_i64);
        let output = usage.get("output_tokens").and_then(Value::as_i64);
        let cache = usage
            .pointer("/input_tokens_details/cached_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let reasoning = usage
            .pointer("/output_tokens_details/reasoning_tokens")
            .and_then(Value::as_i64);
        if input.is_some() || output.is_some() {
            Some(Usage {
                input_tokens: input,
                cache_tokens: cache.max(0),
                output_tokens: output,
                reasoning_tokens: reasoning,
            })
        } else {
            None
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

    pub fn completion_model(&self) -> &str {
        &self.model
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
        let chat = from_str::<Value>(r#"{"model":"m","messages":[{"role":"user","content":"x"}]}"#)
            .unwrap();
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
        let chat = from_str::<Value>(r#"{"model":"m","messages":[{"role":"user","content":"x"}]}"#)
            .unwrap();
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
        let chat = from_str::<Value>(r#"{"model":"m","messages":[{"role":"user","content":"x"}]}"#)
            .unwrap();
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

        let chat = from_str::<Value>(r#"{"model":"m","messages":[{"role":"user","content":"x"}]}"#)
            .unwrap();
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
        let chat = from_str::<Value>(
            r#"{"model":"m","messages":[{"role":"user","content":"x"}],"top_k":40}"#,
        )
        .unwrap();
        let body = build_request_body(&chat, "gpt-5").unwrap();
        assert_eq!(body["top_k"], 40);
    }
}

//! OpenAI chat → Gemini generateContent / streamGenerateContent 转换。
//!
//! 映射参考 nyro 与 LiteLLM：system → systemInstruction；assistant → role "model"；
//! tool 结果 → functionResponse（name 用工具名，从上文 tool_call 反查）；
//! finishReason 全表映射（LiteLLM）；cachedContentTokenCount 计入缓存指标。

use std::collections::HashMap;

use serde_json::{Map, Value, json};
use uuid::Uuid;

use super::{chat_max_tokens, chat_messages, collect_tool_call_names, inline_defs, message_text};
use crate::proxy::metrics::Usage;

/// 生成 Gemini URL 的 action 后缀（generateContent / streamGenerateContent）。
pub fn generate_action(stream: bool) -> String {
    if stream {
        "streamGenerateContent?alt=sse".to_string()
    } else {
        "generateContent".to_string()
    }
}

/// 编码发往 Gemini 的请求体。
pub fn build_request_body(chat: &Value, _actual_model: &str) -> Result<Value, String> {
    let tool_names = collect_tool_call_names(chat);
    let mut system_parts: Vec<Value> = Vec::new();
    let mut contents: Vec<(String, Vec<Value>)> = Vec::new();

    let push_contents =
        |contents: &mut Vec<(String, Vec<Value>)>, role: String, parts: Vec<Value>| {
            if let Some((last_role, last_parts)) = contents.last_mut()
                && *last_role == role
            {
                last_parts.extend(parts);
            } else {
                contents.push((role, parts));
            }
        };

    for message in chat_messages(chat) {
        let role = message.get("role").and_then(Value::as_str).unwrap_or("");
        let content = message.get("content");
        match role {
            "system" | "developer" => {
                let text = message_text(content);
                if !text.trim().is_empty() {
                    system_parts.push(json!({"text": text}));
                }
            }
            "user" => {
                let mut parts = Vec::new();
                for part in user_parts(content) {
                    parts.push(part);
                }
                if parts.is_empty() {
                    parts.push(json!({"text": " "}));
                }
                push_contents(&mut contents, "user".to_string(), parts);
            }
            "assistant" => {
                let mut parts = Vec::new();
                let text = message_text(content);
                if !text.is_empty() {
                    parts.push(json!({"text": text}));
                }
                if let Some(reasoning) = message.get("reasoning_content").and_then(Value::as_str)
                    && !reasoning.is_empty()
                {
                    parts.push(json!({"text": reasoning, "thought": true}));
                }
                if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
                    for call in tool_calls {
                        let arguments = call
                            .pointer("/function/arguments")
                            .and_then(Value::as_str)
                            .unwrap_or("{}");
                        let args: Value =
                            serde_json::from_str(arguments).unwrap_or_else(|_| json!({}));
                        parts.push(json!({
                            "functionCall": {
                                "name": call.pointer("/function/name").and_then(Value::as_str).unwrap_or(""),
                                "args": args,
                            },
                        }));
                    }
                }
                if !parts.is_empty() {
                    push_contents(&mut contents, "model".to_string(), parts);
                }
            }
            "tool" => {
                let tool_call_id = message
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown_tool");
                let name = tool_names
                    .get(tool_call_id)
                    .cloned()
                    .unwrap_or_else(|| tool_call_id.to_string());
                let raw = message_text(content);
                let result: Value =
                    serde_json::from_str(&raw).unwrap_or_else(|_| json!({"result": raw}));
                push_contents(
                    &mut contents,
                    "user".to_string(),
                    vec![json!({"functionResponse": {"name": name, "response": result}})],
                );
            }
            _ => {}
        }
    }

    let contents: Vec<Value> = contents
        .into_iter()
        .map(|(role, parts)| json!({"role": role, "parts": parts}))
        .collect();

    let mut body = Map::new();
    body.insert("contents".to_string(), Value::Array(contents));
    if !system_parts.is_empty() {
        body.insert(
            "systemInstruction".to_string(),
            json!({"parts": system_parts}),
        );
    }

    let mut generation_config = Map::new();
    if let Some(max_tokens) = chat_max_tokens(chat) {
        generation_config.insert("maxOutputTokens".to_string(), json!(max_tokens));
    }
    if let Some(temperature) = chat.get("temperature").and_then(Value::as_f64) {
        generation_config.insert("temperature".to_string(), json!(temperature));
    }
    if let Some(top_p) = chat.get("top_p").and_then(Value::as_f64) {
        generation_config.insert("topP".to_string(), json!(top_p));
    }
    if let Some(seed) = chat.get("seed").and_then(Value::as_i64) {
        generation_config.insert("seed".to_string(), json!(seed));
    }
    if let Some(stop) = chat.get("stop") {
        let sequences = match stop {
            Value::String(s) => vec![json!(s)],
            Value::Array(items) => items.clone(),
            _ => Vec::new(),
        };
        if !sequences.is_empty() {
            generation_config.insert("stopSequences".to_string(), Value::Array(sequences));
        }
    }
    if let Some(response_format) = chat.get("response_format")
        && response_format.is_object()
    {
        let format_type = response_format
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("");
        if format_type == "json_object" {
            generation_config.insert("responseMimeType".to_string(), json!("application/json"));
        } else if format_type == "json_schema" {
            let mut schema = response_format
                .pointer("/json_schema/schema")
                .cloned()
                .unwrap_or_else(|| json!({"type": "object"}));
            sanitize_gemini_schema(&mut schema);
            generation_config.insert("responseMimeType".to_string(), json!("application/json"));
            generation_config.insert("responseJsonSchema".to_string(), schema);
        }
    }
    if !generation_config.is_empty() {
        body.insert(
            "generationConfig".to_string(),
            Value::Object(generation_config),
        );
    }

    if let Some(tools) = chat.get("tools").and_then(Value::as_array) {
        let declarations: Vec<Value> = tools
            .iter()
            .filter_map(|tool| {
                let function = tool.get("function")?;
                let name = function.get("name").and_then(Value::as_str)?;
                let mut parameters = function
                    .get("parameters")
                    .cloned()
                    .unwrap_or_else(|| json!({"type": "OBJECT", "properties": {}}));
                sanitize_gemini_schema(&mut parameters);
                Some(json!({
                    "name": name,
                    "description": function.get("description").and_then(Value::as_str).unwrap_or(""),
                    "parameters": parameters,
                }))
            })
            .collect();
        if !declarations.is_empty() {
            body.insert(
                "tools".to_string(),
                json!([{ "functionDeclarations": declarations }]),
            );
        }
    }
    if let Some(choice) = chat.get("tool_choice") {
        let config = match choice {
            Value::String(s) => match s.as_str() {
                "none" => Some(json!({"mode": "NONE"})),
                "required" => Some(json!({"mode": "ANY"})),
                "auto" => Some(json!({"mode": "AUTO"})),
                _ => None,
            },
            Value::Object(_) => choice
                .pointer("/function/name")
                .and_then(Value::as_str)
                .map(|name| json!({"mode": "ANY", "allowedFunctionNames": [name]})),
            _ => None,
        };
        if let Some(config) = config {
            body.insert(
                "toolConfig".to_string(),
                json!({"functionCallingConfig": config}),
            );
        }
    }

    Ok(Value::Object(body))
}

/// 清洗 JSON Schema 为 Gemini Schema 兼容：内联 $ref、类型大写、去掉不支持的键与非法 format。
pub fn sanitize_gemini_schema(schema: &mut Value) {
    inline_defs(schema);
    sanitize_node(schema, 0);
}

const GEMINI_SCHEMA_KEYS: &[&str] = &[
    "type",
    "format",
    "description",
    "nullable",
    "enum",
    "items",
    "properties",
    "required",
    "minimum",
    "maximum",
    "minItems",
    "maxItems",
    "minProperties",
    "maxProperties",
    "minLength",
    "maxLength",
    "pattern",
    "example",
    "anyOf",
    "propertyOrdering",
    "default",
    "title",
];

fn sanitize_node(value: &mut Value, depth: usize) {
    if depth > 16 {
        return;
    }
    match value {
        Value::Object(map) => {
            // 类型数组（如 ["string","null"]）取首个非 null 类型。
            if let Some(Value::Array(types)) = map.get("type")
                && let Some(first) = types
                    .iter()
                    .find(|t| t.as_str().map(|s| s != "null").unwrap_or(false))
                    .cloned()
            {
                map.insert("type".to_string(), first);
                map.insert("nullable".to_string(), json!(true));
            }
            // 类型名转大写（OpenAPI 风格）。
            if let Some(type_name) = map
                .get("type")
                .and_then(Value::as_str)
                .map(str::to_uppercase)
            {
                map.insert("type".to_string(), json!(type_name));
            }
            // format 只保留 Gemini 接受的值。
            if let Some(format) = map
                .get("format")
                .and_then(Value::as_str)
                .map(str::to_string)
            {
                let type_name = map.get("type").and_then(Value::as_str).unwrap_or("");
                let allowed = match type_name {
                    "STRING" => matches!(format.as_str(), "enum" | "date-time"),
                    "NUMBER" | "INTEGER" => {
                        matches!(format.as_str(), "float" | "double" | "int32" | "int64")
                    }
                    _ => false,
                };
                if !allowed {
                    map.remove("format");
                }
            }
            // properties 是「名称 → schema」映射，子项按 schema 清洗但不能对映射本身做键过滤。
            if let Some(Value::Object(properties)) = map.get_mut("properties") {
                for (_, property_schema) in properties.iter_mut() {
                    sanitize_node(property_schema, depth + 1);
                }
            }
            if let Some(items) = map.get_mut("items") {
                sanitize_node(items, depth + 1);
            }
            if let Some(Value::Array(any_of)) = map.get_mut("anyOf") {
                for branch in any_of.iter_mut() {
                    sanitize_node(branch, depth + 1);
                }
            }
            map.retain(|key, _| GEMINI_SCHEMA_KEYS.contains(&key.as_str()));
            // 空的 properties 移除，避免 Gemini 报错。
            if let Some(Value::Object(properties)) = map.get("properties")
                && properties.is_empty()
            {
                map.remove("properties");
            }
        }
        Value::Array(items) => {
            for child in items.iter_mut() {
                sanitize_node(child, depth + 1);
            }
        }
        _ => {}
    }
}

fn user_parts(content: Option<&Value>) -> Vec<Value> {
    match content {
        Some(Value::String(text)) => vec![json!({"text": text})],
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| match part.get("type").and_then(Value::as_str) {
                Some("text") => part
                    .get("text")
                    .and_then(Value::as_str)
                    .map(|text| json!({"text": text})),
                Some("image_url") => part
                    .pointer("/image_url/url")
                    .and_then(Value::as_str)
                    .and_then(image_part),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn image_part(url: &str) -> Option<Value> {
    if let Some(rest) = url.strip_prefix("data:")
        && let Some((meta, data)) = rest.split_once(',')
    {
        let media_type = meta.strip_suffix(";base64").unwrap_or(meta);
        if !media_type.is_empty() && !data.is_empty() {
            return Some(json!({"inlineData": {"mimeType": media_type, "data": data}}));
        }
    }
    Some(json!({"fileData": {"fileUri": url}}))
}

/// Gemini finishReason → OpenAI finish_reason（LiteLLM 全表）。
pub fn map_finish_reason(reason: &str, has_tool_calls: bool) -> &'static str {
    if has_tool_calls {
        return "tool_calls";
    }
    match reason {
        "STOP"
        | "FINISH_REASON_UNSPECIFIED"
        | "MALFORMED_FUNCTION_CALL"
        | "TOO_MANY_TOOL_CALLS"
        | "MALFORMED_RESPONSE" => "stop",
        "MAX_TOKENS" => "length",
        "SAFETY"
        | "RECITATION"
        | "BLOCKLIST"
        | "PROHIBITED_CONTENT"
        | "SPII"
        | "IMAGE_SAFETY"
        | "IMAGE_PROHIBITED_CONTENT"
        | "LANGUAGE"
        | "OTHER" => "content_filter",
        "" => "stop",
        other => {
            tracing::debug!("unmapped gemini finishReason: {other}");
            "stop"
        }
    }
}

/// usageMetadata → 归一 usage：输出 = candidates + thoughts（兜底 total − prompt）。
pub fn extract_usage(usage: &Value) -> Usage {
    let prompt = usage.get("promptTokenCount").and_then(Value::as_i64);
    let candidates = usage.get("candidatesTokenCount").and_then(Value::as_i64);
    let thoughts = usage
        .get("thoughtsTokenCount")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let total = usage.get("totalTokenCount").and_then(Value::as_i64);
    let output = match (candidates, total, prompt) {
        (Some(candidates), _, _) => Some(candidates + thoughts),
        (None, Some(total), Some(prompt)) => Some((total - prompt).max(0)),
        (None, Some(total), None) => Some(total),
        _ => None,
    };
    Usage {
        input_tokens: prompt,
        cache_tokens: usage
            .get("cachedContentTokenCount")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            .max(0),
        output_tokens: output,
    }
}

fn parts_to_message(parts: &[Value]) -> (String, String, Vec<Value>) {
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut tool_calls: Vec<Value> = Vec::new();
    for part in parts {
        if part.get("functionCall").is_some() {
            let call = part.get("functionCall").cloned().unwrap_or(json!({}));
            let name = call.get("name").and_then(Value::as_str).unwrap_or("");
            tool_calls.push(json!({
                "id": format!("call_{}", Uuid::new_v4()),
                "type": "function",
                "function": {
                    "name": name,
                    "arguments": call.get("args").map(|args| args.to_string()).unwrap_or_else(|| "{}".to_string()),
                },
            }));
            continue;
        }
        let content_text = part.get("text").and_then(Value::as_str).unwrap_or("");
        if content_text.is_empty() {
            continue;
        }
        if part
            .get("thought")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            reasoning.push_str(content_text);
        } else {
            text.push_str(content_text);
        }
    }
    (text, reasoning, tool_calls)
}

/// Gemini 非流式响应 → OpenAI chat.completion。
pub fn convert_response(
    upstream: &Value,
    _request_id: &str,
    requested_model: &str,
) -> Result<(Value, Usage), String> {
    if let Some(error) = upstream.get("error") {
        return Err(super::extract_error_message(&error.to_string()));
    }
    let candidate = upstream
        .pointer("/candidates/0")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let (text, reasoning, mut tool_calls) = parts_to_message(
        candidate
            .pointer("/content/parts")
            .and_then(Value::as_array)
            .unwrap_or(&Vec::new()),
    );
    if let Some(block_reason) = upstream
        .pointer("/promptFeedback/blockReason")
        .and_then(Value::as_str)
    {
        tracing::debug!("gemini prompt blocked: {block_reason}");
    }

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
    if !reasoning.is_empty() {
        message.insert("reasoning_content".to_string(), json!(reasoning));
    }

    let usage = extract_usage(upstream.get("usageMetadata").unwrap_or(&Value::Null));
    let has_tool_calls = message.get("tool_calls").is_some();
    let finish_reason = candidate
        .get("finishReason")
        .and_then(Value::as_str)
        .map(|reason| map_finish_reason(reason, has_tool_calls))
        .unwrap_or(if has_tool_calls { "tool_calls" } else { "stop" });

    let completion = json!({
        "id": format!("chatcmpl-{}", Uuid::new_v4()),
        "object": "chat.completion",
        "created": chrono::Utc::now().timestamp(),
        "model": requested_model,
        "choices": [{
            "index": 0,
            "message": Value::Object(message),
            "finish_reason": finish_reason,
        }],
        "usage": super::client_usage_json(&usage),
    });
    let _ = &mut tool_calls;
    Ok((completion, usage))
}

/// Gemini SSE chunk → OpenAI chunk 流转换器。
#[derive(Debug)]
pub struct GeminiStreamConverter {
    id: String,
    model: String,
    requested_model: String,
    started: bool,
    finish_reason: Option<&'static str>,
    usage: Option<Usage>,
    finished: bool,
    finish_emitted: bool,
    error: Option<String>,
    tool_counter: i64,
}

impl GeminiStreamConverter {
    pub fn new(_request_id: &str, requested_model: &str) -> Self {
        Self {
            id: format!("chatcmpl-{}", Uuid::new_v4()),
            model: requested_model.to_string(),
            requested_model: requested_model.to_string(),
            started: false,
            finish_reason: None,
            usage: None,
            finished: false,
            finish_emitted: false,
            error: None,
            tool_counter: 0,
        }
    }

    fn ensure_started(&mut self, out: &mut Vec<Value>) {
        if !self.started {
            self.started = true;
            out.push(super::chunk_json(
                &self.id,
                &self.requested_model,
                json!({"role": "assistant"}),
                None,
            ));
        }
    }

    pub fn convert_event(&mut self, data: &str) -> Result<Vec<Value>, String> {
        if data == "[DONE]" {
            self.finished = true;
            return Ok(Vec::new());
        }
        let value: Value =
            serde_json::from_str(data).map_err(|e| format!("解析 Gemini SSE 失败：{e}"))?;
        if value.get("error").is_some() {
            self.error = Some(super::extract_error_message(&value.to_string()));
            self.finished = true;
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        if let Some(model_version) = value.get("modelVersion").and_then(Value::as_str) {
            self.model = model_version.to_string();
        }

        if let Some(usage) = value.get("usageMetadata") {
            let extracted = extract_usage(usage);
            if extracted.input_tokens.is_some() || extracted.output_tokens.is_some() {
                self.usage = Some(extracted);
            }
        }

        if let Some(parts) = value
            .pointer("/candidates/0/content/parts")
            .and_then(Value::as_array)
        {
            let (text, reasoning, tool_calls) = parts_to_message(parts);
            if !reasoning.is_empty() {
                self.ensure_started(&mut out);
                out.push(super::chunk_json(
                    &self.id,
                    &self.requested_model,
                    json!({"reasoning_content": reasoning}),
                    None,
                ));
            }
            if !text.is_empty() {
                self.ensure_started(&mut out);
                out.push(super::chunk_json(
                    &self.id,
                    &self.requested_model,
                    json!({"content": text}),
                    None,
                ));
            }
            for call in tool_calls {
                self.ensure_started(&mut out);
                let index = self.tool_counter;
                self.tool_counter += 1;
                let mut call = call;
                call["index"] = json!(index);
                out.push(super::chunk_json(
                    &self.id,
                    &self.requested_model,
                    json!({"tool_calls": [call]}),
                    None,
                ));
            }
        }

        if let Some(reason) = value
            .pointer("/candidates/0/finishReason")
            .and_then(Value::as_str)
        {
            self.finish_reason = Some(map_finish_reason(reason, self.tool_counter > 0));
        }

        // Gemini 以 finishReason + usageMetadata 收尾；没有显式终止事件，
        // 上游关闭连接即结束。这里不输出 finish chunk，由管线在流结束时补发。
        Ok(out)
    }

    /// 流结束后补发 finish chunk。
    pub fn final_chunk(&mut self) -> Option<Value> {
        if self.error.is_some() || self.finish_emitted {
            return None;
        }
        self.finish_emitted = true;
        self.ensure_started(&mut Vec::new());
        Some(super::chunk_json(
            &self.id,
            &self.requested_model,
            json!({}),
            Some(self.finish_reason.unwrap_or("stop")),
        ))
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

/// 工具名反查表（供管线级别复用）。
pub type ToolNameMap = HashMap<String, String>;

#[cfg(test)]
mod tests {
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
        assert_eq!(map_finish_reason("STOP", false), "stop");
        assert_eq!(map_finish_reason("MAX_TOKENS", false), "length");
        assert_eq!(map_finish_reason("SAFETY", false), "content_filter");
        assert_eq!(map_finish_reason("RECITATION", false), "content_filter");
        assert_eq!(map_finish_reason("MALFORMED_FUNCTION_CALL", false), "stop");
        assert_eq!(map_finish_reason("STOP", true), "tool_calls");
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
    fn converts_non_stream_response() {
        let upstream = from_str::<Value>(
            r#"{
                "candidates": [{
                    "content": {"parts": [{"text": "hi"}, {"text": "think", "thought": true}, {"functionCall": {"name": "f", "args": {"a": 1}}}]}
                }],
                "usageMetadata": {"promptTokenCount": 10, "candidatesTokenCount": 3, "thoughtsTokenCount": 2}
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
        assert_eq!(usage.output_tokens, Some(5));
    }
}

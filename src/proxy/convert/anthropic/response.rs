use super::*;

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

/// stop_reason → (OpenAI finish_reason, 原生值透传)；空值原生为 None。
pub fn normalize_stop_reason(
    stop_reason: &str,
    has_tool_calls: bool,
) -> (&'static str, Option<&str>) {
    let native = Some(stop_reason).filter(|reason| !reason.is_empty());
    if has_tool_calls {
        return ("tool_calls", native);
    }
    let finish_reason = match stop_reason {
        "end_turn" | "stop_sequence" | "pause_turn" => "stop",
        "max_tokens" | "compaction" | "model_context_window_exceeded" => "length",
        "refusal" => "content_filter",
        "" => "stop",
        other => {
            tracing::debug!("unmapped anthropic stop_reason: {other}");
            "stop"
        }
    };
    (finish_reason, native)
}

/// usage 归一：prompt_tokens = input + cache_read + cache_creation（含缓存总输入）；
/// 缓存命中口径只算 cache_read——cache_creation 是写入，计入会虚高命中率（OpenRouter 同口径）。
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
        cache_tokens: read.max(0),
        output_tokens: output,
        // Anthropic 无推理 token 单列口径（output_tokens 已含思考）。
        reasoning_tokens: None,
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
        return Err(crate::proxy::convert::extract_error_message(
            &upstream.to_string(),
        ));
    }
    let stop_reason = upstream
        .get("stop_reason")
        .and_then(Value::as_str)
        .unwrap_or("");
    let mut text = String::new();
    let mut reasoning_parts: Vec<String> = Vec::new();
    let mut reasoning_details: Vec<Value> = Vec::new();
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
                    reasoning_details.push(crate::proxy::convert::reasoning_text_detail(
                        crate::proxy::convert::REASONING_FORMAT_ANTHROPIC,
                        reasoning_details.len() as i64,
                        block.get("thinking").and_then(Value::as_str).unwrap_or(""),
                        block.get("signature").and_then(Value::as_str),
                    ));
                }
                Some("redacted_thinking") => {
                    reasoning_details.push(crate::proxy::convert::reasoning_encrypted_detail(
                        crate::proxy::convert::REASONING_FORMAT_ANTHROPIC,
                        reasoning_details.len() as i64,
                        None,
                        block.get("data").cloned().unwrap_or(Value::Null),
                    ));
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
        // 模型违规先输出 preamble 文本再调 json 工具时不丢弃那段文本：
        // 拼在 json 前（客户端仍可解析 json 前缀之后的整体，或按需忽略）。
        let content = if text.is_empty() {
            json_output
        } else {
            format!("{text}{json_output}")
        };
        json!({"role": "assistant", "content": content})
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
            message.insert(
                "reasoning_content".to_string(),
                json!(reasoning_parts.join("\n")),
            );
        }
        crate::proxy::convert::attach_reasoning_details(&mut message, reasoning_details);
        Value::Object(message)
    };

    let has_tool_calls = message.get("tool_calls").is_some();
    let (finish_reason, native_finish_reason) = normalize_stop_reason(stop_reason, has_tool_calls);
    let mut completion = json!({
        "id": upstream.get("id").and_then(Value::as_str).map(|s| s.to_string()).unwrap_or_else(|| request_id.to_string()),
        "object": "chat.completion",
        "created": chrono::Utc::now().timestamp(),
        "model": requested_model,
        "choices": [{
            "index": 0,
            "message": message,
            "finish_reason": finish_reason,
        }],
        "usage": cached_client_usage_json(&usage),
    });
    crate::proxy::convert::attach_native_finish_reason(&mut completion, native_finish_reason);
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
    /// 进行中的 thinking 块缓冲：block_index → (思考文本, 签名)。
    thinking_buffers: HashMap<i64, (String, String)>,
    next_detail_index: i64,
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
            thinking_buffers: HashMap::new(),
            next_detail_index: 0,
            usage: None,
            finished: false,
            finish_emitted: false,
            error: None,
        }
    }

    fn ensure_started(&mut self, out: &mut Vec<Value>) {
        if !self.started {
            self.started = true;
            out.push(crate::proxy::convert::chunk_json(
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
        let value: Value =
            serde_json::from_str(data).map_err(|e| format!("解析 Anthropic SSE 失败：{e}"))?;
        let mut out = Vec::new();
        match value.get("type").and_then(Value::as_str) {
            Some("error") => {
                self.error = Some(crate::proxy::convert::extract_error_message(data));
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
                    out.push(crate::proxy::convert::chunk_json(
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
                } else if block_type == "thinking" {
                    self.thinking_buffers
                        .insert(block_index, (String::new(), String::new()));
                } else if block_type == "redacted_thinking" {
                    // redacted_thinking 无增量事件，data 随 content_block_start 一次到齐。
                    let detail = crate::proxy::convert::reasoning_encrypted_detail(
                        crate::proxy::convert::REASONING_FORMAT_ANTHROPIC,
                        self.next_detail_index,
                        None,
                        block.get("data").cloned().unwrap_or(Value::Null),
                    );
                    self.next_detail_index += 1;
                    self.ensure_started(&mut out);
                    out.push(crate::proxy::convert::chunk_json(
                        &self.id,
                        &self.model,
                        json!({"reasoning_details": [detail]}),
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
                            out.push(crate::proxy::convert::chunk_json(
                                &self.id,
                                &self.model,
                                json!({"content": text}),
                                None,
                            ));
                        }
                    }
                    Some("thinking_delta") => {
                        let text = delta.get("thinking").and_then(Value::as_str).unwrap_or("");
                        if let Some(buffer) = self.thinking_buffers.get_mut(&block_index) {
                            buffer.0.push_str(text);
                        }
                        if !text.is_empty() {
                            self.ensure_started(&mut out);
                            out.push(crate::proxy::convert::chunk_json(
                                &self.id,
                                &self.model,
                                json!({"reasoning_content": text}),
                                None,
                            ));
                        }
                    }
                    Some("signature_delta") => {
                        let signature =
                            delta.get("signature").and_then(Value::as_str).unwrap_or("");
                        if let Some(buffer) = self.thinking_buffers.get_mut(&block_index) {
                            buffer.1.push_str(signature);
                        }
                    }
                    Some("input_json_delta") => {
                        let arguments = delta
                            .get("partial_json")
                            .and_then(Value::as_str)
                            .unwrap_or("");
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
                                out.push(crate::proxy::convert::chunk_json(
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
            Some("content_block_stop") => {
                let block_index = value.get("index").and_then(Value::as_i64).unwrap_or(0);
                if let Some((text, signature)) = self.thinking_buffers.remove(&block_index) {
                    let detail = crate::proxy::convert::reasoning_text_detail(
                        crate::proxy::convert::REASONING_FORMAT_ANTHROPIC,
                        self.next_detail_index,
                        &text,
                        if signature.is_empty() {
                            None
                        } else {
                            Some(signature.as_str())
                        },
                    );
                    self.next_detail_index += 1;
                    self.ensure_started(&mut out);
                    out.push(crate::proxy::convert::chunk_json(
                        &self.id,
                        &self.model,
                        json!({"reasoning_details": [detail]}),
                        None,
                    ));
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
                            reasoning_tokens: extracted
                                .reasoning_tokens
                                .or(previous.reasoning_tokens),
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
                // 按 block_index 升序 flush：HashMap 迭代序不定，多 json 工具块
                // 时输出顺序会错乱（单工具是常态，多调用时顺序应与上游块序一致）。
                let mut buffer_entries: Vec<(i64, String)> = self
                    .json_mode_buffers
                    .iter()
                    .map(|(index, buffer)| (*index, buffer.clone()))
                    .collect();
                buffer_entries.sort_by_key(|(index, _)| *index);
                let buffers: Vec<String> = buffer_entries
                    .into_iter()
                    .map(|(_, buffer)| {
                        let parsed: Value =
                            serde_json::from_str(&buffer).unwrap_or_else(|_| json!({}));
                        unwrap_json_tool_output(parsed).to_string()
                    })
                    .collect();
                for content in buffers {
                    out.push(crate::proxy::convert::chunk_json(
                        &self.id,
                        &self.model,
                        json!({"content": content}),
                        None,
                    ));
                }
                self.finish_emitted = true;
                let (finish_reason, native) =
                    normalize_stop_reason(stop_reason, self.next_tool_index > 0);
                out.push(crate::proxy::convert::chunk_json_with_native(
                    &self.id,
                    &self.model,
                    json!({}),
                    Some(finish_reason),
                    native,
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

/// 原生透传流式用量扫描器：旁路解析 Anthropic SSE 的 data 事件
/// （`event:` 行可有可无，事件类型以 data JSON 的 `type` 为准）。
/// usage 合并口径：message_start（输入侧，含缓存读/写）⊕ message_delta（output_tokens）。
#[derive(Default)]
pub struct AnthropicStreamUsageScanner {
    splitter: SseSplitter,
    merged: Option<Value>,
    usage: Option<Usage>,
    content_seen: bool,
}

impl AnthropicStreamUsageScanner {
    pub fn feed(&mut self, bytes: &[u8]) {
        for data in self.splitter.feed(&String::from_utf8_lossy(bytes)) {
            let Ok(value) = serde_json::from_str::<Value>(&data) else {
                continue;
            };
            match value.get("type").and_then(Value::as_str) {
                Some("message_start") => {
                    self.merged = value
                        .pointer("/message/usage")
                        .filter(|u| u.is_object())
                        .cloned();
                }
                Some("message_delta") => {
                    if let Some(delta_usage) = value.get("usage").filter(|u| u.is_object()) {
                        let target = self.merged.get_or_insert_with(|| Value::Object(Map::new()));
                        if let Some(map) = target.as_object_mut() {
                            for (key, val) in delta_usage.as_object().expect("checked object") {
                                map.insert(key.clone(), val.clone());
                            }
                        }
                    }
                }
                Some("content_block_delta") => self.content_seen = true,
                _ => {}
            }
            if let Some(current) = self.merged.as_ref() {
                self.usage = Some(extract_usage(current));
            }
        }
    }

    /// 是否见过内容块（读取后清零；供 TTFT 打点）。
    pub fn take_content_seen(&mut self) -> bool {
        std::mem::take(&mut self.content_seen)
    }

    pub fn usage(&self) -> Option<Usage> {
        self.usage.clone()
    }
}

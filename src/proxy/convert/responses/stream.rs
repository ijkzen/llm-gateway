use super::*;
use crate::proxy::metrics::Usage;

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
            out.push(crate::proxy::convert::chunk_json(
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
        out.push(crate::proxy::convert::chunk_json(
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
        out.push(crate::proxy::convert::chunk_json(
            &self.id,
            &self.model,
            delta,
            None,
        ));
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
        out.push(crate::proxy::convert::chunk_json(
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
        out.push(crate::proxy::convert::chunk_json(
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
        out.push(crate::proxy::convert::chunk_json(
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
                    let detail = crate::proxy::convert::reasoning_encrypted_detail(
                        crate::proxy::convert::REASONING_FORMAT_RESPONSES,
                        self.next_detail_index,
                        item.get("id").and_then(Value::as_str),
                        json!(encrypted),
                    );
                    self.next_detail_index += 1;
                    self.ensure_started(out);
                    out.push(crate::proxy::convert::chunk_json(
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
                    out.push(crate::proxy::convert::chunk_json(
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
                out.push(crate::proxy::convert::chunk_json(
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
                out.push(crate::proxy::convert::chunk_json(
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
                self.error = Some(crate::proxy::convert::extract_error_message(data));
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

/// 原生透传流式用量扫描器：旁路解析 Responses SSE 的 data 事件。
/// usage 取自 response.completed / response.incomplete / response.failed 的
/// response.usage；内容打点看 output_text / reasoning_summary_text delta。
#[derive(Default)]
pub struct ResponsesStreamUsageScanner {
    splitter: SseSplitter,
    usage: Option<Usage>,
    content_seen: bool,
}

impl ResponsesStreamUsageScanner {
    pub fn feed(&mut self, bytes: &[u8]) {
        for data in self.splitter.feed(&String::from_utf8_lossy(bytes)) {
            let Ok(value) = serde_json::from_str::<Value>(&data) else {
                continue;
            };
            match value.get("type").and_then(Value::as_str) {
                Some("response.output_text.delta")
                | Some("response.reasoning_summary_text.delta") => {
                    self.content_seen = true;
                }
                Some("response.completed")
                | Some("response.incomplete")
                | Some("response.failed") => {
                    if let Some(found) = value.pointer("/response/usage") {
                        self.usage = ResponsesStreamConverter::extract_usage(found);
                    }
                }
                _ => {}
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

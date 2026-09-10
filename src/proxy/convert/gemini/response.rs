use super::*;
use crate::proxy::metrics::Usage;
use uuid::Uuid;

/// Gemini finishReason → (OpenAI finish_reason, 原生值透传)（LiteLLM 全表 + native）。
pub fn map_finish_reason(reason: &str, has_tool_calls: bool) -> (&'static str, Option<&str>) {
    let native = Some(reason).filter(|reason| !reason.is_empty());
    if has_tool_calls {
        return ("tool_calls", native);
    }
    let finish_reason = match reason {
        "STOP"
        | "FINISH_REASON_UNSPECIFIED"
        | "MALFORMED_FUNCTION_CALL"
        | "TOO_MANY_TOOL_CALLS"
        | "MALFORMED_RESPONSE"
        | "UNEXPECTED_TOOL_CALL"
        | "NO_IMAGE" => "stop",
        "MAX_TOKENS" => "length",
        "SAFETY"
        | "RECITATION"
        | "BLOCKLIST"
        | "PROHIBITED_CONTENT"
        | "SPII"
        | "IMAGE_SAFETY"
        | "IMAGE_PROHIBITED_CONTENT"
        | "IMAGE_RECITATION"
        | "IMAGE_OTHER"
        | "LANGUAGE"
        | "OTHER" => "content_filter",
        "" => "stop",
        other => {
            tracing::debug!("unmapped gemini finishReason: {other}");
            "stop"
        }
    };
    (finish_reason, native)
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
        reasoning_tokens: (thoughts > 0).then_some(thoughts),
    }
}

/// AI SDK 客户端（ZCode）的 thoughtSignature 载体：tool_calls[i].extra_content
/// .google.thought_signature。响应方向双写该字段（reasoning_details 保留给
/// OpenRouter 风格客户端），请求方向按 tool_calls 下标读回。
fn extra_content_with_signature(signature: &str) -> Value {
    json!({"google": {"thought_signature": signature}})
}

/// parts → (正文, 思考文本, tool_calls, 各 tool_call 对应的 thoughtSignature)。
fn parts_to_message(parts: &[Value]) -> (String, String, Vec<Value>, Vec<Option<String>>) {
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut tool_calls: Vec<Value> = Vec::new();
    let mut signatures: Vec<Option<String>> = Vec::new();
    for part in parts {
        if part.get("functionCall").is_some() {
            let call = part.get("functionCall").cloned().unwrap_or(json!({}));
            let name = call.get("name").and_then(Value::as_str).unwrap_or("");
            let signature = part
                .get("thoughtSignature")
                .and_then(Value::as_str)
                .map(str::to_string);
            signatures.push(signature.clone());
            let mut tool_call = json!({
                "id": format!("call_{}", Uuid::new_v4()),
                "type": "function",
                "function": {
                    "name": name,
                    "arguments": call.get("args").map(|args| args.to_string()).unwrap_or_else(|| "{}".to_string()),
                },
            });
            if let Some(signature) = signature {
                tool_call["extra_content"] = extra_content_with_signature(&signature);
            }
            tool_calls.push(tool_call);
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
    (text, reasoning, tool_calls, signatures)
}

/// Gemini 非流式响应 → OpenAI chat.completion。
pub fn convert_response(
    upstream: &Value,
    _request_id: &str,
    requested_model: &str,
) -> Result<(Value, Usage), String> {
    if let Some(error) = upstream.get("error") {
        return Err(crate::proxy::convert::extract_error_message(
            &error.to_string(),
        ));
    }
    let candidate = upstream
        .pointer("/candidates/0")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let (text, reasoning, mut tool_calls, signatures) = parts_to_message(
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
    let reasoning_details = signatures
        .iter()
        .enumerate()
        .filter_map(|(index, signature)| {
            signature.as_ref().map(|signature| {
                crate::proxy::convert::reasoning_encrypted_detail(
                    crate::proxy::convert::REASONING_FORMAT_GEMINI,
                    index as i64,
                    None,
                    json!(signature),
                )
            })
        })
        .collect();
    crate::proxy::convert::attach_reasoning_details(&mut message, reasoning_details);

    let usage = extract_usage(upstream.get("usageMetadata").unwrap_or(&Value::Null));
    let has_tool_calls = message.get("tool_calls").is_some();
    // 提示词被安全拦截时 candidates 通常为空，必须显式返回 content_filter，
    // 否则客户端把拒答误判为正常空响应（LiteLLM 同款）。blockReason 不是
    // finishReason，原生值省略。
    let (finish_reason, native_finish_reason) =
        if upstream.pointer("/promptFeedback/blockReason").is_some() {
            ("content_filter", None)
        } else {
            candidate
                .get("finishReason")
                .and_then(Value::as_str)
                .map(|reason| map_finish_reason(reason, has_tool_calls))
                .unwrap_or((if has_tool_calls { "tool_calls" } else { "stop" }, None))
        };

    let mut completion = json!({
        "id": format!("chatcmpl-{}", Uuid::new_v4()),
        "object": "chat.completion",
        "created": chrono::Utc::now().timestamp(),
        "model": requested_model,
        "choices": [{
            "index": 0,
            "message": Value::Object(message),
            "finish_reason": finish_reason,
        }],
        "usage": crate::proxy::convert::cached_client_usage_json(&usage),
    });
    crate::proxy::convert::attach_native_finish_reason(&mut completion, native_finish_reason);
    let _ = &mut tool_calls;
    Ok((completion, usage))
}

/// Gemini SSE chunk → OpenAI chunk 流转换器。
#[derive(Debug)]
pub struct GeminiStreamConverter {
    id: String,
    requested_model: String,
    started: bool,
    finish_reason: Option<&'static str>,
    native_finish_reason: Option<String>,
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
            requested_model: requested_model.to_string(),
            started: false,
            finish_reason: None,
            native_finish_reason: None,
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
            out.push(crate::proxy::convert::chunk_json(
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
            self.error = Some(crate::proxy::convert::extract_error_message(
                &value.to_string(),
            ));
            self.finished = true;
            return Ok(Vec::new());
        }
        let mut out = Vec::new();

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
            let (text, reasoning, tool_calls, signatures) = parts_to_message(parts);
            if !reasoning.is_empty() {
                self.ensure_started(&mut out);
                out.push(crate::proxy::convert::chunk_json(
                    &self.id,
                    &self.requested_model,
                    json!({"reasoning_content": reasoning}),
                    None,
                ));
            }
            if !text.is_empty() {
                self.ensure_started(&mut out);
                out.push(crate::proxy::convert::chunk_json(
                    &self.id,
                    &self.requested_model,
                    json!({"content": text}),
                    None,
                ));
            }
            for (call, signature) in tool_calls.into_iter().zip(signatures) {
                self.ensure_started(&mut out);
                let index = self.tool_counter;
                self.tool_counter += 1;
                let mut call = call;
                call["index"] = json!(index);
                if let Some(signature) = &signature {
                    call["extra_content"] = extra_content_with_signature(signature);
                }
                out.push(crate::proxy::convert::chunk_json(
                    &self.id,
                    &self.requested_model,
                    json!({"tool_calls": [call]}),
                    None,
                ));
                if let Some(signature) = signature {
                    let detail = crate::proxy::convert::reasoning_encrypted_detail(
                        crate::proxy::convert::REASONING_FORMAT_GEMINI,
                        index,
                        None,
                        json!(signature),
                    );
                    out.push(crate::proxy::convert::chunk_json(
                        &self.id,
                        &self.requested_model,
                        json!({"reasoning_details": [detail]}),
                        None,
                    ));
                }
            }
        }

        if let Some(reason) = value
            .pointer("/candidates/0/finishReason")
            .and_then(Value::as_str)
        {
            let (finish_reason, native) = map_finish_reason(reason, self.tool_counter > 0);
            self.finish_reason = Some(finish_reason);
            self.native_finish_reason = native.map(str::to_string);
        }

        // 流式中提示词被拦截（candidates 为空）时同样要给 content_filter。
        if value.pointer("/promptFeedback/blockReason").is_some() {
            self.finish_reason = Some("content_filter");
            self.native_finish_reason = None;
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
        Some(crate::proxy::convert::chunk_json_with_native(
            &self.id,
            &self.requested_model,
            json!({}),
            Some(self.finish_reason.unwrap_or("stop")),
            self.native_finish_reason.as_deref(),
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

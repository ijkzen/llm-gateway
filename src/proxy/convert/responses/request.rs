use super::*;
use crate::proxy::metrics::Usage;

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
    crate::proxy::convert::valid_reasoning_details(message)
        .into_iter()
        .filter(|detail| {
            detail.get("format").and_then(Value::as_str)
                == Some(crate::proxy::convert::REASONING_FORMAT_RESPONSES)
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

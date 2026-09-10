use super::*;

/// response_format 注入的合成 JSON 工具名。
pub const JSON_TOOL_NAME: &str = "__structured_output__";

/// 编码发往 Anthropic 的请求体。
pub fn build_request_body(
    chat: &Value,
    actual_model: &str,
) -> Result<(Value, crate::proxy::convert::RequestFlags), String> {
    let stream = chat.get("stream").and_then(Value::as_bool).unwrap_or(false);
    let max_tokens = chat_max_tokens(chat).unwrap_or(ANTHROPIC_DEFAULT_MAX_TOKENS);
    // 先于消息循环计算：assistant 历史的 thinking 块注入以 thinking 启用为前提
    // （官方约束：input 携带 thinking 块时必须开启 thinking）。
    let thinking = map_thinking(chat, max_tokens);
    let thinking_requested = thinking.is_some();

    let mut system_blocks: Vec<Value> = Vec::new();
    let mut messages: Vec<(String, Vec<Value>)> = Vec::new();

    let push_message =
        |messages: &mut Vec<(String, Vec<Value>)>, role: String, blocks: Vec<Value>| {
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
                // 回传的 thinking 块置于 text/tool_use 之前（官方要求 thinking
                // 块位于 assistant 消息开头），仅 thinking 启用时注入。
                if thinking_requested {
                    blocks.extend(anthropic_thinking_blocks(message));
                }
                let text = message_text(content);
                if !text.is_empty() {
                    blocks.push(json!({"type": "text", "text": text}));
                }
                if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
                    for call in tool_calls {
                        let name = call
                            .pointer("/function/name")
                            .and_then(Value::as_str)
                            .unwrap_or("");
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
                // tool_result 仅需 tool_use_id（Anthropic 以 id 关联，无需工具名反查）。
                // 空内容兜底：Anthropic 拒绝空 text 内容，工具无输出时填占位符（同 user 空消息规则）。
                let text = message_text(content);
                let block = json!({
                    "type": "tool_result",
                    "tool_use_id": tool_use_id,
                    "content": if text.is_empty() { " ".to_string() } else { text },
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
            let mut schema_object: Map<String, Value> =
                input_schema.as_object().cloned().unwrap_or_default();
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

    let thinking = drop_thinking_without_history_blocks(chat, thinking);
    let thinking_dropped = thinking_requested && thinking.is_none();
    let thinking_active = thinking.is_some();
    let mut tool_choice = map_tool_choice(chat);
    if thinking_active
        && let Some(choice) = tool_choice.take()
        && matches!(
            choice.get("type").and_then(Value::as_str),
            Some("any") | Some("tool")
        )
    {
        // thinking 模式只允许 auto/none：强制工具调用降级为 auto（保留并行开关）。
        let mut downgraded = json!({"type": "auto"});
        if let Some(disable) = choice.get("disable_parallel_tool_use") {
            downgraded["disable_parallel_tool_use"] = disable.clone();
        }
        tool_choice = Some(downgraded);
    }
    let mut json_mode_tool = false;

    if let Some(response_format) = chat.get("response_format") {
        let format_type = response_format
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("");
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
    if let Some(temperature) = chat.get("temperature")
        && (!thinking_active || temperature.as_f64() == Some(1.0))
    {
        // thinking 启用时官方只允许 temperature=1。
        body.insert("temperature".to_string(), temperature.clone());
    }
    if !thinking_active && let Some(top_p) = chat.get("top_p") {
        body.insert("top_p".to_string(), top_p.clone());
    }
    // top_k 官方支持且与 thinking 互斥（同 temperature 规则）。
    if !thinking_active && let Some(top_k) = chat.get("top_k").and_then(Value::as_i64) {
        body.insert("top_k".to_string(), json!(top_k));
    }
    if let Some(stop_sequences) = map_stop(chat) {
        body.insert("stop_sequences".to_string(), Value::Array(stop_sequences));
    }
    if let Some(user) = chat.get("user").and_then(Value::as_str)
        && !user.is_empty()
    {
        body.insert(
            "metadata".to_string(),
            json!({"user_id": truncate_chars(user, 512)}),
        );
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
    Ok((
        Value::Object(body),
        crate::proxy::convert::RequestFlags {
            json_mode_tool,
            thinking_dropped,
        },
    ))
}

fn map_tool_choice(chat: &Value) -> Option<Value> {
    let mut choice = match chat.get("tool_choice") {
        Some(Value::String(s)) => match s.as_str() {
            "auto" => Some(json!({"type": "auto"})),
            "required" => Some(json!({"type": "any"})),
            "none" => Some(json!({"type": "none"})),
            _ => None,
        },
        Some(value) => value
            .pointer("/function/name")
            .and_then(Value::as_str)
            .map(|name| json!({"type": "tool", "name": name})),
        None => None,
    };
    let parallel = chat
        .get("parallel_tool_calls")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    if !parallel {
        let base = choice.take().unwrap_or_else(|| json!({"type": "auto"}));
        // 官方 schema 不接受 none + disable_parallel_tool_use 组合（A7）：
        // tool_choice=none 本身已禁止任何调用，并行开关对该形态无意义，
        // 原样保留 none（不得因跳过改写而丢失整个 tool_choice）。
        if base.get("type").and_then(Value::as_str) == Some("none") {
            choice = Some(base);
        } else {
            let mut object = base.as_object().cloned().unwrap_or_default();
            object.insert("disable_parallel_tool_use".to_string(), json!(true));
            choice = Some(Value::Object(object));
        }
    }
    choice
}

fn map_thinking(chat: &Value, max_tokens: i64) -> Option<Value> {
    // 明确关闭（Disabled）与未指定（Unspecified）都不写 thinking（Anthropic 缺省即关闭）。
    let reasoning = match chat_reasoning(chat) {
        crate::proxy::convert::ChatReasoning::Enabled(reasoning) => reasoning,
        _ => return None,
    };
    if max_tokens <= 1024 {
        return None;
    }
    // 官方约束：budget_tokens >= 1024 且 < max_tokens（LiteLLM 同款钳制）。
    // reasoning.max_tokens 直传预算（OpenRouter Anthropic 风格），否则按 effort 档位换算。
    let budget = reasoning
        .max_tokens
        .unwrap_or_else(|| reasoning_budget(&reasoning.effort))
        .max(1024)
        .min(max_tokens - 1);
    Some(json!({"type": "enabled", "budget_tokens": budget}))
}

/// assistant 消息上回传的 anthropic 格式 reasoning_details → thinking 块。
/// 无有效签名/密文的项无法通过官方校验，跳过；块内容原样语义还原。
fn anthropic_thinking_blocks(message: &Value) -> Vec<Value> {
    crate::proxy::convert::valid_reasoning_details(message)
        .into_iter()
        .filter(|detail| {
            detail.get("format").and_then(Value::as_str)
                == Some(crate::proxy::convert::REASONING_FORMAT_ANTHROPIC)
        })
        .filter_map(|detail| match detail.get("type").and_then(Value::as_str) {
            Some("reasoning.text") => {
                let signature = detail
                    .get("signature")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if signature.is_empty() {
                    return None;
                }
                Some(json!({
                    "type": "thinking",
                    "thinking": detail.get("text").and_then(Value::as_str).unwrap_or(""),
                    "signature": signature,
                }))
            }
            Some("reasoning.encrypted") => {
                let data = detail.get("data").and_then(Value::as_str)?;
                (!data.is_empty()).then(|| json!({"type": "redacted_thinking", "data": data}))
            }
            _ => None,
        })
        .collect()
}

/// thinking + tool_calls 共存的官方约束规避：最后一条含 tool_calls 的
/// assistant 消息若没有可回传的 thinking 块（无有效 signature），启用
/// thinking 会触发 "Expected thinking or redacted_thinking" 400，此时丢弃
/// thinking 参数（LiteLLM 同款）；有块回传则正常保留。
fn drop_thinking_without_history_blocks(chat: &Value, thinking: Option<Value>) -> Option<Value> {
    let thinking = thinking?;
    let messages = chat_messages(chat);
    let Some(message) = messages.iter().rev().find(|message| {
        message.get("role").and_then(Value::as_str) == Some("assistant")
            && message
                .get("tool_calls")
                .and_then(Value::as_array)
                .is_some_and(|calls| !calls.is_empty())
    }) else {
        return Some(thinking);
    };
    if anthropic_thinking_blocks(message).is_empty() {
        tracing::warn!("已丢弃 thinking 参数：含 tool_calls 的 assistant 轮无 thinking 块回传");
        return None;
    }
    Some(thinking)
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
    if filtered.is_empty() {
        None
    } else {
        Some(filtered)
    }
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

use super::*;

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
                let mut function_call_positions: Vec<usize> = Vec::new();
                // extra_content 载体契约见 extra_content_with_signature。
                let mut extra_content_signatures: Vec<Option<String>> = Vec::new();
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
                        // functionCall.args 必须是 JSON 对象，非法值退空对象。
                        let args = serde_json::from_str::<Value>(arguments)
                            .ok()
                            .filter(Value::is_object)
                            .unwrap_or_else(|| json!({}));
                        extra_content_signatures.push(
                            call.pointer("/extra_content/google/thought_signature")
                                .and_then(Value::as_str)
                                .filter(|signature| !signature.is_empty())
                                .map(str::to_string),
                        );
                        function_call_positions.push(parts.len());
                        parts.push(json!({
                            "functionCall": {
                                "name": call.pointer("/function/name").and_then(Value::as_str).unwrap_or(""),
                                "args": args,
                            },
                        }));
                    }
                }
                // 回传的 thoughtSignature 按 tool_calls 下标挂回对应 functionCall
                // part（Gemini 3 工具轮强制校验签名，缺失直接 400）。
                // extra_content 来源优先（与 tool_call 一一对应），reasoning_details
                // 的 index 下标来源补缺。
                for (index, signature) in extra_content_signatures.iter().enumerate() {
                    if let Some(signature) = signature
                        && let Some(&position) = function_call_positions.get(index)
                    {
                        parts[position]["thoughtSignature"] = json!(signature);
                    }
                }
                for detail in crate::proxy::convert::valid_reasoning_details(message) {
                    if detail.get("format").and_then(Value::as_str)
                        != Some(crate::proxy::convert::REASONING_FORMAT_GEMINI)
                        || detail.get("type").and_then(Value::as_str) != Some("reasoning.encrypted")
                    {
                        continue;
                    }
                    let Some(signature) = detail.get("data").and_then(Value::as_str) else {
                        continue;
                    };
                    if signature.is_empty() {
                        continue;
                    }
                    let index = detail.get("index").and_then(Value::as_i64).unwrap_or(-1);
                    if extra_content_signatures
                        .get(index.max(0) as usize)
                        .is_some_and(Option::is_some)
                    {
                        continue;
                    }
                    if let Some(&position) = function_call_positions.get(index.max(0) as usize) {
                        parts[position]["thoughtSignature"] = json!(signature);
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
                let name = match tool_names.get(tool_call_id) {
                    Some(name) => name.clone(),
                    None => {
                        tracing::warn!(
                            tool_call_id,
                            "tool 结果未反查到工具名，退用 tool_call_id 作为 functionResponse.name"
                        );
                        tool_call_id.to_string()
                    }
                };
                let raw = message_text(content);
                // functionResponse.response 必须是 JSON 对象：非对象值（数组/
                // 数字/字符串字面量）包装进 result，避免上游 400（LiteLLM 同款）。
                let result = match serde_json::from_str::<Value>(&raw) {
                    Ok(value) if value.is_object() => value,
                    Ok(value) => json!({"result": value}),
                    Err(_) => json!({"result": raw}),
                };
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
    if let Some(top_k) = chat.get("top_k").and_then(Value::as_i64) {
        generation_config.insert("topK".to_string(), json!(top_k));
    }
    if let Some(seed) = chat.get("seed").and_then(Value::as_i64) {
        generation_config.insert("seed".to_string(), json!(seed));
    }
    match chat_reasoning(chat) {
        crate::proxy::convert::ChatReasoning::Enabled(reasoning) => {
            // includeThoughts=true：不开启时上游只思考不返回思考摘要，客户端收不到。
            // reasoning.max_tokens 直传 thinkingBudget（OpenRouter Gemini 语义），否则按 effort 档位。
            let budget = reasoning
                .max_tokens
                .unwrap_or_else(|| reasoning_budget(&reasoning.effort));
            generation_config.insert(
                "thinkingConfig".to_string(),
                json!({"thinkingBudget": budget, "includeThoughts": true}),
            );
        }
        // 明确关闭：Gemini 缺省动态思考仍开启，thinkingBudget=0 才是关闭语义
        // （不支持关闭的模型由上游自行钳到最小预算）。
        crate::proxy::convert::ChatReasoning::Disabled => {
            generation_config.insert("thinkingConfig".to_string(), json!({"thinkingBudget": 0}));
        }
        crate::proxy::convert::ChatReasoning::Unspecified => {}
    }
    if let Some(presence) = chat.get("presence_penalty").and_then(Value::as_f64) {
        generation_config.insert("presencePenalty".to_string(), json!(presence));
    }
    if let Some(frequency) = chat.get("frequency_penalty").and_then(Value::as_f64) {
        generation_config.insert("frequencyPenalty".to_string(), json!(frequency));
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

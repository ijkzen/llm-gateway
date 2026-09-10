//! 协议转换：OpenAI chat 入站 ↔ 各上游出站协议。
//!
//! 转换映射综合参考 nyro 与 LiteLLM 的实现（细节见各文件）。

pub mod anthropic;
pub mod gemini;
pub mod openai;
pub mod responses;

use serde_json::{Map, Value, json};

use crate::provider_model::refresh::PROTOCOL_GEMINI;

/// Anthropic `max_tokens` 缺省值（Anthropic 必填该字段）。
pub const ANTHROPIC_DEFAULT_MAX_TOKENS: i64 = 4096;

/// reasoning_details 载体的 format 标识（OpenRouter 兼容值）。
pub const REASONING_FORMAT_ANTHROPIC: &str = "anthropic-claude-v1";
pub const REASONING_FORMAT_RESPONSES: &str = "openai-responses-v1";
pub const REASONING_FORMAT_GEMINI: &str = "google-gemini-v1";

/// 构造 reasoning.text detail（思考原文 + 可选签名；签名缺失填 null）。
pub fn reasoning_text_detail(
    format: &str,
    index: i64,
    text: &str,
    signature: Option<&str>,
) -> Value {
    json!({
        "type": "reasoning.text",
        "text": text,
        "signature": signature,
        "id": Value::Null,
        "format": format,
        "index": index,
    })
}

/// 构造 reasoning.encrypted detail（data 为厂商原生签名/密文，原样搬运）。
pub fn reasoning_encrypted_detail(
    format: &str,
    index: i64,
    id: Option<&str>,
    data: Value,
) -> Value {
    json!({
        "type": "reasoning.encrypted",
        "data": data,
        "id": id,
        "format": format,
        "index": index,
    })
}

/// 非空时把 reasoning_details 数组写入 message。
pub fn attach_reasoning_details(message: &mut Map<String, Value>, details: Vec<Value>) {
    if !details.is_empty() {
        message.insert("reasoning_details".to_string(), Value::Array(details));
    }
}

/// reasoning_details 请求侧校验上限（防滥用；正常回传远小于此）。
const REASONING_DETAILS_MAX_ITEMS: usize = 128;
const REASONING_DETAILS_MAX_BYTES: usize = 512 * 1024;

/// 读取 assistant 消息上回传的 reasoning_details，逐项做形状校验与
/// 总量钳制；畸形项丢弃（debug 日志）。顺序保持原样（签名链不允许重排）。
pub fn valid_reasoning_details(message: &Value) -> Vec<Value> {
    let Some(items) = message.get("reasoning_details").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut total_bytes = 0usize;
    for item in items.iter().take(REASONING_DETAILS_MAX_ITEMS) {
        let Some(object) = item.as_object() else {
            tracing::debug!("丢弃畸形 reasoning_details 项：非对象");
            continue;
        };
        let detail_type = object
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !detail_type.starts_with("reasoning.") {
            tracing::debug!(detail_type, "丢弃畸形 reasoning_details 项：type 非法");
            continue;
        }
        let format = object.get("format").and_then(Value::as_str).unwrap_or("");
        if format.is_empty() {
            tracing::debug!("丢弃畸形 reasoning_details 项：format 缺失");
            continue;
        }
        total_bytes += item.to_string().len();
        if total_bytes > REASONING_DETAILS_MAX_BYTES {
            tracing::debug!("reasoning_details 超出大小上限，截断后续项");
            break;
        }
        out.push(item.clone());
    }
    out
}

/// reasoning_effort → thinking/thinkingConfig 预算（LiteLLM 档位）。
pub fn reasoning_budget(effort: &str) -> i64 {
    match effort {
        "minimal" => 128,
        "low" => 1024,
        "medium" => 2048,
        "high" => 4096,
        "xhigh" => 8192,
        "max" => 16384,
        _ => 1024,
    }
}

/// 请求构造的附加标记（目前仅 Anthropic 转换产生非默认值，其余协议 Default）。
#[derive(Debug, Default, Clone, Copy)]
pub struct RequestFlags {
    /// 是否注入了 JSON 模式合成工具。
    pub json_mode_tool: bool,
    /// 思考参数因工具轮历史缺签名块而被丢弃（客户端推理质量静默降级，
    /// 供管线向客户端透出可观测信号）。
    pub thinking_dropped: bool,
}

/// 归一后的请求侧思考参数（OpenRouter `reasoning` 对象语义）。
#[derive(Debug, PartialEq)]
pub struct ReasoningRequest {
    pub effort: String,
    pub max_tokens: Option<i64>,
    pub exclude: bool,
}

/// 归一后的请求侧思考状态。「未指定」与「明确关闭」必须区分：Gemini 缺省
/// 动态思考仍开启（照计费不回显）、Responses 缺省按默认档位思考，把关闭
/// 归一成未指定会导致客户端关不掉思考。
#[derive(Debug, PartialEq)]
pub enum ChatReasoning {
    Unspecified,
    Disabled,
    Enabled(ReasoningRequest),
}

impl ChatReasoning {
    /// 开启档位的参数引用；未指定/关闭返回 None。
    pub fn enabled(&self) -> Option<&ReasoningRequest> {
        match self {
            ChatReasoning::Enabled(request) => Some(request),
            _ => None,
        }
    }
}

/// 归一请求侧思考参数。识别的形态（优先级从高到低）：
/// 1. OpenRouter 主形态 `reasoning` 对象（effort/max_tokens/exclude/enabled）；
/// 2. 顶层 `reasoning_effort` 简写（与对象同传且不一致时取对象值）；
/// 3. `thinking: {"type": "enabled"/"disabled"}`（DeepSeek/智谱风格开关，
///    enabled 无档位概念，按 medium 兜底）；
/// 4. `enable_thinking: true/false`（阿里百炼风格开关）。
///
/// `enabled:false`、effort "none"、开关关闭均归一为 `Disabled`；空 reasoning
/// 对象等价 legacy `include_reasoning:true`（medium 兜底）；都没有是 `Unspecified`。
pub fn chat_reasoning(chat: &Value) -> ChatReasoning {
    let empty = Map::new();
    let has_object = chat.get("reasoning").is_some_and(Value::is_object);
    let object = chat
        .get("reasoning")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let shorthand = chat
        .get("reasoning_effort")
        .and_then(Value::as_str)
        .filter(|effort| !effort.is_empty());
    if has_object || shorthand.is_some() {
        if object.get("enabled").and_then(Value::as_bool) == Some(false) {
            return ChatReasoning::Disabled;
        }
        let max_tokens = object.get("max_tokens").and_then(Value::as_i64);
        let exclude = object
            .get("exclude")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let object_effort = object
            .get("effort")
            .and_then(Value::as_str)
            .filter(|effort| !effort.is_empty());
        if let Some(object_effort) = object_effort {
            if let Some(shorthand) = shorthand
                && shorthand != object_effort
            {
                tracing::debug!(
                    object_effort,
                    shorthand,
                    "reasoning.effort 与 reasoning_effort 不一致，取 reasoning.effort"
                );
            }
            if object_effort == "none" {
                return ChatReasoning::Disabled;
            }
            return ChatReasoning::Enabled(ReasoningRequest {
                effort: object_effort.to_string(),
                max_tokens,
                exclude,
            });
        }
        return match shorthand {
            Some("none") => ChatReasoning::Disabled,
            Some(effort) => ChatReasoning::Enabled(ReasoningRequest {
                effort: effort.to_string(),
                max_tokens,
                exclude,
            }),
            // OpenRouter 语义：空对象等价 legacy include_reasoning:true。
            None => ChatReasoning::Enabled(ReasoningRequest {
                effort: "medium".to_string(),
                max_tokens,
                exclude,
            }),
        };
    }
    // 开关型形态（无档位概念，开启按 medium 兜底）。
    if let Some(thinking_type) = chat.pointer("/thinking/type").and_then(Value::as_str) {
        return match thinking_type {
            "enabled" => ChatReasoning::Enabled(ReasoningRequest {
                effort: "medium".to_string(),
                max_tokens: None,
                exclude: false,
            }),
            "disabled" => ChatReasoning::Disabled,
            _ => ChatReasoning::Unspecified,
        };
    }
    if let Some(enable) = chat.get("enable_thinking").and_then(Value::as_bool) {
        return if enable {
            ChatReasoning::Enabled(ReasoningRequest {
                effort: "medium".to_string(),
                max_tokens: None,
                exclude: false,
            })
        } else {
            ChatReasoning::Disabled
        };
    }
    ChatReasoning::Unspecified
}

/// 拼接上游 URL：版本段判定与刷新侧共用
/// [`crate::provider_model::refresh::is_version_segment`]（单一事实源）。
/// base 末段已是版本段（`v1`/`v1beta`/`v1alpha`，或 OpenAI 兼容服务自定义的
/// `v2`/`v3`/`v4` 等纯数字版本）则直接拼子路径；否则按协议补默认版本段
/// （OpenAI Compat 补 `v1`、Gemini 补 `v1beta`）。
/// 注意：火山 Ark / 腾讯 lkeap 的套餐端点形如 `.../api/coding/v3`，其
/// OpenAI 兼容 chat 路径就是 `{base}/chat/completions`，不能再补一层 `/v1`。
pub fn build_upstream_url(base_url: &str, protocol_type: i32, sub_path: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    let last = trimmed.rsplit('/').next().unwrap_or("");
    if crate::provider_model::refresh::is_version_segment(last) {
        format!("{trimmed}/{sub_path}")
    } else if protocol_type == PROTOCOL_GEMINI {
        format!("{trimmed}/v1beta/{sub_path}")
    } else {
        format!("{trimmed}/v1/{sub_path}")
    }
}

/// 上游错误体中提取人类可读信息（OpenAI / Anthropic / Gemini 结构 + 纯文本兜底）。
pub fn extract_error_message(body: &str) -> String {
    if let Ok(value) = serde_json::from_str::<Value>(body) {
        let message = value
            .pointer("/error/message")
            .or_else(|| value.pointer("/error"))
            .and_then(|v| if v.is_string() { v.as_str() } else { None })
            .map(str::to_string)
            .or_else(|| {
                value
                    .get("message")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            });
        if let Some(message) = message {
            return message;
        }
        let compact = value.to_string();
        return truncate_chars(&compact, 200);
    }
    truncate_chars(body.trim(), 200)
}

pub fn truncate_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// 客户端可见的 usage JSON（只含 prompt/completion/total 三项）。
pub fn client_usage_json(usage: &crate::proxy::metrics::Usage) -> Value {
    let total = match (usage.input_tokens, usage.output_tokens) {
        (Some(input), Some(output)) => input + output,
        (Some(input), None) => input,
        (None, Some(output)) => output,
        (None, None) => 0,
    };
    json!({
        "prompt_tokens": usage.input_tokens.unwrap_or(0),
        "completion_tokens": usage.output_tokens.unwrap_or(0),
        "total_tokens": total,
    })
}

/// 客户端可见的带缓存命中 token 的 usage JSON。
pub fn cached_client_usage_json(usage: &crate::proxy::metrics::Usage) -> Value {
    let mut client_usage = client_usage_json(usage);
    if usage.cache_tokens > 0 {
        client_usage["prompt_tokens_details"] = json!({
            "cached_tokens": usage.cache_tokens,
        });
    }
    if let Some(reasoning) = usage.reasoning_tokens
        && reasoning > 0
    {
        client_usage["completion_tokens_details"] = json!({
            "reasoning_tokens": reasoning,
        });
    }
    client_usage
}

/// 构造 OpenAI chat.completion.chunk。
pub fn chunk_json(id: &str, model: &str, delta: Value, finish_reason: Option<&str>) -> Value {
    json!({
        "id": id,
        "object": "chat.completion.chunk",
        "created": chrono::Utc::now().timestamp(),
        "model": model,
        "choices": [{
            "index": 0,
            "delta": delta,
            "finish_reason": finish_reason,
        }],
    })
}

/// 构造末尾携带指定 usage JSON 的 chunk（仅在客户端请求 include_usage 时）。
pub fn usage_chunk_json(id: &str, model: &str, usage: Value) -> Value {
    json!({
        "id": id,
        "object": "chat.completion.chunk",
        "created": chrono::Utc::now().timestamp(),
        "model": model,
        "choices": [],
        "usage": usage,
    })
}

/// 原生终止值透传上限（防上游垃圾值透给客户端；信息保真优先于白名单，
/// 上游新枚举不会被静默丢弃）。
const NATIVE_FINISH_REASON_MAX_CHARS: usize = 32;

/// 构造携带 native_finish_reason 的 chunk（原生终止值透传，None 时省略字段）。
pub fn chunk_json_with_native(
    id: &str,
    model: &str,
    delta: Value,
    finish_reason: Option<&str>,
    native_finish_reason: Option<&str>,
) -> Value {
    let mut chunk = chunk_json(id, model, delta, finish_reason);
    if let Some(native) = native_finish_reason {
        chunk["choices"][0]["native_finish_reason"] =
            json!(truncate_chars(native, NATIVE_FINISH_REASON_MAX_CHARS));
    }
    chunk
}

/// 非流式响应 choice 上注入 native_finish_reason（None 时省略字段）。
pub fn attach_native_finish_reason(completion: &mut Value, native_finish_reason: Option<&str>) {
    let Some(native) = native_finish_reason else {
        return;
    };
    completion["choices"][0]["native_finish_reason"] =
        json!(truncate_chars(native, NATIVE_FINISH_REASON_MAX_CHARS));
}

/// 从 OpenAI chat 请求体提取归一后的 max_tokens（优先 max_completion_tokens）。
pub fn chat_max_tokens(chat: &Value) -> Option<i64> {
    chat.get("max_completion_tokens")
        .or_else(|| chat.get("max_tokens"))
        .and_then(Value::as_i64)
}

/// 读取 messages 数组。
pub fn chat_messages(chat: &Value) -> Vec<&Value> {
    chat.get("messages")
        .and_then(Value::as_array)
        .map(|items| items.iter().collect())
        .unwrap_or_default()
}

/// 消息文本内容：字符串原样；数组则拼接 text 字段。
pub fn message_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

/// 解析 JSON Schema 中 `$defs`/`definitions` 引用（内联展开一层递归）。
pub fn inline_defs(schema: &mut Value) {
    let defs = schema
        .get("$defs")
        .or_else(|| schema.get("definitions"))
        .cloned();
    let Some(defs) = defs else { return };
    inline_defs_recursive(schema, &defs, 0);
    if let Some(object) = schema.as_object_mut() {
        object.remove("$defs");
        object.remove("definitions");
    }
}

fn inline_defs_recursive(value: &mut Value, defs: &Value, depth: usize) {
    if depth > 16 {
        return;
    }
    match value {
        Value::Object(map) => {
            let ref_path = map
                .get("$ref")
                .and_then(Value::as_str)
                .and_then(|r| {
                    r.strip_prefix("#/$defs/")
                        .or_else(|| r.strip_prefix("#/definitions/"))
                })
                .map(str::to_string);
            if let Some(name) = ref_path
                && let Some(def) = defs.get(&name)
            {
                let mut cloned = def.clone();
                inline_defs_recursive(&mut cloned, defs, depth + 1);
                *value = cloned;
                return;
            }
            for (_, child) in map.iter_mut() {
                inline_defs_recursive(child, defs, depth + 1);
            }
        }
        Value::Array(items) => {
            for child in items.iter_mut() {
                inline_defs_recursive(child, defs, depth + 1);
            }
        }
        _ => {}
    }
}

/// 提取 assistant 消息中 tool_call_id → 工具名 的映射（供 tool 结果反查工具名）。
pub fn collect_tool_call_names(chat: &Value) -> std::collections::HashMap<String, String> {
    let mut names = std::collections::HashMap::new();
    for message in chat_messages(chat) {
        let role = message.get("role").and_then(Value::as_str).unwrap_or("");
        if role != "assistant" {
            continue;
        }
        if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
            for call in tool_calls {
                if let (Some(id), Some(name)) = (
                    call.get("id").and_then(Value::as_str),
                    call.pointer("/function/name").and_then(Value::as_str),
                ) {
                    names.insert(id.to_string(), name.to_string());
                }
            }
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider_model::refresh::{
        PROTOCOL_ANTHROPIC, PROTOCOL_GEMINI, PROTOCOL_OPENAI_COMPATIBLE,
    };

    #[test]
    fn build_url_follows_version_segment_rule() {
        assert_eq!(
            build_upstream_url(
                "https://api.openai.com/v1",
                PROTOCOL_OPENAI_COMPATIBLE,
                "chat/completions"
            ),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            build_upstream_url(
                "https://api.openai.com",
                PROTOCOL_OPENAI_COMPATIBLE,
                "chat/completions"
            ),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            build_upstream_url("https://api.anthropic.com", PROTOCOL_ANTHROPIC, "messages"),
            "https://api.anthropic.com/v1/messages"
        );
        assert_eq!(
            build_upstream_url(
                "https://generativelanguage.googleapis.com/",
                PROTOCOL_GEMINI,
                "models/m:generateContent"
            ),
            "https://generativelanguage.googleapis.com/v1beta/models/m:generateContent"
        );
        // 火山/腾讯套餐端点末段是 v3（OpenAI 兼容版本段），不能再补一层 /v1。
        assert_eq!(
            build_upstream_url(
                "https://ark.cn-beijing.volces.com/api/coding/v3",
                PROTOCOL_OPENAI_COMPATIBLE,
                "chat/completions"
            ),
            "https://ark.cn-beijing.volces.com/api/coding/v3/chat/completions"
        );
        assert_eq!(
            build_upstream_url(
                "https://api.lkeap.cloud.tencent.com/coding/v3",
                PROTOCOL_OPENAI_COMPATIBLE,
                "chat/completions"
            ),
            "https://api.lkeap.cloud.tencent.com/coding/v3/chat/completions"
        );
    }

    #[test]
    fn extract_error_message_supports_common_shapes() {
        assert_eq!(
            extract_error_message(r#"{"error":{"message":"boom"}}"#),
            "boom"
        );
        assert_eq!(
            extract_error_message(
                r#"{"type":"error","error":{"type":"x","message":"anthropic boom"}}"#
            ),
            "anthropic boom"
        );
        assert_eq!(
            extract_error_message(
                r#"{"error":{"code":400,"message":"gemini boom","status":"INVALID"}}"#
            ),
            "gemini boom"
        );
        assert_eq!(
            extract_error_message("plain text error"),
            "plain text error"
        );
    }

    #[test]
    fn inline_defs_expands_refs() {
        let mut schema: Value = serde_json::from_str(
            r##"{"type":"object","properties":{"a":{"$ref":"#/$defs/Item"}},"$defs":{"Item":{"type":"string"}}}"##,
        )
        .unwrap();
        inline_defs(&mut schema);
        assert_eq!(schema["properties"]["a"]["type"], "string");
        assert!(schema.get("$defs").is_none());
    }

    #[test]
    fn reasoning_budget_tiers() {
        assert_eq!(reasoning_budget("low"), 1024);
        assert_eq!(reasoning_budget("high"), 4096);
        assert_eq!(reasoning_budget("max"), 16384);
        assert_eq!(reasoning_budget("minimal"), 128);
    }

    #[test]
    fn chat_reasoning_parses_object_effort() {
        let reasoning = chat_reasoning(&json!({"reasoning": {"effort": "high"}}));
        let reasoning = reasoning.enabled().expect("should be enabled");
        assert_eq!(reasoning.effort, "high");
        assert_eq!(reasoning.max_tokens, None);
        assert!(!reasoning.exclude);
    }

    #[test]
    fn chat_reasoning_falls_back_to_shorthand() {
        assert_eq!(
            chat_reasoning(&json!({"reasoning_effort": "low"}))
                .enabled()
                .expect("should be enabled")
                .effort,
            "low"
        );
        assert_eq!(chat_reasoning(&json!({})), ChatReasoning::Unspecified);
    }

    #[test]
    fn chat_reasoning_object_wins_over_conflicting_shorthand() {
        let chat = json!({"reasoning": {"effort": "high"}, "reasoning_effort": "low"});
        assert_eq!(chat_reasoning(&chat).enabled().unwrap().effort, "high");
    }

    #[test]
    fn chat_reasoning_none_and_enabled_false_disable() {
        // 明确关闭是独立状态，不等于「未指定」（Gemini/Responses 下游行为不同）。
        assert_eq!(
            chat_reasoning(&json!({"reasoning": {"effort": "none"}})),
            ChatReasoning::Disabled
        );
        assert_eq!(
            chat_reasoning(&json!({"reasoning": {"enabled": false, "effort": "high"}})),
            ChatReasoning::Disabled
        );
        assert_eq!(
            chat_reasoning(&json!({"reasoning_effort": "none"})),
            ChatReasoning::Disabled
        );
    }

    #[test]
    fn chat_reasoning_empty_object_defaults_to_medium() {
        // OpenRouter 语义：reasoning:{} 等价 legacy include_reasoning:true。
        assert_eq!(
            chat_reasoning(&json!({"reasoning": {}}))
                .enabled()
                .unwrap()
                .effort,
            "medium"
        );
        assert_eq!(
            chat_reasoning(&json!({"reasoning": {"enabled": true}}))
                .enabled()
                .unwrap()
                .effort,
            "medium"
        );
    }

    #[test]
    fn chat_reasoning_carries_max_tokens_and_exclude() {
        let reasoning =
            chat_reasoning(&json!({"reasoning": {"max_tokens": 2000, "exclude": true}}));
        let reasoning = reasoning.enabled().expect("should be enabled");
        assert_eq!(reasoning.max_tokens, Some(2000));
        assert!(reasoning.exclude);
        // 无 effort 但有 max_tokens：仍视为开启，effort 以 medium 兜底。
        assert_eq!(reasoning.effort, "medium");
    }

    #[test]
    fn chat_reasoning_thinking_toggle_form() {
        // DeepSeek/智谱风格开关：无档位概念，开启按 medium 兜底。
        assert_eq!(
            chat_reasoning(&json!({"thinking": {"type": "enabled"}}))
                .enabled()
                .unwrap()
                .effort,
            "medium"
        );
        assert_eq!(
            chat_reasoning(&json!({"thinking": {"type": "disabled"}})),
            ChatReasoning::Disabled
        );
        // 显式 reasoning_effort 优先于开关（ZCode deepseek 家族两字段同发）。
        let chat = json!({"thinking": {"type": "enabled"}, "reasoning_effort": "high"});
        assert_eq!(chat_reasoning(&chat).enabled().unwrap().effort, "high");
        // 反向冲突：effort 显式关闭同样优先于开关开启。
        let chat = json!({"thinking": {"type": "enabled"}, "reasoning_effort": "none"});
        assert_eq!(chat_reasoning(&chat), ChatReasoning::Disabled);
    }

    #[test]
    fn chat_reasoning_enable_thinking_form() {
        // 阿里百炼风格开关。
        assert_eq!(
            chat_reasoning(&json!({"enable_thinking": true}))
                .enabled()
                .unwrap()
                .effort,
            "medium"
        );
        assert_eq!(
            chat_reasoning(&json!({"enable_thinking": false})),
            ChatReasoning::Disabled
        );
    }

    #[test]
    fn valid_reasoning_details_filters_malformed_items() {
        let message = json!({
            "role": "assistant",
            "reasoning_details": [
                "not-an-object",
                {"type": "wrong", "format": "x"},
                {"type": "reasoning.text"},
                {"type": "reasoning.text", "text": "ok", "format": "anthropic-claude-v1"},
            ],
        });
        let details = valid_reasoning_details(&message);
        assert_eq!(details.len(), 1);
        assert_eq!(details[0]["text"], "ok");
        // 非 assistant 场景由调用方保证；无字段时返回空。
        assert!(valid_reasoning_details(&json!({"role": "assistant"})).is_empty());
    }

    #[test]
    fn cached_client_usage_includes_cached_tokens_when_present() {
        let usage = crate::proxy::metrics::Usage {
            input_tokens: Some(12),
            cache_tokens: 5,
            output_tokens: Some(6),
            reasoning_tokens: None,
        };

        assert_eq!(
            cached_client_usage_json(&usage),
            json!({
                "prompt_tokens": 12,
                "prompt_tokens_details": {"cached_tokens": 5},
                "completion_tokens": 6,
                "total_tokens": 18,
            })
        );
    }

    #[test]
    fn client_usage_includes_reasoning_tokens_when_present() {
        let usage = crate::proxy::metrics::Usage {
            input_tokens: Some(12),
            cache_tokens: 0,
            output_tokens: Some(20),
            reasoning_tokens: Some(8),
        };
        assert_eq!(
            cached_client_usage_json(&usage)["completion_tokens_details"]["reasoning_tokens"],
            8
        );
        // 总量口径不变：completion_tokens 仍含推理 token。
        assert_eq!(cached_client_usage_json(&usage)["completion_tokens"], 20);

        let usage = crate::proxy::metrics::Usage {
            input_tokens: Some(12),
            cache_tokens: 0,
            output_tokens: Some(20),
            reasoning_tokens: None,
        };
        assert!(
            cached_client_usage_json(&usage)
                .get("completion_tokens_details")
                .is_none()
        );
    }

    #[test]
    fn cached_client_usage_omits_cached_tokens_when_absent() {
        let usage = crate::proxy::metrics::Usage {
            input_tokens: Some(12),
            cache_tokens: 0,
            output_tokens: Some(6),
            reasoning_tokens: None,
        };

        assert!(
            cached_client_usage_json(&usage)
                .get("prompt_tokens_details")
                .is_none()
        );
        assert!(
            client_usage_json(&usage)
                .get("prompt_tokens_details")
                .is_none()
        );
    }
    /// 05-08：128 项上限——超出部分静默丢弃，前 128 项保持原顺序。
    #[test]
    fn valid_reasoning_details_caps_item_count() {
        let items: Vec<Value> = (0..200)
            .map(|i| json!({"type": "reasoning.text", "text": format!("t{i}"), "format": "anthropic-claude-v1"}))
            .collect();
        let message = json!({"role": "assistant", "reasoning_details": items});
        let details = valid_reasoning_details(&message);
        assert_eq!(details.len(), 128, "超出 128 项应被截断");
        assert_eq!(details[0]["text"], "t0", "顺序从首项开始");
        assert_eq!(details[127]["text"], "t127", "第 128 项保留");
    }

    /// 05-08：512KB 上限——超字节后 break，剩余项丢弃；顺序仍为原序。
    #[test]
    fn valid_reasoning_details_caps_total_bytes() {
        // 每项约 10KB，100 项 ≈ 1MB > 512KB（项数在 128 上限内，专测字节上限）。
        let big = "x".repeat(10_000);
        let items: Vec<Value> = (0..100)
            .map(|_| json!({"type": "reasoning.encrypted", "data": big, "format": "openai-responses-v1"}))
            .collect();
        let message = json!({"role": "assistant", "reasoning_details": items});
        let details = valid_reasoning_details(&message);
        assert!(
            details.len() < 100,
            "超 512KB 应截断（实际 {}）",
            details.len()
        );
        assert!(details.len() > 40, "接近上限前应尽量保留");
    }

    /// 05-08：顺序保持（签名链不允许重排）——输出与输入顺序逐项一致。
    #[test]
    fn valid_reasoning_details_preserves_order() {
        let message = json!({
            "role": "assistant",
            "reasoning_details": [
                {"type": "reasoning.text", "text": "first", "format": "anthropic-claude-v1"},
                {"type": "reasoning.encrypted", "data": "second", "format": "openai-responses-v1"},
                {"type": "reasoning.text", "text": "third", "format": "google-gemini-v1"},
            ],
        });
        let details = valid_reasoning_details(&message);
        assert_eq!(details.len(), 3);
        assert_eq!(details[0]["text"], "first");
        assert_eq!(details[1]["data"], "second");
        assert_eq!(details[2]["text"], "third");
    }
}

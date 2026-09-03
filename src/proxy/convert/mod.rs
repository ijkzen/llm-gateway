//! 协议转换：OpenAI chat 入站 ↔ 各上游出站协议。
//!
//! 转换映射综合参考 nyro 与 LiteLLM 的实现（细节见各文件）。

pub mod anthropic;
pub mod gemini;
pub mod openai;
pub mod responses;

use serde_json::{Value, json};

use crate::entity::provider_model;
use crate::provider_model::refresh::PROTOCOL_GEMINI;

/// Anthropic `max_tokens` 缺省值（Anthropic 必填该字段）。
pub const ANTHROPIC_DEFAULT_MAX_TOKENS: i64 = 4096;

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

/// 拼接上游 URL：沿用 `build_models_url` 的版本段规则
/// （base 末段已是 v1/v1beta/v1alpha 则直接拼，否则按协议补默认版本段）。
pub fn build_upstream_url(base_url: &str, protocol_type: i32, sub_path: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    let last = trimmed.rsplit('/').next().unwrap_or("");
    let versioned = matches!(last, "v1" | "v1beta" | "v1alpha");
    if versioned {
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

/// provider_model 上的真实模型 ID（发给上游的 model）。
pub fn actual_model_id(model: &provider_model::Model) -> String {
    model.provider_model_id.clone()
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
    fn cached_client_usage_includes_cached_tokens_when_present() {
        let usage = crate::proxy::metrics::Usage {
            input_tokens: Some(12),
            cache_tokens: 5,
            output_tokens: Some(6),
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
    fn cached_client_usage_omits_cached_tokens_when_absent() {
        let usage = crate::proxy::metrics::Usage {
            input_tokens: Some(12),
            cache_tokens: 0,
            output_tokens: Some(6),
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
}

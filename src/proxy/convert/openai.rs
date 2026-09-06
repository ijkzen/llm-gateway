//! OpenAI Compatible 出站：字节直通（仅重写 model + 注入 include_usage），
//! 以及 OpenAI 格式 usage 提取。

use serde_json::{Value, json};

use crate::proxy::metrics::Usage;
use crate::proxy::sse::SseSplitter;

/// 生成发往上游的请求体副本：重写 model；流式且客户端未开 include_usage 时注入。
pub fn build_request_body(chat: &Value, actual_model: &str) -> Value {
    let mut body = chat.clone();
    if let Some(object) = body.as_object_mut() {
        object.insert("model".to_string(), json!(actual_model));
        let stream = object
            .get("stream")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if stream {
            let include_usage = object
                .get("stream_options")
                .and_then(|opts| opts.get("include_usage"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if !include_usage {
                match object.get_mut("stream_options") {
                    Some(Value::Object(opts)) => {
                        opts.insert("include_usage".to_string(), json!(true));
                    }
                    _ => {
                        object.insert("stream_options".to_string(), json!({"include_usage": true}));
                    }
                }
            }
        }
    }
    body
}

/// 从 OpenAI 兼容 usage 对象提取归一 usage（兼容 DeepSeek/Gemini 兼容别名）。
pub fn extract_usage(usage: &Value) -> Usage {
    let input = first_i64(
        usage,
        &[
            "prompt_tokens",
            "promptTokenCount",
            "input_tokens",
            "inputTokenCount",
        ],
    );
    let output = first_i64(
        usage,
        &[
            "completion_tokens",
            "candidatesTokenCount",
            "output_tokens",
            "outputTokenCount",
        ],
    );
    let cache = usage
        .get("prompt_tokens_details")
        .and_then(|details| details.get("cached_tokens"))
        .and_then(Value::as_i64)
        .or_else(|| usage.get("prompt_cache_hit_tokens").and_then(Value::as_i64))
        .or_else(|| {
            usage
                .get("cached_content_token_count")
                .and_then(Value::as_i64)
        })
        .unwrap_or(0);
    Usage {
        input_tokens: input,
        cache_tokens: cache.max(0),
        output_tokens: output,
    }
}

fn first_i64(value: &Value, keys: &[&str]) -> Option<i64> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_i64))
}

/// 从 OpenAI 兼容错误体提取 message（convert::extract_error_message 的别名场景已覆盖）。
pub fn error_message(body: &str) -> String {
    super::extract_error_message(body)
}

/// 判定是否为 usage-only 尾块（include_usage 注入产生：choices 为空数组 +
/// usage 对象）。客户端未请求 include_usage 时应从直通流中过滤。
pub fn is_usage_only_chunk(event: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(event) else {
        return false;
    };
    value.get("usage").is_some_and(Value::is_object)
        && value
            .get("choices")
            .and_then(Value::as_array)
            .is_some_and(|choices| choices.is_empty())
}

/// OpenAI 兼容透传的思考字段归一：部分聚合上游（OpenRouter/Command Code 风格）
/// 用 `delta.reasoning`/`message.reasoning` 携带思考文本，统一改写为 DeepSeek
/// 事实标准的 `reasoning_content`；已是 reasoning_content 或字段非字符串时不动。
pub fn normalize_reasoning_value(value: &mut Value) -> bool {
    let Some(choices) = value.get_mut("choices").and_then(Value::as_array_mut) else {
        return false;
    };
    let mut changed = false;
    for choice in choices {
        for key in ["delta", "message"] {
            let Some(map) = choice.get_mut(key).and_then(Value::as_object_mut) else {
                continue;
            };
            if map.contains_key("reasoning_content") {
                continue;
            }
            if let Some(reasoning) = map.remove("reasoning") {
                if reasoning.is_string() {
                    map.insert("reasoning_content".to_string(), reasoning);
                    changed = true;
                } else {
                    map.insert("reasoning".to_string(), reasoning);
                }
            }
        }
    }
    changed
}

/// 归一单个 SSE 事件（JSON 文本）；非 JSON 或无需改写时原样返回。
pub fn normalize_reasoning_event(event: &str) -> String {
    match serde_json::from_str::<Value>(event) {
        Ok(mut value) => {
            if normalize_reasoning_value(&mut value) {
                value.to_string()
            } else {
                event.to_string()
            }
        }
        Err(_) => event.to_string(),
    }
}

/// OpenAI 兼容流式旁路扫描器：统计 usage 与内容 token 时刻，不改变转发字节。
#[derive(Debug, Default)]
pub struct OpenAiStreamScanner {
    splitter: SseSplitter,
    pub usage: Option<Usage>,
    pub saw_content: bool,
}

impl OpenAiStreamScanner {
    pub fn feed(&mut self, text: &str) {
        for event in self.splitter.feed(text) {
            self.feed_event(&event);
        }
    }

    pub fn feed_event(&mut self, event: &str) {
        if event == "[DONE]" {
            return;
        }
        let Ok(value) = serde_json::from_str::<Value>(event) else {
            return;
        };
        if let Some(usage_value) = value.get("usage")
            && usage_value.is_object()
            && (usage_value.get("prompt_tokens").is_some()
                || usage_value.get("input_tokens").is_some())
        {
            let usage = extract_usage(usage_value);
            if usage.input_tokens.is_some() || usage.output_tokens.is_some() {
                self.usage = Some(usage);
            }
        }
        // 内容判定与 chunk_has_content 对齐：content / reasoning_content / tool_calls
        // 任一非空即视为首个内容 token（推理模型与纯函数调用流同样计入 ttft）。
        if !self.saw_content
            && (value
                .pointer("/choices/0/delta/content")
                .and_then(Value::as_str)
                .is_some_and(|s| !s.is_empty())
                || value
                    .pointer("/choices/0/delta/reasoning_content")
                    .and_then(Value::as_str)
                    .is_some_and(|s| !s.is_empty())
                || value
                    .pointer("/choices/0/delta/tool_calls")
                    .and_then(Value::as_array)
                    .is_some_and(|calls| !calls.is_empty()))
        {
            self.saw_content = true;
            // 内容 token 时刻由调用方在 feed 时统一记录（见 StreamMetrics::on_token）。
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::from_str;

    #[test]
    fn is_usage_only_chunk_detects_spec_shape() {
        // OpenAI 规范：include_usage 注入产生的尾块 choices 为空数组。
        assert!(is_usage_only_chunk(
            r#"{"id":"x","object":"chat.completion.chunk","choices":[],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#
        ));
        assert!(!is_usage_only_chunk(
            r#"{"id":"x","choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}"#
        ));
        assert!(!is_usage_only_chunk(r#"{"usage":{"prompt_tokens":1}}"#));
        assert!(!is_usage_only_chunk("not json"));
        assert!(!is_usage_only_chunk("[DONE]"));
    }

    #[test]
    fn build_body_rewrites_model_and_injects_include_usage() {
        let chat = from_str::<Value>(r#"{"model":"vm-a","stream":true,"messages":[]}"#).unwrap();
        let body = build_request_body(&chat, "gpt-4o");
        assert_eq!(body["model"], "gpt-4o");
        assert_eq!(body["stream_options"]["include_usage"], true);

        let chat = from_str::<Value>(
            r#"{"model":"vm-a","stream":true,"stream_options":{"include_usage":true},"messages":[]}"#,
        )
        .unwrap();
        let body = build_request_body(&chat, "gpt-4o");
        assert_eq!(body["stream_options"]["include_usage"], true);

        let chat = from_str::<Value>(r#"{"model":"vm-a","messages":[]}"#).unwrap();
        let body = build_request_body(&chat, "gpt-4o");
        assert!(body.get("stream_options").is_none());
    }

    #[test]
    fn extract_usage_handles_aliases_and_cache() {
        let usage = extract_usage(&from_str::<Value>(
            r#"{"prompt_tokens":100,"completion_tokens":20,"prompt_tokens_details":{"cached_tokens":40}}"#,
        )
        .unwrap());
        assert_eq!(usage.input_tokens, Some(100));
        assert_eq!(usage.output_tokens, Some(20));
        assert_eq!(usage.cache_tokens, 40);

        let usage = extract_usage(
            &from_str::<Value>(
                r#"{"prompt_tokens":100,"completion_tokens":20,"prompt_cache_hit_tokens":60}"#,
            )
            .unwrap(),
        );
        assert_eq!(usage.cache_tokens, 60);
    }

    #[test]
    fn scanner_finds_usage_and_content() {
        let mut scanner = OpenAiStreamScanner::default();
        scanner.feed("data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n");
        scanner.feed("data: {\"choices\":[{\"delta\":{\"content\":\"你\"}}]}\n\n");
        scanner.feed(
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":2}}\n\n",
        );
        scanner.feed("data: [DONE]\n\n");
        assert!(scanner.saw_content);
        let usage = scanner.usage.expect("usage should be captured");
        assert_eq!(usage.input_tokens, Some(10));
        assert_eq!(usage.output_tokens, Some(2));
    }

    #[test]
    fn scanner_detects_reasoning_and_tool_content() {
        // 推理模型（reasoning_content）与纯函数调用（tool_calls）流同样应标记首 token。
        let mut reasoning = OpenAiStreamScanner::default();
        reasoning.feed("data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"思考\"}}]}\n\n");
        assert!(reasoning.saw_content);

        let mut tools = OpenAiStreamScanner::default();
        tools.feed(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"name\":\"f\"}}]}}]}\n\n",
        );
        assert!(tools.saw_content);
    }
    #[test]
    fn normalizes_reasoning_field_to_reasoning_content() {
        // 流式 delta.reasoning（OpenRouter/Command Code 风格）改写为 reasoning_content。
        let event = r#"{"choices":[{"delta":{"reasoning":"想一想"}}]}"#;
        let normalized = normalize_reasoning_event(event);
        assert!(
            normalized.contains(r#""reasoning_content":"想一想""#),
            "{normalized}"
        );
        assert!(!normalized.contains(r#""reasoning":""#), "{normalized}");

        // 已是 reasoning_content 时原样保留；非字符串 reasoning 不动。
        let kept =
            normalize_reasoning_event(r#"{"choices":[{"delta":{"reasoning_content":"已有"}}"#);
        assert!(kept.contains("已有"));
        let non_string =
            normalize_reasoning_event(r#"{"choices":[{"delta":{"reasoning":{"enabled":true}}}]}"#);
        assert!(non_string.contains(r#""reasoning":{"#));

        // 非 JSON 与无 choices 时原样返回。
        assert_eq!(normalize_reasoning_event("[DONE]"), "[DONE]");
        assert_eq!(
            normalize_reasoning_event(r#"{"usage":{}}"#),
            r#"{"usage":{}}"#
        );

        // 非流式 message.reasoning 同样归一。
        let mut value = serde_json::from_str::<Value>(
            r#"{"choices":[{"message":{"role":"assistant","reasoning":"想","content":"答"}}]}"#,
        )
        .unwrap();
        assert!(normalize_reasoning_value(&mut value));
        let message = &value["choices"][0]["message"];
        assert_eq!(message["reasoning_content"], "想");
        assert!(message.get("reasoning").is_none());
        assert_eq!(message["content"], "答");
    }
}

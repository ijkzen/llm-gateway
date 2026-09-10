//! OpenAI Compatible 出站：字节直通（仅重写 model + 注入 include_usage），
//! 以及 OpenAI 格式 usage 提取。

use serde_json::{Value, json};

use crate::proxy::metrics::Usage;
use crate::proxy::sse::SseSplitter;

/// 生成发往上游的请求体副本：重写 model；流式且客户端未开 include_usage 时注入；
/// 剥离 messages 中的 reasoning_details（网关内部无损载体，OpenAI 兼容上游
/// 以原生 reasoning_content/reasoning 字段工作，避免严格实现拒收未知字段）。
pub fn build_request_body(chat: &Value, actual_model: &str) -> Value {
    let mut body = chat.clone();
    if let Some(object) = body.as_object_mut() {
        object.insert("model".to_string(), json!(actual_model));
        if let Some(messages) = object.get_mut("messages").and_then(Value::as_array_mut) {
            for message in messages.iter_mut() {
                if let Some(message_object) = message.as_object_mut() {
                    message_object.remove("reasoning_details");
                }
            }
        }
        // reasoning 对象是 OpenRouter 形态，OpenAI 兼容上游不识别：
        // 归一为 reasoning_effort 简写透传并剥离原对象。明确关闭（Disabled）
        // 不注入：客户端直发的 reasoning_effort:"none" 本就字节透传，
        // 额外注入反而可能触达不支持 "none" 的上游。
        if let Some(reasoning) = super::chat_reasoning(chat).enabled() {
            object.insert(
                "reasoning_effort".to_string(),
                json!(reasoning.effort.clone()),
            );
        }
        object.remove("reasoning");
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
    let reasoning = usage
        .pointer("/completion_tokens_details/reasoning_tokens")
        .and_then(Value::as_i64);
    Usage {
        input_tokens: input,
        cache_tokens: cache.max(0),
        output_tokens: output,
        reasoning_tokens: reasoning,
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
///
/// 已知边界（03-09 拍板：文档化接受差异）：部分兼容厂商（DeepSeek 类）把
/// usage 并入**非空 choices 的终块**，本判定不命中，客户端会在未请求
/// include_usage 时多收一个带 usage 的内容块——浅泄漏，不影响正确性；扩展
/// 判定需先确证该类上游的终块形态。
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

/// OpenAI 兼容流式旁路扫描器：统计 usage 与内容 token 时刻，不改变转发字节。
#[derive(Debug, Default)]
pub struct OpenAiStreamScanner {
    splitter: SseSplitter,
    pub usage: Option<Usage>,
    pub saw_content: bool,
    /// 已见到上游 [DONE]：其后的读错误属连接 teardown 噪音（03-02），
    /// 内容已完整交付，不该翻转整单为失败。
    pub saw_done: bool,
}

impl OpenAiStreamScanner {
    pub fn feed(&mut self, text: &str) {
        for event in self.splitter.feed(text) {
            self.feed_event(&event);
        }
    }

    pub fn feed_event(&mut self, event: &str) {
        if event == "[DONE]" {
            self.saw_done = true;
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
    fn build_body_strips_reasoning_details() {
        // reasoning_details 是网关内部载体，直通上游前剥离。
        let chat = from_str::<Value>(
            r#"{"model":"vm-a","messages":[{"role":"user","content":"x"},{"role":"assistant","content":"hi","reasoning_details":[{"type":"reasoning.text","text":"想","signature":"sig","id":null,"format":"anthropic-claude-v1","index":0}]}]}"#,
        )
        .unwrap();
        let body = build_request_body(&chat, "gpt-4o");
        assert!(body["messages"][0].get("reasoning_details").is_none());
        assert!(body["messages"][1].get("reasoning_details").is_none());
        assert_eq!(body["messages"][1]["content"], "hi");
    }

    #[test]
    fn build_body_normalizes_reasoning_object() {
        // OpenRouter reasoning 对象归一为 reasoning_effort 简写并剥离原对象。
        let chat = from_str::<Value>(
            r#"{"model":"vm-a","messages":[],"reasoning":{"effort":"high","exclude":true}}"#,
        )
        .unwrap();
        let body = build_request_body(&chat, "gpt-4o");
        assert_eq!(body["reasoning_effort"], "high");
        assert!(body.get("reasoning").is_none());

        // 与显式 reasoning_effort 冲突时对象优先。
        let chat = from_str::<Value>(
            r#"{"model":"vm-a","messages":[],"reasoning":{"effort":"high"},"reasoning_effort":"low"}"#,
        )
        .unwrap();
        let body = build_request_body(&chat, "gpt-4o");
        assert_eq!(body["reasoning_effort"], "high");

        // effort none：不注入 reasoning_effort。
        let chat =
            from_str::<Value>(r#"{"model":"vm-a","messages":[],"reasoning":{"effort":"none"}}"#)
                .unwrap();
        let body = build_request_body(&chat, "gpt-4o");
        assert!(body.get("reasoning_effort").is_none());
        assert!(body.get("reasoning").is_none());
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
}

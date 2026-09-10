//! OpenAI chat → Anthropic Messages 转换。
//!
//! 映射参考 nyro 与 LiteLLM：system 合并为顶层 `system`；tool_calls → tool_use；
//! role=tool → tool_result；stop → stop_sequences；reasoning_effort → thinking 预算；
//! response_format(json) → 合成 JSON 工具；max_tokens 缺省 4096。

use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::proxy::sse::SseSplitter;

use super::{
    ANTHROPIC_DEFAULT_MAX_TOKENS, cached_client_usage_json, chat_max_tokens, chat_messages,
    chat_reasoning, inline_defs, message_text, reasoning_budget, truncate_chars,
};
use crate::proxy::metrics::Usage;

mod request;
mod response;

pub use request::{JSON_TOOL_NAME, build_request_body};
pub use response::{
    AnthropicStreamConverter, AnthropicStreamUsageScanner, convert_response, extract_usage,
    normalize_stop_reason, unwrap_json_tool_output,
};

#[cfg(test)]
mod tests;

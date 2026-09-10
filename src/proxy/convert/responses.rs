//! OpenAI chat → OpenAI Responses API 转换。
//!
//! 上游始终强制流式（`stream: true`，与 nyro/LiteLLM 一致：部分 Responses
//! 后端仅支持 SSE）；客户端请求非流式时由管线把 chunk 聚合回单个 JSON。

use std::collections::{HashMap, HashSet};

use serde_json::{Value, json};
use uuid::Uuid;

use crate::proxy::sse::SseSplitter;

use super::{
    ChatReasoning, chat_max_tokens, chat_messages, chat_reasoning, inline_defs, message_text,
};

mod request;
mod stream;

pub use request::build_request_body;
pub use stream::{ResponsesStreamConverter, ResponsesStreamUsageScanner};

#[cfg(test)]
mod tests;

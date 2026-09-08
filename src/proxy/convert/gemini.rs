//! OpenAI chat → Gemini generateContent / streamGenerateContent 转换。
//!
//! 映射参考 nyro 与 LiteLLM：system → systemInstruction；assistant → role "model"；
//! tool 结果 → functionResponse（name 用工具名，从上文 tool_call 反查）；
//! finishReason 全表映射（LiteLLM）；cachedContentTokenCount 计入缓存指标。

use std::collections::HashMap;

use base64::Engine as _;
use serde_json::{Map, Value, json};
use uuid::Uuid;

use super::{
    chat_max_tokens, chat_messages, chat_reasoning, collect_tool_call_names, inline_defs,
    message_text, reasoning_budget,
};

mod images;
mod request;
mod response;

pub use images::inline_remote_images;
pub use request::{build_request_body, generate_action, sanitize_gemini_schema};
pub use response::{GeminiStreamConverter, convert_response, extract_usage, map_finish_reason};

#[cfg(test)]
mod tests;

//! /v1 请求转发管线。
//!
//! 流程：Bearer 鉴权（中间件）→ 虚拟模型路由（display_id 精确匹配）→ 成员
//! 选择（LB 策略）→ 逐成员尝试（failover）→ 协议转换 → OpenAI 格式响应，
//! 并把每次请求的指标异步写入 request 表。
//!
//! 按转发阶段拆分为同目录子模块：`lb`（成员加载/LB 排序）、`headers`（出站头
//! 四层组装）、`calls`（上游调用组装）、`failover`（成员尝试循环）、`forward`
//! /`native`（转发入口）、`dispatch`（成功分派与落库）、`probe`（测速探测）。
//! 对外符号经 `pub use` 在模块根重导出，调用路径保持不变。

// 子文件与 tests 经 `use super::*` 共享本模块的导入与跨文件符号（拆分前同属一个
// 文件）；本文件内未直接使用的行由子文件消费，故整模块豁免 unused_imports。
#![allow(unused_imports)]

pub mod convert;
pub mod failure_recheck;
pub mod failure_recovery;
pub mod metrics;
pub mod pool;
pub mod sse;
pub mod upstream;
pub mod usage_rank;

mod calls;
mod dispatch;
mod failover;
mod forward;
mod headers;
mod lb;
mod native;
mod probe;
mod relay;
mod route;

pub use dispatch::accumulate_chunks;
pub use forward::{forward_chat, forward_chat_direct};
pub use headers::{is_never_outbound, select_forwardable_headers, select_passthrough_headers};
pub use lb::{LbState, Protocol};
pub use native::{NativeEndpoint, forward_native};
pub use probe::{ProbeFailure, probe_provider, test_model};

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};

use axum::body::Bytes;
use axum::http::header::{HeaderName, HeaderValue};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use http_body_util::BodyExt;
use rand::seq::SliceRandom;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

use crate::auth::{AuthedApiKey, openai_error};
use crate::crypto;
use crate::entity::{provider, provider_model, virtual_model, virtual_model_item};
use crate::provider_template;
use crate::proxy::convert::{
    RequestFlags, anthropic, attach_reasoning_details, build_upstream_url,
    cached_client_usage_json, chat_reasoning, chunk_json, extract_error_message, gemini, openai,
    responses, truncate_chars, usage_chunk_json,
};
use crate::proxy::metrics::{RequestRecord, StreamMetrics, Usage, now_ms};
use crate::proxy::pool::PooledBody;
use crate::proxy::upstream::{UpstreamCall, UpstreamReply};
use crate::state::AppState;
use crate::usage::persist::{fetch_and_store, read_usage_cache};
use crate::usage::types::{UsageData, UsageKind, WindowKind};

pub(crate) use calls::{build_native_upstream_call, build_upstream_call};
pub(crate) use dispatch::{collect_stream_events, dispatch_success, record_failure, sse_response};
pub(crate) use failover::{
    ForwardFlavor, MemberLoopOutcome, SuccessContext, forward_through_members,
};
pub(crate) use forward::build_member;
pub(crate) use headers::{
    NEVER_OUTBOUND, OPENCODE_SESSION_HEADER, THINKING_DROPPED_HEADER, apply_protocol_auth_headers,
    merge_custom_headers, merge_template_default_headers, opencode_session_fallback,
    protocol_auth_header_names, with_thinking_dropped_header,
};
pub(crate) use lb::{
    Member, format_usage, load_members, note_member_failure, order_members, rank_by_balance_with,
    rank_by_quota_with, resolve_proxy, resolve_usage_map,
};
pub(crate) use native::{NativeUsageScanner, dispatch_native_success};
pub(crate) use probe::{TEST_API_KEY_NAME, TEST_PROMPT, TEST_VIRTUAL_MODEL_ID};
pub(crate) use relay::{Converter, PumpSource, RecordCtx, StreamOutcome, TailSpec, relay_stream};
pub(crate) use route::{RouteError, resolve_and_order};

#[cfg(test)]
mod tests;

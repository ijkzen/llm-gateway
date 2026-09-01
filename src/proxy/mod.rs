//! /v1 请求转发管线。
//!
//! 流程：Bearer 鉴权（中间件）→ 虚拟模型路由（display_id 精确匹配）→ 成员
//! 选择（LB 策略）→ 逐成员尝试（failover）→ 协议转换 → OpenAI 格式响应，
//! 并把每次请求的指标异步写入 request 表。

pub mod convert;
pub mod metrics;
pub mod pool;
pub mod sse;
pub mod upstream;
pub mod usage_rank;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};

use axum::body::Bytes;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use http_body_util::BodyExt;
use hyper::header::HeaderName;
use rand::seq::SliceRandom;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

use crate::auth::{AuthedApiKey, openai_error};
use crate::crypto;
use crate::entity::{provider, provider_model, virtual_model, virtual_model_item};
use crate::proxy::convert::{
    anthropic, build_upstream_url, chunk_json, extract_error_message, gemini, openai, responses,
    truncate_chars, usage_chunk_json,
};
use crate::proxy::metrics::{RequestRecord, StreamMetrics, Usage, now_ms};
use crate::proxy::pool::PooledBody;
use crate::proxy::upstream::{UpstreamCall, UpstreamReply};
use crate::state::AppState;
use crate::usage::persist::{fetch_and_store, read_usage_cache};
use crate::usage::types::{UsageData, UsageKind, WindowKind};

/// 上游协议（provider.protocol_type）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    OpenAiCompat,
    OpenAiResponses,
    Anthropic,
    Gemini,
}

impl Protocol {
    pub fn from_i32(value: i32) -> Self {
        match value {
            crate::provider_model::refresh::PROTOCOL_OPENAI_RESPONSE => Protocol::OpenAiResponses,
            crate::provider_model::refresh::PROTOCOL_ANTHROPIC => Protocol::Anthropic,
            crate::provider_model::refresh::PROTOCOL_GEMINI => Protocol::Gemini,
            _ => Protocol::OpenAiCompat,
        }
    }
}

/// 可重试的失败路径：LLM 网关惯用的 408/429/5xx（nyro 同款）。
fn is_retryable_status(status: StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 429 | 500 | 502 | 503 | 529)
}

/// LB 轮转状态：虚拟模型 id → 已轮转次数。
#[derive(Clone, Default)]
pub struct LbState {
    counters: Arc<Mutex<HashMap<i32, u64>>>,
}

impl LbState {
    fn next_offset(&self, virtual_model_id: i32, len: usize) -> usize {
        if len == 0 {
            return 0;
        }
        let mut counters = self.counters.lock().expect("lb counters lock");
        let counter = counters.entry(virtual_model_id).or_insert(0);
        let offset = (*counter % len as u64) as usize;
        *counter += 1;
        offset
    }
}

/// 参与转发的一个成员（供应商 + 真实模型）。
#[derive(Debug, Clone)]
struct Member {
    provider_id: i32,
    /// 发给上游的真实模型 ID（provider_model.provider_model_id）。
    model_id: String,
    protocol: Protocol,
    /// 0=按量付费，1=订阅制。
    billing_mode: i32,
    base_url: String,
    api_key_encrypted: String,
    custom_header: String,
    /// 是否经网络代理转发该供应商请求。
    proxy_enabled: bool,
    /// HTTP 代理地址（如 `http://127.0.0.1:7890`）。
    proxy_addr: String,
}

/// 加载虚拟模型全部可用成员（item 启用 + 供应商启用且状态可用）。
async fn load_members(
    db: &DatabaseConnection,
    virtual_model_id: i32,
) -> Result<Vec<Member>, sea_orm::DbErr> {
    let items = virtual_model_item::Entity::find()
        .filter(virtual_model_item::Column::VirtualModelId.eq(virtual_model_id))
        .filter(virtual_model_item::Column::Enable.eq(true))
        .order_by_asc(virtual_model_item::Column::VirtualModelItemId)
        .all(db)
        .await?;
    if items.is_empty() {
        return Ok(Vec::new());
    }

    let model_pks: Vec<i32> = items.iter().map(|item| item.model_id).collect();
    let models = provider_model::Entity::find()
        .filter(provider_model::Column::ModelId.is_in(model_pks.clone()))
        .all(db)
        .await?;
    let model_by_pk: HashMap<i32, provider_model::Model> =
        models.into_iter().map(|m| (m.model_id, m)).collect();

    let provider_ids: Vec<i32> = {
        let ids: HashSet<i32> = model_by_pk.values().map(|m| m.provider_id).collect();
        ids.into_iter().collect()
    };
    let providers = provider::Entity::find()
        .filter(provider::Column::Id.is_in(provider_ids))
        .all(db)
        .await?;
    let provider_by_id: HashMap<i32, provider::Model> =
        providers.into_iter().map(|p| (p.id, p)).collect();

    Ok(items
        .iter()
        .filter_map(|item| {
            let model = model_by_pk.get(&item.model_id)?;
            let p = provider_by_id.get(&model.provider_id)?;
            if !p.enable || p.status != 0 {
                return None;
            }
            Some(Member {
                provider_id: p.id,
                model_id: model.provider_model_id.clone(),
                protocol: Protocol::from_i32(p.protocol_type),
                billing_mode: p.billing_mode,
                base_url: p.base_url.clone(),
                api_key_encrypted: p.api_key.clone(),
                custom_header: p.custom_header.clone(),
                proxy_enabled: p.proxy_enabled,
                proxy_addr: p.proxy_addr.clone(),
            })
        })
        .collect())
}

/// 把用量数据格式化为可读字符串，用于 LB 决策日志：
/// - 订阅制：`quota[5h:92.4%,week:70.3%,mon:85.2%]`（无数据的窗口显示 `-`）
/// - 按量：`balance=7.65`（余额合计）
/// - 无数据：`no-usage`
fn format_usage(data: Option<&UsageData>) -> String {
    let Some(data) = data else {
        return "no-usage".to_string();
    };
    match data.kind {
        UsageKind::Quota => {
            let parts: Vec<String> = [
                WindowKind::FiveHour,
                WindowKind::Weekly,
                WindowKind::Monthly,
            ]
            .iter()
            .map(|kind| {
                let pct = data
                    .windows
                    .iter()
                    .find(|w| w.window == *kind)
                    .and_then(|w| w.remaining_percent_value());
                match pct {
                    Some(p) => format!("{p}"),
                    None => "-".to_string(),
                }
            })
            .collect();
            format!("quota[5h:{},week:{},mon:{}]", parts[0], parts[1], parts[2])
        }
        UsageKind::Balance => format!("balance={:.2}", usage_rank::balance_amount(Some(data))),
    }
}

/// 按虚拟模型的负载均衡策略排序成员。
///
/// 策略 0/1 分组后做组内用量感知排序（订阅制按 5h→周→月剩余百分比逐层比较、
/// 全平随机；按量付费按剩余金额降序），用量优先取 10 分钟数据库缓存，缺失/
/// 过期才真实抓取。排序结果即 failover 优先级（`forward_chat` 按 ordered 顺序
/// 逐个重试）。策略 2/3 保持轮转/随机。
async fn order_members(
    state: &AppState,
    members: Vec<Member>,
    strategy: i32,
    lb_state: &LbState,
    virtual_model_id: i32,
    request_id: &str,
) -> Vec<Member> {
    match strategy {
        // 订阅制优先 / 按量优先：先按付费模式分组，再组内按剩余用量排序。
        0 | 1 => {
            let subscription_first = strategy == 0;
            let mut subs = Vec::new();
            let mut payg = Vec::new();
            for member in members {
                let group = if member.billing_mode == 1 {
                    &mut subs
                } else {
                    &mut payg
                };
                group.push(member);
            }
            // 决策过程日志：先打印订阅制与按量两组所有成员的用量明细，
            // 再打印排序后的顺序，便于事后还原「为什么选它」。
            // 用量一次解析全部成员（10 分钟缓存/抓取），两组共用同一份。
            let usage_map = resolve_usage_map(
                state,
                &subs.iter().chain(&payg).cloned().collect::<Vec<_>>(),
            )
            .await;
            let member_detail = |member: &Member, usage: &HashMap<i32, Option<UsageData>>| {
                format!(
                    "{}:{} billing={} {}",
                    member.provider_id,
                    member.model_id,
                    member.billing_mode,
                    format_usage(usage.get(&member.provider_id).and_then(Option::as_ref)),
                )
            };
            let subs_desc: Vec<String> =
                subs.iter().map(|m| member_detail(m, &usage_map)).collect();
            let payg_desc: Vec<String> =
                payg.iter().map(|m| member_detail(m, &usage_map)).collect();
            tracing::info!(
                request_id,
                virtual_model_id,
                strategy,
                subscription_first,
                subscription_members = ?subs_desc,
                payg_members = ?payg_desc,
                "LB 决策：成员用量明细",
            );

            let mut subs = rank_by_quota_with(state, subs, &usage_map).await;
            let mut payg = rank_by_balance_with(state, payg, &usage_map).await;
            // 订阅制额度耗尽即跳过：任一已提供窗口剩余为 0 的订阅成员视为当前
            // 不可用（与用量门控 apply_usage_gate 同口径），从候选里剔除，让位给
            // 还有额度的订阅成员或按量成员；无法判定（无窗口数据）的保持原状。
            let mut skipped: Vec<String> = Vec::new();
            subs.retain(|m| {
                let usable = usage_map
                    .get(&m.provider_id)
                    .and_then(Option::as_ref)
                    .and_then(UsageData::subscription_usable);
                match usable {
                    Some(false) => {
                        skipped.push(member_detail(m, &usage_map));
                        false
                    }
                    _ => true,
                }
            });
            // 按量付费余额耗尽即跳过：查得到余额且合计为 0 的按量成员不可用
            // （与订阅制同口径），从候选剔除；查不到余额（无法判定）的保持原状。
            let mut skipped_balance: Vec<String> = Vec::new();
            payg.retain(|m| {
                let usable = usage_map
                    .get(&m.provider_id)
                    .and_then(Option::as_ref)
                    .and_then(UsageData::balance_usable);
                match usable {
                    Some(false) => {
                        skipped_balance.push(member_detail(m, &usage_map));
                        false
                    }
                    _ => true,
                }
            });
            let (first_group, second_group) = if subscription_first {
                (subs.as_slice(), payg.as_slice())
            } else {
                (payg.as_slice(), subs.as_slice())
            };
            let ordered_desc: Vec<String> = first_group
                .iter()
                .chain(second_group)
                .map(|m| member_detail(m, &usage_map))
                .collect();
            tracing::info!(
                request_id,
                virtual_model_id,
                strategy,
                skipped_quota_exhausted = ?skipped,
                skipped_balance_exhausted = ?skipped_balance,
                ordered = ?ordered_desc,
                "LB 决策：排序结果",
            );

            if subscription_first {
                subs.append(&mut payg);
                subs
            } else {
                payg.append(&mut subs);
                payg
            }
        }
        // RoundRobin
        2 => {
            let len = members.len();
            if len <= 1 {
                return members;
            }
            let offset = lb_state.next_offset(virtual_model_id, len);
            let mut rotated: Vec<Member> = members[offset..].to_vec();
            rotated.extend_from_slice(&members[..offset]);
            rotated
        }
        // Random
        3 => {
            let mut shuffled = members;
            shuffled.shuffle(&mut rand::thread_rng());
            shuffled
        }
        _ => members,
    }
}

/// 订阅制组内排序：剩余百分比 5h→周→月 降序。先 shuffle 再稳定排序，
/// 三层全平的成员保持随机相对顺序（即“同等条件随机选一个”）。
/// `usage` 由调用方已解析（决策日志共用同一份，避免重复抓取）。
async fn rank_by_quota_with(
    _state: &AppState,
    mut members: Vec<Member>,
    usage: &HashMap<i32, Option<UsageData>>,
) -> Vec<Member> {
    if members.len() <= 1 {
        return members;
    }
    members.shuffle(&mut rand::thread_rng());
    // 自然序比较器（a vs b）；sort_by 升序，因此交换参数实现「剩余多的在前」。
    members.sort_by(|a, b| {
        usage_rank::cmp_quota_remaining(
            usage.get(&b.provider_id).and_then(Option::as_ref),
            usage.get(&a.provider_id).and_then(Option::as_ref),
        )
    });
    members
}

/// 按量付费组内排序：剩余金额合计降序（同额保持原序）。
/// `usage` 由调用方已解析（决策日志共用同一份，避免重复抓取）。
async fn rank_by_balance_with(
    _state: &AppState,
    mut members: Vec<Member>,
    usage: &HashMap<i32, Option<UsageData>>,
) -> Vec<Member> {
    if members.len() <= 1 {
        return members;
    }
    members.sort_by(|a, b| {
        usage_rank::cmp_balance(
            usage.get(&b.provider_id).and_then(Option::as_ref),
            usage.get(&a.provider_id).and_then(Option::as_ref),
        )
    });
    members
}

/// 收集成员用量：10 分钟数据库缓存新鲜即用；缺失/过期并发真实抓取并落库，
/// 抓取失败按无数据处理（排在本组末尾）。
async fn resolve_usage_map(
    state: &AppState,
    members: &[Member],
) -> HashMap<i32, Option<UsageData>> {
    let mut seen = HashSet::new();
    let provider_ids: Vec<i32> = members
        .iter()
        .map(|m| m.provider_id)
        .filter(|id| seen.insert(*id))
        .collect();

    let mut map = HashMap::new();
    let mut stale = Vec::new();
    for id in provider_ids {
        let cached = read_usage_cache(&state.db, id).await.ok().flatten();
        if let Some(data) = cached {
            map.insert(id, Some(data));
        } else {
            stale.push(id);
        }
    }
    if stale.is_empty() {
        return map;
    }

    let mut set = tokio::task::JoinSet::new();
    for id in stale {
        let db = state.db.clone();
        set.spawn(async move { (id, fetch_and_store(&db, id).await.ok()) });
    }
    while let Some(outcome) = set.join_next().await {
        if let Ok((id, data)) = outcome {
            map.insert(id, data);
        }
    }
    map
}

/// 组装发往上游的请求。返回 (调用, Anthropic 是否注入了 JSON 模式合成工具)。
fn build_upstream_call(
    member: &Member,
    chat: &Value,
    client_stream: bool,
    api_key: &str,
) -> Result<(UpstreamCall, bool), String> {
    let (body, json_mode_tool, sub_path, auth_headers): (
        Value,
        bool,
        String,
        Vec<(String, String)>,
    ) = match member.protocol {
        Protocol::OpenAiCompat => {
            let body = openai::build_request_body(chat, &member.model_id);
            (
                body,
                false,
                "chat/completions".to_string(),
                vec![("authorization".to_string(), format!("Bearer {api_key}"))],
            )
        }
        Protocol::OpenAiResponses => {
            let body = responses::build_request_body(chat, &member.model_id)?;
            (
                body,
                false,
                "responses".to_string(),
                vec![("authorization".to_string(), format!("Bearer {api_key}"))],
            )
        }
        Protocol::Anthropic => {
            let (body, json_mode_tool) = anthropic::build_request_body(chat, &member.model_id)?;
            (
                body,
                json_mode_tool,
                "messages".to_string(),
                vec![
                    ("x-api-key".to_string(), api_key.to_string()),
                    ("anthropic-version".to_string(), "2023-06-01".to_string()),
                ],
            )
        }
        Protocol::Gemini => {
            let body = gemini::build_request_body(chat, &member.model_id)?;
            let action = gemini::generate_action(client_stream);
            let model_path = if member.model_id.starts_with("models/") {
                member.model_id.clone()
            } else {
                format!("models/{}", member.model_id)
            };
            (
                body,
                false,
                format!("{model_path}:{action}"),
                vec![("x-goog-api-key".to_string(), api_key.to_string())],
            )
        }
    };

    let url = build_upstream_url(&member.base_url, member.protocol_code(), &sub_path);
    let body_bytes = Bytes::from(body.to_string());
    let mut headers: Vec<(HeaderName, HeaderValue)> = Vec::new();
    for (name, value) in auth_headers {
        headers.push((
            HeaderName::from_bytes(name.as_bytes())
                .map_err(|e| format!("无效请求头名 {name}：{e}"))?,
            HeaderValue::from_str(&value).map_err(|e| format!("无效请求头值：{e}"))?,
        ));
    }
    merge_custom_headers(&member.custom_header, &mut headers);

    Ok((
        UpstreamCall {
            url,
            headers,
            body: body_bytes,
            stream: client_stream || member.protocol == Protocol::OpenAiResponses,
        },
        json_mode_tool,
    ))
}

impl Member {
    fn protocol_code(&self) -> i32 {
        match self.protocol {
            Protocol::OpenAiCompat => crate::provider_model::refresh::PROTOCOL_OPENAI_COMPATIBLE,
            Protocol::OpenAiResponses => crate::provider_model::refresh::PROTOCOL_OPENAI_RESPONSE,
            Protocol::Anthropic => crate::provider_model::refresh::PROTOCOL_ANTHROPIC,
            Protocol::Gemini => crate::provider_model::refresh::PROTOCOL_GEMINI,
        }
    }
}

fn merge_custom_headers(custom_header: &str, headers: &mut Vec<(HeaderName, HeaderValue)>) {
    let Ok(value) = serde_json::from_str::<Value>(custom_header.trim()) else {
        return;
    };
    let Some(map) = value.as_object() else { return };
    for (name, header_value) in map {
        let Some(header_value) = header_value.as_str() else {
            continue;
        };
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(header_value),
        ) {
            headers.push((name, value));
        }
    }
}

/// 上游流式转换器（OpenAI Compat 直通场景不使用）。
enum Converter {
    Anthropic(Box<anthropic::AnthropicStreamConverter>),
    Responses(Box<responses::ResponsesStreamConverter>),
    Gemini(Box<gemini::GeminiStreamConverter>),
}

impl Converter {
    fn convert_event(&mut self, data: &str) -> Result<Vec<Value>, String> {
        match self {
            Converter::Anthropic(c) => c.convert_event(data),
            Converter::Responses(c) => c.convert_event(data),
            Converter::Gemini(c) => c.convert_event(data),
        }
    }

    fn usage(&self) -> Option<Usage> {
        match self {
            Converter::Anthropic(c) => c.usage().cloned(),
            Converter::Responses(c) => c.usage().cloned(),
            Converter::Gemini(c) => c.usage().cloned(),
        }
    }

    fn is_finished(&self) -> bool {
        match self {
            Converter::Anthropic(c) => c.is_finished(),
            Converter::Responses(c) => c.is_finished(),
            Converter::Gemini(c) => c.is_finished(),
        }
    }

    fn error(&self) -> Option<String> {
        match self {
            Converter::Anthropic(c) => c.error().cloned(),
            Converter::Responses(c) => c.error().cloned(),
            Converter::Gemini(c) => c.error().cloned(),
        }
    }

    fn has_finish(&self) -> bool {
        match self {
            Converter::Anthropic(c) => c.has_finish(),
            Converter::Responses(c) => c.has_finish(),
            Converter::Gemini(c) => c.has_finish(),
        }
    }

    fn final_chunk(&mut self) -> Option<Value> {
        match self {
            Converter::Anthropic(_) => None,
            Converter::Responses(_) => None,
            Converter::Gemini(c) => c.final_chunk(),
        }
    }

    fn completion_id(&self) -> String {
        match self {
            Converter::Anthropic(c) => c.completion_id().to_string(),
            Converter::Responses(c) => c.completion_id().to_string(),
            Converter::Gemini(c) => c.completion_id().to_string(),
        }
    }
}

/// chunk 是否携带内容（用于 ttft / 末 token 时刻统计）。
fn chunk_has_content(chunk: &Value) -> bool {
    let delta = chunk.pointer("/choices/0/delta");
    let Some(delta) = delta else { return false };
    ["content", "reasoning_content"].iter().any(|key| {
        delta
            .get(*key)
            .and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty())
    }) || delta
        .get("tool_calls")
        .and_then(Value::as_array)
        .is_some_and(|calls| !calls.is_empty())
}

/// 把流式转换出的 chunk 列表聚合为非流式 chat.completion（Responses 出站非流式路径）。
pub fn accumulate_chunks(chunks: &[Value], usage: &Usage) -> Value {
    let mut id = String::from("chatcmpl");
    let mut model = String::new();
    let mut content = String::new();
    let mut reasoning = String::new();
    let mut tool_calls: BTreeMap<i64, (String, String, String)> = BTreeMap::new();
    let mut finish_reason = "stop".to_string();
    let mut created = 0i64;

    for chunk in chunks {
        if id == "chatcmpl" {
            if let Some(chunk_id) = chunk.get("id").and_then(Value::as_str) {
                id = chunk_id.to_string();
            }
            if let Some(chunk_model) = chunk.get("model").and_then(Value::as_str) {
                model = chunk_model.to_string();
            }
            created = chunk.get("created").and_then(Value::as_i64).unwrap_or(0);
        }
        let Some(choice) = chunk.pointer("/choices/0") else {
            continue;
        };
        if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
            finish_reason = reason.to_string();
        }
        let Some(delta) = choice.get("delta") else {
            continue;
        };
        if let Some(text) = delta.get("content").and_then(Value::as_str) {
            content.push_str(text);
        }
        if let Some(text) = delta.get("reasoning_content").and_then(Value::as_str) {
            reasoning.push_str(text);
        }
        if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
            for call in calls {
                let index = call.get("index").and_then(Value::as_i64).unwrap_or(0);
                let entry = tool_calls.entry(index).or_default();
                if let Some(call_id) = call.get("id").and_then(Value::as_str)
                    && !call_id.is_empty()
                {
                    entry.0 = call_id.to_string();
                }
                if let Some(name) = call.pointer("/function/name").and_then(Value::as_str)
                    && !name.is_empty()
                {
                    entry.1 = name.to_string();
                }
                if let Some(arguments) = call.pointer("/function/arguments").and_then(Value::as_str)
                {
                    entry.2.push_str(arguments);
                }
            }
        }
    }

    let mut message = serde_json::Map::new();
    message.insert("role".to_string(), json!("assistant"));
    message.insert(
        "content".to_string(),
        if content.is_empty() && tool_calls.is_empty() {
            Value::Null
        } else {
            json!(content)
        },
    );
    if !tool_calls.is_empty() {
        let calls: Vec<Value> = tool_calls
            .into_iter()
            .enumerate()
            .map(|(position, (_, (call_id, name, arguments)))| {
                json!({
                    "id": call_id,
                    "type": "function",
                    "index": position,
                    "function": {"name": name, "arguments": arguments},
                })
            })
            .collect();
        message.insert("tool_calls".to_string(), Value::Array(calls));
    }
    if !reasoning.is_empty() {
        message.insert("reasoning_content".to_string(), json!(reasoning));
    }

    json!({
        "id": id,
        "object": "chat.completion",
        "created": created,
        "model": model,
        "choices": [{
            "index": 0,
            "message": Value::Object(message),
            "finish_reason": finish_reason,
        }],
        "usage": convert::client_usage_json(usage),
    })
}

/// 转发入口：处理 POST /v1/chat/completions。
pub async fn forward_chat(state: &AppState, api_key: AuthedApiKey, client_body: Value) -> Response {
    let request_id = Uuid::new_v4().to_string();
    let requested_model = client_body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let client_stream = client_body
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let include_usage = client_body
        .pointer("/stream_options/include_usage")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    if requested_model.is_empty() {
        return openai_error(
            StatusCode::BAD_REQUEST,
            "请求缺少 model 字段",
            "invalid_request_error",
            "invalid_request",
        );
    }

    // 路由：display_id 精确匹配（鉴权失败与路由未命中不落 request 表）。
    let virtual_model = match virtual_model::Entity::find()
        .filter(virtual_model::Column::DisplayId.eq(&requested_model))
        .filter(virtual_model::Column::Enable.eq(true))
        .one(&state.db)
        .await
    {
        Ok(Some(model)) => model,
        Ok(None) => {
            return openai_error(
                StatusCode::NOT_FOUND,
                format!("The model '{requested_model}' does not exist"),
                "invalid_request_error",
                "model_not_found",
            );
        }
        Err(e) => {
            return openai_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("查询虚拟模型失败：{e}"),
                "server_error",
                "internal_error",
            );
        }
    };

    let members = match load_members(&state.db, virtual_model.virtual_model_id).await {
        Ok(members) => members,
        Err(e) => {
            return openai_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("查询模型成员失败：{e}"),
                "server_error",
                "internal_error",
            );
        }
    };
    if members.is_empty() {
        return openai_error(
            StatusCode::SERVICE_UNAVAILABLE,
            format!("虚拟模型 '{requested_model}' 没有可用的成员"),
            "server_error",
            "no_available_members",
        );
    }

    let ordered = order_members(
        state,
        members,
        virtual_model.load_balancing_strategy,
        &state.lb_state,
        virtual_model.virtual_model_id,
        &request_id,
    )
    .await;
    if ordered.is_empty() {
        tracing::warn!(
            request_id,
            virtual_model_id = virtual_model.virtual_model_id,
            requested_model = %requested_model,
            "虚拟模型成员全部因额度耗尽不可用，无可用候选",
        );
        return openai_error(
            StatusCode::SERVICE_UNAVAILABLE,
            format!("虚拟模型 '{requested_model}' 没有可用的成员（订阅制额度均已耗尽）"),
            "server_error",
            "no_available_members",
        );
    }
    let retry_enabled = virtual_model.fallback_strategy == 1;

    // 负载均衡决策日志：选路结果每请求 1 条 info；完整排序明细 debug
    // （默认 RUST_LOG=info 不输出，深排时临时调 debug）。
    let ordered_desc: Vec<String> = ordered
        .iter()
        .map(|m| format!("{}:{}", m.provider_id, m.model_id))
        .collect();
    tracing::debug!(
        request_id,
        virtual_model_id = virtual_model.virtual_model_id,
        requested_model = %requested_model,
        strategy = virtual_model.load_balancing_strategy,
        member_order = ?ordered_desc,
        "LB 排序明细",
    );
    if let Some(first) = ordered.first() {
        tracing::info!(
            request_id,
            virtual_model_id = virtual_model.virtual_model_id,
            requested_model = %requested_model,
            strategy = virtual_model.load_balancing_strategy,
            member_count = ordered.len(),
            selected_provider_id = first.provider_id,
            selected_model_id = %first.model_id,
            "LB 选路结果",
        );
    }

    let mut last_failure: Option<(Member, String, StatusCode)> = None;
    for (index, member) in ordered.iter().enumerate() {
        let has_more = index + 1 < ordered.len();
        let start_time = now_ms();
        let decrypted_key = match crypto::decrypt(&member.api_key_encrypted) {
            Ok(key) => key,
            Err(e) => {
                let message = format!("解密供应商密钥失败：{e}");
                if retry_enabled && has_more {
                    tracing::warn!(
                        request_id,
                        virtual_model_id = virtual_model.virtual_model_id,
                        provider_id = member.provider_id,
                        model_id = %member.model_id,
                        attempt_index = index,
                        fail_reason = %message,
                        "上游成员失败，降级重试下一成员",
                    );
                    last_failure = Some((member.clone(), message, StatusCode::BAD_GATEWAY));
                    continue;
                }
                record_failure(
                    &state.db,
                    &request_id,
                    virtual_model.virtual_model_id,
                    member,
                    &api_key.name,
                    start_time,
                    false,
                    client_stream,
                    &message,
                    start_time,
                );
                return openai_error(
                    StatusCode::BAD_GATEWAY,
                    message,
                    "api_error",
                    "upstream_error",
                );
            }
        };

        let (call, json_mode_tool) =
            match build_upstream_call(member, &client_body, client_stream, &decrypted_key) {
                Ok(result) => result,
                Err(message) => {
                    if retry_enabled && has_more {
                        tracing::warn!(
                            request_id,
                            virtual_model_id = virtual_model.virtual_model_id,
                            provider_id = member.provider_id,
                            model_id = %member.model_id,
                            attempt_index = index,
                            fail_reason = %message,
                            "上游成员请求构造失败，降级重试下一成员",
                        );
                        last_failure = Some((member.clone(), message, StatusCode::BAD_GATEWAY));
                        continue;
                    }
                    record_failure(
                        &state.db,
                        &request_id,
                        virtual_model.virtual_model_id,
                        member,
                        &api_key.name,
                        start_time,
                        false,
                        client_stream,
                        &message,
                        start_time,
                    );
                    return openai_error(
                        StatusCode::BAD_GATEWAY,
                        message,
                        "api_error",
                        "upstream_error",
                    );
                }
            };

        // 供应商开启代理且地址有效时经 HTTP 代理转发。
        let proxy = if member.proxy_enabled && !member.proxy_addr.trim().is_empty() {
            Some(member.proxy_addr.as_str())
        } else {
            None
        };
        let reply = match upstream::call(call, &state.upstream_pool, proxy).await {
            Ok(reply) => reply,
            Err(e) => {
                let message = e.fail_reason();
                if retry_enabled && has_more {
                    tracing::warn!(
                        request_id,
                        virtual_model_id = virtual_model.virtual_model_id,
                        provider_id = member.provider_id,
                        model_id = %member.model_id,
                        attempt_index = index,
                        fail_reason = %message,
                        "上游成员调用失败，降级重试下一成员",
                    );
                    last_failure = Some((member.clone(), message, StatusCode::BAD_GATEWAY));
                    continue;
                }
                record_failure(
                    &state.db,
                    &request_id,
                    virtual_model.virtual_model_id,
                    member,
                    &api_key.name,
                    start_time,
                    false,
                    client_stream,
                    &message,
                    start_time,
                );
                return openai_error(
                    StatusCode::BAD_GATEWAY,
                    message,
                    "api_error",
                    "upstream_error",
                );
            }
        };

        if reply.status.as_u16() >= 400 {
            let body = upstream::read_body(reply.body).await.unwrap_or_default();
            let message = extract_error_message(&String::from_utf8_lossy(&body));
            let status = reply.status;
            if retry_enabled && is_retryable_status(status) && has_more {
                tracing::warn!(
                    request_id,
                    virtual_model_id = virtual_model.virtual_model_id,
                    provider_id = member.provider_id,
                    model_id = %member.model_id,
                    attempt_index = index,
                    http_status = status.as_u16(),
                    fail_reason = %message,
                    "上游成员返回可重试错误，降级重试下一成员",
                );
                last_failure = Some((member.clone(), message, status));
                continue;
            }
            record_failure(
                &state.db,
                &request_id,
                virtual_model.virtual_model_id,
                member,
                &api_key.name,
                start_time,
                false,
                client_stream,
                &message,
                reply.start_at_ms,
            );
            let error_type = if status.is_client_error() {
                "invalid_request_error"
            } else {
                "api_error"
            };
            return openai_error(status, message, error_type, "upstream_error");
        }

        // 成功：按协议与客户端流式标记分派响应路径。
        return dispatch_success(
            state,
            SuccessContext {
                request_id,
                virtual_model_id: virtual_model.virtual_model_id,
                api_key_name: api_key.name.clone(),
                requested_model: requested_model.clone(),
                start_time,
                member: member.clone(),
                reply,
                client_stream,
                include_usage,
                json_mode_tool,
            },
        )
        .await;
    }

    // 理论上不可达：循环内要么返回要么 continue；兜底返回最后失败。
    let (member, message, status) = last_failure.unwrap_or_else(|| {
        (
            ordered[0].clone(),
            "上游全部成员失败".to_string(),
            StatusCode::BAD_GATEWAY,
        )
    });
    tracing::error!(
        request_id,
        virtual_model_id = virtual_model.virtual_model_id,
        provider_id = member.provider_id,
        model_id = %member.model_id,
        http_status = status.as_u16(),
        fail_reason = %message,
        "虚拟模型全部成员失败",
    );
    let start_time = now_ms();
    record_failure(
        &state.db,
        &request_id,
        virtual_model.virtual_model_id,
        &member,
        &api_key.name,
        start_time,
        false,
        client_stream,
        &message,
        start_time,
    );
    openai_error(status, message, "api_error", "upstream_error")
}

/// 成功路径上下文。
struct SuccessContext {
    request_id: String,
    virtual_model_id: i32,
    api_key_name: String,
    requested_model: String,
    start_time: i64,
    member: Member,
    reply: UpstreamReply,
    client_stream: bool,
    include_usage: bool,
    json_mode_tool: bool,
}

#[allow(clippy::too_many_lines)]
async fn dispatch_success(state: &AppState, ctx: SuccessContext) -> Response {
    let SuccessContext {
        request_id,
        virtual_model_id,
        api_key_name,
        requested_model,
        start_time,
        member,
        mut reply,
        client_stream,
        include_usage,
        json_mode_tool,
    } = ctx;

    match (member.protocol, client_stream) {
        // OpenAI Compat 非流式：JSON 原样透传。
        (Protocol::OpenAiCompat, false) => {
            let body = upstream::read_body(reply.body).await.unwrap_or_default();
            let text = String::from_utf8_lossy(&body).to_string();
            let parsed: Value = serde_json::from_str(&text).unwrap_or_else(|_| json!({}));
            let usage = parsed
                .get("usage")
                .filter(|u| u.is_object())
                .map(openai::extract_usage)
                .unwrap_or_default();
            let body_done = now_ms();
            RequestRecord {
                request_id,
                virtual_model_id,
                provider_id: member.provider_id,
                model_id: member.model_id.clone(),
                stream: false,
                ttft: None,
                output_tokens_time: Some((body_done - reply.start_at_ms).max(0)),
                ttft_start_ms: reply.start_at_ms,
                start_time,
                end_time: body_done,
                usage,
                success: true,
                fail_reason: None,
                api_key_name,
            }
            .insert(&state.db);
            (StatusCode::OK, axum::Json(parsed)).into_response()
        }
        // OpenAI Compat 流式：字节直通 + 旁路扫描统计。
        (Protocol::OpenAiCompat, true) => {
            let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(32);
            let db = state.db.clone();
            let mut scanner = openai::OpenAiStreamScanner::default();
            let reply_start_at = reply.start_at_ms;
            let mut stream_metrics = StreamMetrics::new(reply.start_at_ms);
            tokio::spawn(async move {
                let mut body = reply.body;
                let mut disconnect = false;
                while let Some(frame) = body.frame().await {
                    let bytes = match frame {
                        Ok(frame) => frame.into_data().unwrap_or_default(),
                        Err(e) => {
                            let _ = tx.send(Err(std::io::Error::other(e.to_string()))).await;
                            break;
                        }
                    };
                    let text = String::from_utf8_lossy(&bytes).to_string();
                    scanner.feed(&text);
                    if scanner.saw_content {
                        scanner.saw_content = false;
                        stream_metrics.on_token();
                    }
                    if tx.send(Ok(bytes)).await.is_err() {
                        disconnect = true;
                        break;
                    }
                }
                let end_time = now_ms();
                let usage = scanner.usage.clone().unwrap_or_default();
                RequestRecord {
                    request_id,
                    virtual_model_id,
                    provider_id: member.provider_id,
                    model_id: member.model_id.clone(),
                    stream: true,
                    ttft: stream_metrics.ttft_ms(),
                    output_tokens_time: stream_metrics.output_duration_ms(),
                    ttft_start_ms: reply_start_at,
                    start_time,
                    end_time,
                    usage,
                    success: true,
                    fail_reason: disconnect.then(|| "客户端提前断开".to_string()),
                    api_key_name,
                }
                .insert(&db);
            });
            sse_response(ReceiverStream::new(rx))
        }
        // Responses 出站：上游强制流式。
        (Protocol::OpenAiResponses, _) => {
            let mut converter = Converter::Responses(Box::new(
                responses::ResponsesStreamConverter::new(&request_id, &requested_model),
            ));
            let events = collect_stream_events(
                &mut reply.body,
                &mut converter,
                &mut StreamMetrics::new(reply.start_at_ms),
            )
            .await;
            if let Some(error) = events.error {
                record_failure(
                    &state.db,
                    &request_id,
                    virtual_model_id,
                    &member,
                    &api_key_name,
                    start_time,
                    client_stream,
                    client_stream,
                    &error,
                    reply.start_at_ms,
                );
                let status = StatusCode::BAD_GATEWAY;
                return openai_error(status, error, "api_error", "upstream_error");
            }
            let usage = converter.usage().unwrap_or_default();
            let completion = accumulate_chunks(&events.chunks, &usage);
            let end_time = now_ms();
            let usage_for_chunk = usage.clone();
            RequestRecord {
                request_id: request_id.clone(),
                virtual_model_id,
                provider_id: member.provider_id,
                model_id: member.model_id.clone(),
                stream: client_stream,
                ttft: events.stream_metrics.ttft_ms(),
                output_tokens_time: if client_stream {
                    events.stream_metrics.output_duration_ms()
                } else {
                    Some((end_time - reply.start_at_ms).max(0))
                },
                ttft_start_ms: reply.start_at_ms,
                start_time,
                end_time,
                usage,
                success: true,
                fail_reason: events.disconnect.then(|| "客户端提前断开".to_string()),
                api_key_name,
            }
            .insert(&state.db);
            if client_stream {
                let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(32);
                let request_id_for_chunk = request_id.clone();
                tokio::spawn(async move {
                    for chunk in events.chunks {
                        if tx
                            .send(Ok(Bytes::from(crate::proxy::sse::sse_frame(
                                &chunk.to_string(),
                            ))))
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                    if include_usage {
                        let frame = crate::proxy::sse::sse_frame(
                            &usage_chunk_json(
                                &request_id_for_chunk,
                                &requested_model,
                                &usage_for_chunk,
                            )
                            .to_string(),
                        );
                        let _ = tx.send(Ok(Bytes::from(frame))).await;
                    }
                    let _ = tx.send(Ok(Bytes::from("data: [DONE]\n\n"))).await;
                });
                sse_response(ReceiverStream::new(rx))
            } else {
                (StatusCode::OK, axum::Json(completion)).into_response()
            }
        }
        // Anthropic / Gemini：非流式直接转换；流式逐事件转换后转发。
        (protocol, client_stream) => {
            let mut converter = match protocol {
                Protocol::Anthropic => {
                    Converter::Anthropic(Box::new(anthropic::AnthropicStreamConverter::new(
                        &request_id,
                        &requested_model,
                        json_mode_tool,
                    )))
                }
                Protocol::Gemini => Converter::Gemini(Box::new(
                    gemini::GeminiStreamConverter::new(&request_id, &requested_model),
                )),
                Protocol::OpenAiCompat | Protocol::OpenAiResponses => unreachable!("handled above"),
            };

            if !client_stream {
                let body = upstream::read_body(reply.body).await.unwrap_or_default();
                let text = String::from_utf8_lossy(&body).to_string();
                let parsed: Value = match serde_json::from_str(&text) {
                    Ok(value) => value,
                    Err(e) => {
                        let message = format!("解析上游响应失败：{e}");
                        record_failure(
                            &state.db,
                            &request_id,
                            virtual_model_id,
                            &member,
                            &api_key_name,
                            start_time,
                            false,
                            false,
                            &message,
                            reply.start_at_ms,
                        );
                        return openai_error(
                            StatusCode::BAD_GATEWAY,
                            message,
                            "api_error",
                            "upstream_error",
                        );
                    }
                };
                let converted = match protocol {
                    Protocol::Anthropic => anthropic::convert_response(
                        &parsed,
                        &request_id,
                        &requested_model,
                        json_mode_tool,
                    ),
                    Protocol::Gemini => {
                        gemini::convert_response(&parsed, &request_id, &requested_model)
                    }
                    _ => unreachable!(),
                };
                let body_done = now_ms();
                return match converted {
                    Ok((completion, usage)) => {
                        RequestRecord {
                            request_id,
                            virtual_model_id,
                            provider_id: member.provider_id,
                            model_id: member.model_id.clone(),
                            stream: false,
                            ttft: None,
                            output_tokens_time: Some((body_done - reply.start_at_ms).max(0)),
                            ttft_start_ms: reply.start_at_ms,
                            start_time,
                            end_time: body_done,
                            usage,
                            success: true,
                            fail_reason: None,
                            api_key_name,
                        }
                        .insert(&state.db);
                        (StatusCode::OK, axum::Json(completion)).into_response()
                    }
                    Err(message) => {
                        record_failure(
                            &state.db,
                            &request_id,
                            virtual_model_id,
                            &member,
                            &api_key_name,
                            start_time,
                            false,
                            false,
                            &message,
                            reply.start_at_ms,
                        );
                        openai_error(
                            StatusCode::BAD_GATEWAY,
                            message,
                            "api_error",
                            "upstream_error",
                        )
                    }
                };
            }

            // 流式：逐事件转换并推送给客户端。
            let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(32);
            let db = state.db.clone();
            let reply_start_at = reply.start_at_ms;
            let mut stream_metrics = StreamMetrics::new(reply.start_at_ms);
            tokio::spawn(async move {
                let mut body = reply.body;
                let mut splitter = crate::proxy::sse::SseSplitter::default();
                let mut disconnect = false;
                'outer: while let Some(frame) = body.frame().await {
                    let bytes = match frame {
                        Ok(frame) => frame.into_data().unwrap_or_default(),
                        Err(e) => {
                            let _ = tx.send(Err(std::io::Error::other(e.to_string()))).await;
                            break;
                        }
                    };
                    let text = String::from_utf8_lossy(&bytes).to_string();
                    for event in splitter.feed(&text) {
                        match converter.convert_event(&event) {
                            Ok(chunks) => {
                                for chunk in chunks {
                                    if chunk_has_content(&chunk) {
                                        stream_metrics.on_token();
                                    }
                                    let frame = crate::proxy::sse::sse_frame(&chunk.to_string());
                                    if tx.send(Ok(Bytes::from(frame))).await.is_err() {
                                        disconnect = true;
                                        break 'outer;
                                    }
                                }
                            }
                            Err(message) => {
                                let error_frame = format!(
                                    "data: {}\n\n",
                                    json!({"error": {"message": message, "type": "api_error", "code": "upstream_error"}})
                                );
                                let _ = tx.send(Ok(Bytes::from(error_frame))).await;
                                disconnect = false;
                                break 'outer;
                            }
                        }
                        if converter.is_finished() {
                            break 'outer;
                        }
                    }
                }
                // 补发缺失的 finish / usage / [DONE]。
                if converter.error().is_none() {
                    if let Some(chunk) = converter.final_chunk()
                        && tx
                            .send(Ok(Bytes::from(crate::proxy::sse::sse_frame(
                                &chunk.to_string(),
                            ))))
                            .await
                            .is_err()
                    {
                        disconnect = true;
                    }
                    if !converter.has_finish() {
                        let finish = chunk_json(
                            &converter.completion_id(),
                            &requested_model,
                            json!({}),
                            Some("stop"),
                        );
                        let _ = tx
                            .send(Ok(Bytes::from(crate::proxy::sse::sse_frame(
                                &finish.to_string(),
                            ))))
                            .await;
                    }
                    if include_usage && let Some(usage) = converter.usage() {
                        let frame =
                            usage_chunk_json(&converter.completion_id(), &requested_model, &usage)
                                .to_string();
                        let _ = tx
                            .send(Ok(Bytes::from(crate::proxy::sse::sse_frame(&frame))))
                            .await;
                    }
                }
                let _ = tx.send(Ok(Bytes::from("data: [DONE]\n\n"))).await;
                let end_time = now_ms();
                let success = converter.error().is_none();
                let usage = converter.usage().unwrap_or_default();
                RequestRecord {
                    request_id,
                    virtual_model_id,
                    provider_id: member.provider_id,
                    model_id: member.model_id.clone(),
                    stream: true,
                    ttft: stream_metrics.ttft_ms(),
                    output_tokens_time: stream_metrics.output_duration_ms(),
                    ttft_start_ms: reply_start_at,
                    start_time,
                    end_time,
                    usage,
                    success,
                    fail_reason: converter
                        .error()
                        .clone()
                        .or(disconnect.then(|| "客户端提前断开".to_string())),
                    api_key_name,
                }
                .insert(&db);
            });
            sse_response(ReceiverStream::new(rx))
        }
    }
}

/// 上游流式事件收集（Responses 聚合路径）。
struct CollectedEvents {
    chunks: Vec<Value>,
    stream_metrics: StreamMetrics,
    error: Option<String>,
    disconnect: bool,
}

async fn collect_stream_events(
    body: &mut PooledBody,
    converter: &mut Converter,
    stream_metrics: &mut StreamMetrics,
) -> CollectedEvents {
    let mut splitter = crate::proxy::sse::SseSplitter::default();
    let mut chunks = Vec::new();
    let mut error = None;
    let mut disconnect = false;
    'outer: while let Some(frame) = body.frame().await {
        let bytes = match frame {
            Ok(frame) => frame.into_data().unwrap_or_default(),
            Err(e) => {
                error = Some(format!("读取上游流失败：{e}"));
                break;
            }
        };
        let text = String::from_utf8_lossy(&bytes).to_string();
        for event in splitter.feed(&text) {
            match converter.convert_event(&event) {
                Ok(emitted) => {
                    for chunk in emitted {
                        if chunk_has_content(&chunk) {
                            stream_metrics.on_token();
                        }
                        chunks.push(chunk);
                    }
                }
                Err(message) => {
                    error = Some(message);
                    break 'outer;
                }
            }
            if converter.is_finished() {
                break 'outer;
            }
        }
    }
    if error.is_none()
        && let Some(converter_error) = converter.error()
    {
        error = Some(converter_error);
    }
    if error.is_some() {
        disconnect = false;
    }
    CollectedEvents {
        chunks,
        stream_metrics: std::mem::take(stream_metrics),
        error,
        disconnect,
    }
}

fn sse_response(stream: ReceiverStream<Result<Bytes, std::io::Error>>) -> Response {
    use axum::body::Body;
    (
        StatusCode::OK,
        [
            ("content-type", "text/event-stream"),
            ("cache-control", "no-cache"),
            ("connection", "keep-alive"),
        ],
        Body::from_stream(stream),
    )
        .into_response()
}

/// 失败请求的统一落库。
#[allow(clippy::too_many_arguments)]
fn record_failure(
    db: &DatabaseConnection,
    request_id: &str,
    virtual_model_id: i32,
    member: &Member,
    api_key_name: &str,
    start_time: i64,
    stream: bool,
    _client_stream: bool,
    message: &str,
    ttft_start_ms: i64,
) {
    RequestRecord {
        request_id: request_id.to_string(),
        virtual_model_id,
        provider_id: member.provider_id,
        model_id: member.model_id.clone(),
        stream,
        ttft: None,
        output_tokens_time: None,
        ttft_start_ms,
        start_time,
        end_time: now_ms(),
        usage: Usage::default(),
        success: false,
        fail_reason: Some(truncate_chars(message, 200)),
        api_key_name: api_key_name.to_string(),
    }
    .insert(db);
}

/// 测试请求写入 request 表时用于标记来源的虚拟模型 ID 与 API Key 名。
/// 测试流量不属于任何虚拟模型，虚拟模型维度记 0；api_key_name 记 `test` 便于数据面板区分。
const TEST_VIRTUAL_MODEL_ID: i32 = 0;
const TEST_API_KEY_NAME: &str = "test";

/// 测试提示词：固定「你好」。部分模型可能拒答或返回空文本，但只要上游
/// 受理请求（HTTP 2xx）即判定模型有效（连通性验证）。
const TEST_PROMPT: &str = "你好";

/// 手动构建最小化测试请求发往指定供应商模型的上游，验证模型可用性。
///
/// 复用 `build_upstream_call`（四协议转换 + Responses 强制流式）与连接池
/// （含代理）；成功/失败均写入 request 表（与正式流量同口径，计入数据面板）。
/// 成功不要求模型产出文本：上游返回 2xx 即视为有效；失败返回人类可读原因。
pub async fn test_model(
    state: &AppState,
    provider_row: &crate::entity::provider::Model,
    model: &crate::entity::provider_model::Model,
    api_key: &str,
) -> Result<(), String> {
    let member = Member {
        provider_id: provider_row.id,
        model_id: model.provider_model_id.clone(),
        protocol: Protocol::from_i32(provider_row.protocol_type),
        billing_mode: provider_row.billing_mode,
        base_url: provider_row.base_url.clone(),
        api_key_encrypted: provider_row.api_key.clone(),
        custom_header: provider_row.custom_header.clone(),
        proxy_enabled: provider_row.proxy_enabled,
        proxy_addr: provider_row.proxy_addr.clone(),
    };

    let chat = json!({
        "model": model.provider_model_id,
        "stream": false,
        "max_tokens": model.max_output_tokens,
        "messages": [{"role": "user", "content": TEST_PROMPT}],
    });
    let (call, _json_mode_tool) = build_upstream_call(&member, &chat, false, api_key)?;

    let proxy = if member.proxy_enabled && !member.proxy_addr.trim().is_empty() {
        Some(member.proxy_addr.as_str())
    } else {
        None
    };
    let start_time = now_ms();
    let reply = match upstream::call(call, &state.upstream_pool, proxy).await {
        Ok(reply) => reply,
        Err(e) => {
            let message = e.fail_reason();
            record_failure(
                &state.db,
                &format!("test-{}", Uuid::new_v4()),
                TEST_VIRTUAL_MODEL_ID,
                &member,
                TEST_API_KEY_NAME,
                start_time,
                false,
                false,
                &message,
                start_time,
            );
            return Err(message);
        }
    };

    if reply.status.as_u16() >= 400 {
        let body = upstream::read_body(reply.body).await.unwrap_or_default();
        let message = format!(
            "{} {}",
            reply.status.as_u16(),
            extract_error_message(&String::from_utf8_lossy(&body))
        );
        record_failure(
            &state.db,
            &format!("test-{}", Uuid::new_v4()),
            TEST_VIRTUAL_MODEL_ID,
            &member,
            TEST_API_KEY_NAME,
            start_time,
            false,
            false,
            &message,
            reply.start_at_ms,
        );
        return Err(message);
    }

    // 成功：读取响应体并提取 usage 落库。Responses 上游强制流式（SSE），
    // 需要逐事件解析出 usage；其余协议直接读 JSON。
    let usage = match member.protocol {
        Protocol::OpenAiResponses => {
            let body = upstream::read_body(reply.body).await.unwrap_or_default();
            let text = String::from_utf8_lossy(&body).to_string();
            let mut splitter = sse::SseSplitter::default();
            let mut usage = Usage::default();
            for event in splitter.feed(&text) {
                if let Ok(value) = serde_json::from_str::<Value>(&event)
                    && let Some(usage_value) = value.pointer("/response/usage")
                    && let Some(parsed) =
                        responses::ResponsesStreamConverter::extract_usage(usage_value)
                {
                    usage = parsed;
                }
            }
            usage
        }
        _ => {
            let body = upstream::read_body(reply.body).await.unwrap_or_default();
            let text = String::from_utf8_lossy(&body).to_string();
            let parsed: Value = serde_json::from_str(&text).unwrap_or_else(|_| json!({}));
            match member.protocol {
                Protocol::OpenAiCompat => parsed
                    .get("usage")
                    .filter(|u| u.is_object())
                    .map(openai::extract_usage)
                    .unwrap_or_default(),
                Protocol::Anthropic => parsed
                    .get("usage")
                    .map(anthropic::extract_usage)
                    .unwrap_or_default(),
                Protocol::Gemini => parsed
                    .get("usageMetadata")
                    .map(gemini::extract_usage)
                    .unwrap_or_default(),
                Protocol::OpenAiResponses => unreachable!("handled above"),
            }
        }
    };
    let end_time = now_ms();
    RequestRecord {
        request_id: format!("test-{}", Uuid::new_v4()),
        virtual_model_id: TEST_VIRTUAL_MODEL_ID,
        provider_id: member.provider_id,
        model_id: member.model_id.clone(),
        stream: false,
        ttft: None,
        output_tokens_time: Some((end_time - reply.start_at_ms).max(0)),
        ttft_start_ms: reply.start_at_ms,
        start_time,
        end_time,
        usage,
        success: true,
        fail_reason: None,
        api_key_name: TEST_API_KEY_NAME.to_string(),
    }
    .insert(&state.db);

    Ok(())
}

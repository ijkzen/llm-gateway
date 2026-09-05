//! /v1 请求转发管线。
//!
//! 流程：Bearer 鉴权（中间件）→ 虚拟模型路由（display_id 精确匹配）→ 成员
//! 选择（LB 策略）→ 逐成员尝试（failover）→ 协议转换 → OpenAI 格式响应，
//! 并把每次请求的指标异步写入 request 表。

pub mod convert;
pub mod failure_counter;
pub mod failure_recheck;
pub mod failure_recovery;
pub mod metrics;
pub mod pool;
pub mod sse;
pub mod upstream;
pub mod usage_rank;

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
    anthropic, build_upstream_url, cached_client_usage_json, chunk_json, client_usage_json,
    extract_error_message, gemini, openai, responses, truncate_chars, usage_chunk_json,
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

/// 成员请求失败后记连续失败（所有失败，含不可重试 4xx）；达到设置项
/// `max_consecutive_failures` 阈值时熔断停用供应商（原子化，详见
/// `provider_repo::disable_provider_on_failures`）。
/// `counted` 为本次请求已计数的 provider 集合：同一请求内同一供应商的多个
/// 成员失败只计一次，避免一次降级链把计数顶到阈值。
async fn note_member_failure(
    state: &AppState,
    member: &Member,
    request_id: &str,
    counted: &mut HashSet<i32>,
) {
    if !counted.insert(member.provider_id) {
        return;
    }
    let consecutive = state.failure_counter.record_failure(member.provider_id);
    // 失败复查（异步节流）：耗尽即门控禁用，切断缓存过期导致的后续降级。
    failure_recheck::trigger(state, member.provider_id, request_id);
    let threshold = state.settings.max_consecutive_failures().await;
    if consecutive >= threshold
        && let Err(e) = crate::provider_repo::disable_provider_on_failures(
            &state.db,
            member.provider_id,
            consecutive,
            request_id,
        )
        .await
    {
        tracing::warn!(
            request_id,
            provider_id = member.provider_id,
            "连续失败熔断执行失败：{e}"
        );
    }
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
    /// 该成员最终生效的网络代理（模型级优先，其次供应商级；都未开启则直连）。
    proxy_enabled: bool,
    /// HTTP 代理地址（如 `http://127.0.0.1:7890`）。
    proxy_addr: String,
}

/// 解析成员最终代理：模型级开启且地址有效 → 用模型地址；否则供应商级开启
/// 且地址有效 → 用供应商地址；都没有 → 直连。
fn resolve_proxy(model: &provider_model::Model, provider: &provider::Model) -> (bool, String) {
    if model.proxy_enabled && !model.proxy_addr.trim().is_empty() {
        (true, model.proxy_addr.clone())
    } else if provider.proxy_enabled && !provider.proxy_addr.trim().is_empty() {
        (true, provider.proxy_addr.clone())
    } else {
        (false, String::new())
    }
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
            if !p.enable {
                return None;
            }
            let (proxy_enabled, proxy_addr) = resolve_proxy(model, p);
            // 协议优先级：模型单独指定（非空）→ 供应商协议。与代理同款覆盖语义。
            let protocol_value = model.protocol_type.unwrap_or(p.protocol_type);
            Some(Member {
                provider_id: p.id,
                model_id: model.provider_model_id.clone(),
                protocol: Protocol::from_i32(protocol_value),
                billing_mode: p.billing_mode,
                base_url: p.base_url.clone(),
                api_key_encrypted: p.api_key.clone(),
                custom_header: p.custom_header.clone(),
                proxy_enabled,
                proxy_addr,
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

/// 订阅制组内排序：剩余百分比 5h→周→月 降序，同层打平比该层重置时间（早的优先）。
/// 先 shuffle 再稳定排序，全部平局的成员保持随机相对顺序（即“同等条件随机选一个”）。
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
///
/// Header 组装优先级（同名时高优先级者胜，低优先级只补缺）：
///   协议鉴权/必需头：网关生成，任何层不得覆盖（D3）
///   第 4 层：`forwarded` 下游 allowlist 透传（调用方已剥离/过滤，first-wins）
///   第 3 层：provider `custom_header`（不覆盖透传层同名）
///   模板默认头：按 base_url host 查漏补缺（不覆盖透传/custom_header 同名，
///            见 `provider_template::template_default_headers`）
///   第 1 层：框架头（`Host`/`Content-Type`/`Accept`/`Content-Length`）由
///            `upstream::send_upstream_request` 发送端唯一写入。
/// 另：`opencode.ai` 上游要求携带 `x-opencode-session` 会话头（OpenCode Go
/// 会话亲和，缺失部分后端直接 400）；全部组装完后若仍无该头，注入
/// `opencode_session` 回退值（仅对该 host 生效，不覆盖已有值）。
fn build_upstream_call(
    member: &Member,
    chat: &Value,
    client_stream: bool,
    api_key: &str,
    forwarded: &[(HeaderName, HeaderValue)],
    request_id: &str,
    opencode_session: &str,
) -> Result<(UpstreamCall, bool), String> {
    let (body, json_mode_tool, sub_path) = match member.protocol {
        Protocol::OpenAiCompat => {
            let body = openai::build_request_body(chat, &member.model_id);
            (body, false, "chat/completions".to_string())
        }
        Protocol::OpenAiResponses => {
            let body = responses::build_request_body(chat, &member.model_id)?;
            (body, false, "responses".to_string())
        }
        Protocol::Anthropic => {
            let (body, json_mode_tool) = anthropic::build_request_body(chat, &member.model_id)?;
            (body, json_mode_tool, "messages".to_string())
        }
        Protocol::Gemini => {
            let body = gemini::build_request_body(chat, &member.model_id)?;
            let action = gemini::generate_action(client_stream);
            let model_path = if member.model_id.starts_with("models/") {
                member.model_id.clone()
            } else {
                format!("models/{}", member.model_id)
            };
            (body, false, format!("{model_path}:{action}"))
        }
    };

    let url = build_upstream_url(&member.base_url, member.protocol_code(), &sub_path);
    let upstream_host = provider_template::host_of(&member.base_url).unwrap_or_default();
    let body_bytes = Bytes::from(body.to_string());
    let mut headers: Vec<(HeaderName, HeaderValue)> = Vec::new();
    // 第 4 层：下游透传子集（调用方已过滤）。
    headers.extend_from_slice(forwarded);
    // 第 3 层：provider custom_header（同名不覆盖透传层；协议保留名跳过并告警）。
    merge_custom_headers(
        &member.custom_header,
        member.protocol,
        request_id,
        &mut headers,
    );
    // 模板默认头：按 host 查漏补缺（同名以下游透传/custom_header 为准）。
    merge_template_default_headers(&upstream_host, &mut headers);
    // 第 2 层：协议鉴权/必需头（insert 覆盖以上所有层，D3）。
    apply_protocol_auth_headers(member.protocol, api_key, &mut headers);
    // OpenCode Go 会话亲和头：透传/custom_header 已带则不覆盖。
    if provider_template::is_opencode_host(&upstream_host)
        && !headers
            .iter()
            .any(|(n, _)| n.as_str().eq_ignore_ascii_case(OPENCODE_SESSION_HEADER))
        && let Ok(value) = HeaderValue::from_str(opencode_session)
    {
        headers.push((HeaderName::from_static(OPENCODE_SESSION_HEADER), value));
    }

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

// ─── 上游出站头组装：四层覆盖模型（见 .scratch/upstream-header-forwarding/） ───

/// 剥离清单：命中即不进上游（下游头与 custom_header 都受此约束）。
/// 分为两类：凭据名与协议保留名（绝不出站/不得作为自定义覆盖名），
/// 以及框架头保留名（`Host`/`Content-Length`/`Content-Type`/`accept` 由网关生成）。
/// 框架名也列入禁止名，避免组装层写入后与发送端第 1 层重复。
const NEVER_OUTBOUND: &[&str] = &[
    // 凭据 / 身份（下游与 custom_header 都不得带出）
    "authorization",
    "cookie",
    "proxy-authorization",
    "x-api-key",
    "x-goog-api-key",
    "x-amz-security-token",
    // hop-by-hop / 连接管理（RFC 9110 §7.6.1）
    "connection",
    "keep-alive",
    "proxy-connection",
    "upgrade",
    "te",
    "transfer-encoding",
    "trailer",
    "proxy-authenticate",
    // framing / 表示元数据（网关重新生成）
    "host",
    "content-length",
    "content-type",
    "accept",
    "content-encoding",
    "content-language",
    "content-md5",
    "expect",
    // 入站路由/链路头（会污染上游）
    "forwarded",
    "x-forwarded-for",
    "x-forwarded-proto",
    "x-forwarded-host",
    "via",
    "x-real-ip",
];

/// OpenCode Go 会话亲和头名（缺失时上游部分后端直接 400）。
const OPENCODE_SESSION_HEADER: &str = "x-opencode-session";

/// 按 API Key 派生稳定的回退会话 ID（UUIDv5）：网关无会话概念，取
/// 「每个 API Key 一个稳定会话」作为会话亲和的近似——重启不漂移，换 Key 即换会话。
/// 客户端自带的 `x-opencode-session` 透传值优先于此回退。
fn opencode_session_fallback(api_key_name: &str) -> String {
    Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("llm-gateway/opencode-session/{api_key_name}").as_bytes(),
    )
    .to_string()
}

/// 第 4 层透传默认 allowlist：W3C Trace Context 头（traceparent 原样透传；
/// tracestate 仅在透传 traceparent 时透传）、`x-opencode-session`（客户端
/// 自带的 OpenCode 会话标识按原值透传）与 `user-agent`（下游客户端标识
/// 原样透传所有上游）。其余下游头一律不透传。
pub fn forward_allowlist() -> &'static [HeaderName] {
    use std::sync::OnceLock;
    static ALLOWLIST: OnceLock<Vec<HeaderName>> = OnceLock::new();
    ALLOWLIST.get_or_init(|| {
        vec![
            HeaderName::from_static("traceparent"),
            HeaderName::from_static("tracestate"),
            HeaderName::from_static(OPENCODE_SESSION_HEADER),
            HeaderName::from_static("user-agent"),
        ]
    })
}

/// 判定 header 名是否落在剥离/禁止清单。
pub fn is_never_outbound(name: &HeaderName) -> bool {
    NEVER_OUTBOUND
        .iter()
        .any(|reserved| name.as_str().eq_ignore_ascii_case(reserved))
}

/// 从下游请求头中选择可透传的子集（allowlist 命中项）。
/// - allowlist 命中项 first-wins、单值（HTTP 语义上重复等同逗号列表的项我们不透传）。
/// - 剥离清单命中项即使 allowlist 里写了也不透传（黑名单优先）。
///
/// 供 `/v1` 入口（handler 拿到下游 `HeaderMap`）与本模块单测使用。
pub fn select_forwardable_headers(
    downstream: &HeaderMap,
    allowlist: &[HeaderName],
) -> Vec<(HeaderName, HeaderValue)> {
    let mut out: Vec<(HeaderName, HeaderValue)> = Vec::new();
    for name in allowlist {
        if is_never_outbound(name) {
            continue;
        }
        if let Some(value) = downstream.get(name) {
            out.push((name.clone(), value.clone()));
        }
    }
    out
}

/// 把 `custom_header`（JSON 对象，字符串值）合并进出站表。
/// - 命中协议鉴权/必需头名（`protocol_auth_header_names`，D3）→ 跳过并 `warn!`
///   （管理员误配同名协议头会静默失效，需告警；只记 request_id/协议/头名，不记值）。
/// - 命中其余剥离清单名（框架头、凭据名等）→ 跳过。
/// - 其余项仅补缺：同名已存在（下游透传层）则跳过，下游值优先。
///
/// JSON 非法 / 非对象 / 非字符串值：静默跳过（保持原语义）。
fn merge_custom_headers(
    custom_header: &str,
    protocol: Protocol,
    request_id: &str,
    headers: &mut Vec<(HeaderName, HeaderValue)>,
) {
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
            if protocol_auth_header_names(protocol)
                .iter()
                .any(|reserved| name.as_str().eq_ignore_ascii_case(reserved))
            {
                tracing::warn!(
                    request_id,
                    protocol = ?protocol,
                    header = %name.as_str(),
                    "custom_header 试图覆盖协议鉴权/必需头，已忽略（D3）",
                );
                continue;
            }
            if is_never_outbound(&name) {
                continue;
            }
            // 下游 allowlist 透传同名值优先：custom_header 只补缺，不覆盖。
            if headers.iter().any(|(existing, _)| *existing == name) {
                continue;
            }
            headers.push((name, value));
        }
    }
}

/// 模板默认头查漏补缺：按 base_url host 取 `provider_template` 的默认
/// custom_header（opencode.ai → pi 同款 UA、api.kimi.com → KimiCLI），仅补
/// 出站表中尚不存在的名字——下游透传与 custom_header 同名值优先。
fn merge_template_default_headers(host: &str, headers: &mut Vec<(HeaderName, HeaderValue)>) {
    for (name, value) in provider_template::template_default_headers(host) {
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(&value),
        ) && !headers.iter().any(|(existing, _)| *existing == name)
        {
            headers.push((name, value));
        }
    }
}

/// 各协议在第 2 层写入的鉴权/必需头名（spec D3：custom_header 不得覆盖这些头，
/// 冲突时跳过并记 warn）。供 `apply_protocol_auth_headers` 与 `merge_custom_headers`
/// 共用，避免两处各自维护清单。
fn protocol_auth_header_names(protocol: Protocol) -> &'static [&'static str] {
    match protocol {
        Protocol::OpenAiCompat | Protocol::OpenAiResponses => &["authorization"],
        Protocol::Anthropic => &["x-api-key", "anthropic-version"],
        Protocol::Gemini => &["x-goog-api-key"],
    }
}

/// 组装第 2 层协议鉴权/必需头并 `insert` 覆盖低层同名（第 3 层 custom_header
/// 与第 4 层透传都不允许覆盖协议头，D3）。
fn apply_protocol_auth_headers(
    protocol: Protocol,
    api_key: &str,
    headers: &mut Vec<(HeaderName, HeaderValue)>,
) {
    for name in protocol_auth_header_names(protocol) {
        let value = match *name {
            "authorization" => format!("Bearer {api_key}"),
            "x-api-key" | "x-goog-api-key" => api_key.to_string(),
            "anthropic-version" => "2023-06-01".to_string(),
            _ => continue, // protocol_auth_header_names 新增保留名时若缺值映射则跳过
        };
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(&value),
        ) {
            headers.retain(|(existing, _)| existing != name);
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

    fn completion_model(&self) -> String {
        match self {
            Converter::Responses(c) => c.completion_model().to_string(),
            Converter::Anthropic(_) | Converter::Gemini(_) => {
                unreachable!("only Responses uses upstream completion metadata")
            }
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
        "usage": cached_client_usage_json(usage),
    })
}

/// 转发入口：处理 POST /v1/chat/completions。
///
/// `forwarded` 是入口从下游请求头中按 allowlist 选出、并已剥离保留名的
/// 透传子集（可空）；同一请求的多次 failover 出站共用同一快照，保证一致。
pub async fn forward_chat(
    state: &AppState,
    api_key: AuthedApiKey,
    client_body: Value,
    forwarded: Vec<(HeaderName, HeaderValue)>,
) -> Response {
    let request_id = Uuid::new_v4().to_string();
    // OpenCode Go 会话亲和：客户端自带值经 allowlist 透传，缺失时用回退值。
    let opencode_session = opencode_session_fallback(&api_key.name);
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
    // 本次请求已记连续失败的 provider：同一请求内同供应商多个成员失败只计一次。
    let mut counted_failures: HashSet<i32> = HashSet::new();
    for (index, member) in ordered.iter().enumerate() {
        let has_more = index + 1 < ordered.len();
        let start_time = now_ms();
        // 降级失败统一落库：与最终失败同字段，request_id 带尝试序号后缀区分。
        let record_degraded = |message: &str, ttft_start_ms: i64| {
            record_failure(
                &state.db,
                &format!("{request_id}-{}", index + 1),
                virtual_model.virtual_model_id,
                member,
                &api_key.name,
                start_time,
                false,
                client_stream,
                message,
                ttft_start_ms,
            );
        };
        let decrypted_key = match crypto::decrypt(&member.api_key_encrypted) {
            Ok(key) => key,
            Err(e) => {
                let message = format!("解密供应商密钥失败：{e}");
                note_member_failure(state, member, &request_id, &mut counted_failures).await;
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
                    record_degraded(&message, start_time);
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

        let (call, json_mode_tool) = match build_upstream_call(
            member,
            &client_body,
            client_stream,
            &decrypted_key,
            &forwarded,
            &request_id,
            &opencode_session,
        ) {
            Ok(result) => result,
            Err(message) => {
                note_member_failure(state, member, &request_id, &mut counted_failures).await;
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
                    record_degraded(&message, start_time);
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

        // Member 已由 resolve_proxy 归一：proxy_enabled 时地址必非空。
        let proxy = member.proxy_enabled.then_some(member.proxy_addr.as_str());
        let reply = match upstream::call(call, &state.upstream_pool, proxy).await {
            Ok(reply) => reply,
            Err(e) => {
                let message = e.fail_reason();
                note_member_failure(state, member, &request_id, &mut counted_failures).await;
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
                    record_degraded(&message, start_time);
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
            note_member_failure(state, member, &request_id, &mut counted_failures).await;
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
                record_degraded(&message, reply.start_at_ms);
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

    // 成功即清零该供应商的连续失败计数（偶发失败不累积）。
    state.failure_counter.reset(member.provider_id);

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
            let completion_id = converter.completion_id();
            let completion_model = converter.completion_model();
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
                                &completion_id,
                                &completion_model,
                                cached_client_usage_json(&usage_for_chunk),
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
                        let usage_json = if protocol == Protocol::Gemini {
                            cached_client_usage_json(&usage)
                        } else {
                            client_usage_json(&usage)
                        };
                        let usage_chunk = usage_chunk_json(
                            &converter.completion_id(),
                            &requested_model,
                            usage_json,
                        );
                        let frame = usage_chunk.to_string();
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
/// 成功返回 `duration_ms`：本次请求耗时（上游响应开始到读完，与 request 表
/// `output_tokens_time` 同口径，排除 TTFT）。
pub async fn test_model(
    state: &AppState,
    provider_row: &crate::entity::provider::Model,
    model: &crate::entity::provider_model::Model,
    api_key: &str,
) -> Result<i64, String> {
    let (proxy_enabled, proxy_addr) = resolve_proxy(model, provider_row);
    // 协议优先级与转发一致：模型单独指定（非空）→ 供应商协议。
    let protocol_value = model.protocol_type.unwrap_or(provider_row.protocol_type);
    let member = Member {
        provider_id: provider_row.id,
        model_id: model.provider_model_id.clone(),
        protocol: Protocol::from_i32(protocol_value),
        billing_mode: provider_row.billing_mode,
        base_url: provider_row.base_url.clone(),
        api_key_encrypted: provider_row.api_key.clone(),
        custom_header: provider_row.custom_header.clone(),
        proxy_enabled,
        proxy_addr,
    };

    let chat = json!({
        "model": model.provider_model_id,
        "stream": false,
        "max_tokens": model.max_output_tokens,
        "messages": [{"role": "user", "content": TEST_PROMPT}],
    });
    // test_model 无下游请求头（管理面手动触发）：透传子集为空。
    let request_id = format!("test-{}", Uuid::new_v4());
    let (call, _json_mode_tool) = build_upstream_call(
        &member,
        &chat,
        false,
        api_key,
        &[],
        &request_id,
        &opencode_session_fallback(TEST_API_KEY_NAME),
    )?;

    // Member 已由 resolve_proxy 归一：proxy_enabled 时地址必非空。
    let proxy = member.proxy_enabled.then_some(member.proxy_addr.as_str());
    let start_time = now_ms();
    let reply = match upstream::call(call, &state.upstream_pool, proxy).await {
        Ok(reply) => reply,
        Err(e) => {
            let message = e.fail_reason();
            record_failure(
                &state.db,
                &request_id,
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

    if !reply.status.is_success() {
        let body = upstream::read_body(reply.body).await.unwrap_or_default();
        let message = format!(
            "{} {}",
            reply.status.as_u16(),
            extract_error_message(&String::from_utf8_lossy(&body))
        );
        record_failure(
            &state.db,
            &request_id,
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
    let duration_ms = (end_time - reply.start_at_ms).max(0);
    RequestRecord {
        request_id: format!("test-{}", Uuid::new_v4()),
        virtual_model_id: TEST_VIRTUAL_MODEL_ID,
        provider_id: member.provider_id,
        model_id: member.model_id.clone(),
        stream: false,
        ttft: None,
        output_tokens_time: Some(duration_ms),
        ttft_start_ms: reply.start_at_ms,
        start_time,
        end_time,
        usage,
        success: true,
        fail_reason: None,
        api_key_name: TEST_API_KEY_NAME.to_string(),
    }
    .insert(&state.db);

    Ok(duration_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(proxy_enabled: bool, proxy_addr: &str) -> provider::Model {
        let now = chrono::Utc::now();
        provider::Model {
            id: 1,
            name: "p".to_string(),
            enable: true,
            base_url: "https://api.example.com".to_string(),
            api_key: "enc".to_string(),
            custom_header: "{}".to_string(),
            protocol_type: 0,
            billing_mode: 0,
            extra: "{}".to_string(),
            sort_order: 0,
            proxy_enabled,
            proxy_addr: proxy_addr.to_string(),
            failure_disabled: false,
            created_at: now,
            updated_at: now,
        }
    }

    fn model(proxy_enabled: bool, proxy_addr: &str) -> provider_model::Model {
        let now = chrono::Utc::now();
        provider_model::Model {
            model_id: 1,
            provider_id: 1,
            provider_model_id: "m".to_string(),
            context_length: 1000,
            max_output_tokens: 1000,
            reasoning: false,
            tool_use: false,
            image_understand: false,
            video_understand: false,
            protocol_type: None,
            proxy_enabled,
            proxy_addr: proxy_addr.to_string(),
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn resolve_proxy_prefers_model_then_provider_then_direct() {
        // 模型开启 → 用模型地址（即使供应商也开着、地址不同）。
        assert_eq!(
            resolve_proxy(
                &model(true, "http://model:1"),
                &provider(true, "http://p:2")
            ),
            (true, "http://model:1".to_string())
        );
        // 模型关、供应商开 → 回落供应商地址。
        assert_eq!(
            resolve_proxy(&model(false, ""), &provider(true, "http://p:2")),
            (true, "http://p:2".to_string())
        );
        // 模型开但地址空白 → 视为未配置，回落供应商。
        assert_eq!(
            resolve_proxy(&model(true, "  "), &provider(true, "http://p:2")),
            (true, "http://p:2".to_string())
        );
        // 两者都关 → 直连。
        assert_eq!(
            resolve_proxy(&model(false, ""), &provider(false, "")),
            (false, String::new())
        );
    }

    // ─── 上游出站头组装（四层覆盖 + 剥离）单测 ───

    fn member(protocol: Protocol, custom_header: &str) -> Member {
        member_with_base_url(protocol, custom_header, "https://api.example.com/v1")
    }

    fn member_with_base_url(protocol: Protocol, custom_header: &str, base_url: &str) -> Member {
        Member {
            provider_id: 1,
            model_id: "m".to_string(),
            protocol,
            billing_mode: 0,
            base_url: base_url.to_string(),
            api_key_encrypted: "enc".to_string(),
            custom_header: custom_header.to_string(),
            proxy_enabled: false,
            proxy_addr: String::new(),
        }
    }

    fn hv(value: &str) -> HeaderValue {
        HeaderValue::from_str(value).unwrap()
    }

    fn names(call: &UpstreamCall) -> Vec<String> {
        let mut seen: Vec<String> = call
            .headers
            .iter()
            .map(|(n, _)| n.as_str().to_ascii_lowercase())
            .collect();
        seen.sort();
        seen
    }

    fn header_map(entries: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (k, v) in entries {
            map.insert(HeaderName::from_bytes(k.as_bytes()).unwrap(), hv(v));
        }
        map
    }

    #[test]
    fn is_never_outbound_covers_credentials_framing_and_hop_by_hop() {
        for reserved in [
            "authorization",
            "cookie",
            "proxy-authorization",
            "x-api-key",
            "x-goog-api-key",
            "connection",
            "keep-alive",
            "proxy-connection",
            "transfer-encoding",
            "te",
            "host",
            "content-length",
            "content-type",
            "accept",
            "expect",
            "x-forwarded-for",
            "forwarded",
            "via",
            "x-real-ip",
        ] {
            let name = HeaderName::from_bytes(reserved.as_bytes()).unwrap();
            assert!(is_never_outbound(&name), "{reserved} 应被剥离");
        }
        for allowed in ["traceparent", "tracestate", "x-trace-id", "anthropic-beta"] {
            let name = HeaderName::from_bytes(allowed.as_bytes()).unwrap();
            assert!(!is_never_outbound(&name), "{allowed} 不应被剥离");
        }
    }

    #[test]
    fn select_forwardable_passes_allowlist_and_blocks_blacklist() {
        let map = header_map(&[
            (
                "traceparent",
                "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
            ),
            ("tracestate", "vendor=abc"),
            ("x-trace-id", "client-1"),
            ("authorization", "Bearer lg-secret"),
            ("host", "evil.example"),
        ]);
        let out = select_forwardable_headers(&map, forward_allowlist());
        let got: Vec<(String, String)> = out
            .iter()
            .map(|(n, v)| (n.as_str().to_string(), v.to_str().unwrap().to_string()))
            .collect();
        // trace 头透传；x-trace-id 不在 allowlist 不透传；凭据/框架头即使被
        // allowlist 点名也不透传（黑名单优先）。
        assert_eq!(
            got,
            vec![
                (
                    "traceparent".to_string(),
                    "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".to_string()
                ),
                ("tracestate".to_string(), "vendor=abc".to_string()),
            ]
        );

        // allowlist 里点名黑名单项也不会放行。
        let allow_forced = [
            HeaderName::from_static("authorization"),
            HeaderName::from_static("traceparent"),
        ];
        let out2 = select_forwardable_headers(&map, &allow_forced);
        assert_eq!(out2.len(), 1);
        assert_eq!(out2[0].0.as_str(), "traceparent");
    }

    #[test]
    fn custom_header_cannot_override_protocol_auth_or_framing() {
        // Anthropic custom_header 携带 x-api-key / anthropic-version 同名、以及
        // authorization/content-type 保留名：全部被跳过，协议头保留网关值。
        let m = member(
            Protocol::Anthropic,
            r#"{"x-api-key":"custom","anthropic-version":"2099-01-01","authorization":"Bearer custom","content-type":"text/plain","X-A":"b"}"#,
        );
        let chat = json!({"model":"m","messages":[{"role":"user","content":"hi"}]});
        let (call, _) =
            build_upstream_call(&m, &chat, false, "sk-provider", &[], "test", "fb-sess").unwrap();
        let map: std::collections::HashMap<&str, &str> = call
            .headers
            .iter()
            .map(|(n, v)| (n.as_str(), v.to_str().unwrap()))
            .collect();
        assert_eq!(map.get("x-api-key").copied(), Some("sk-provider"));
        assert_eq!(map.get("anthropic-version").copied(), Some("2023-06-01"));
        assert!(
            !map.contains_key("authorization"),
            "authorization 不进 Anthropic 上游"
        );
        assert!(
            !map.contains_key("content-type"),
            "content-type 由发送端框架头生成"
        );
        assert_eq!(map.get("x-a").copied(), Some("b"), "普通自定义头应生效");
        // 无同名重复。
        assert!(!has_duplicate_names(&call));
    }

    #[test]
    fn openai_compat_auth_header_uses_provider_key_and_drops_custom() {
        let m = member(
            Protocol::OpenAiCompat,
            r#"{"authorization":"Bearer stale","X-Tenant":"t1"}"#,
        );
        let chat = json!({"model":"m","messages":[{"role":"user","content":"hi"}]});
        let (call, _) =
            build_upstream_call(&m, &chat, false, "sk-provider", &[], "test", "fb-sess").unwrap();
        let map: std::collections::HashMap<&str, &str> = call
            .headers
            .iter()
            .map(|(n, v)| (n.as_str(), v.to_str().unwrap()))
            .collect();
        assert_eq!(
            map.get("authorization").copied(),
            Some("Bearer sk-provider")
        );
        assert_eq!(map.get("x-tenant").copied(), Some("t1"));
        assert!(!has_duplicate_names(&call));
    }

    #[test]
    fn forwarded_headers_beat_custom_header_on_same_name() {
        let m = member(
            Protocol::OpenAiCompat,
            r#"{"traceparent":"custom-tp","X-A":"b"}"#,
        );
        let chat = json!({"model":"m","messages":[{"role":"user","content":"hi"}]});
        let forwarded = vec![
            (
                HeaderName::from_static("traceparent"),
                hv("00-downstream-tp"),
            ),
            (HeaderName::from_static("x-trace-id"), hv("client-1")),
        ];
        let (call, _) = build_upstream_call(
            &m,
            &chat,
            false,
            "sk-provider",
            &forwarded,
            "test",
            "fb-sess",
        )
        .unwrap();
        let map: std::collections::HashMap<&str, &str> = call
            .headers
            .iter()
            .map(|(n, v)| (n.as_str(), v.to_str().unwrap()))
            .collect();
        // 同名时下游透传值优先，custom_header 只补缺（新合并语义）。
        assert_eq!(map.get("traceparent").copied(), Some("00-downstream-tp"));
        assert_eq!(map.get("x-trace-id").copied(), Some("client-1"));
        assert_eq!(map.get("x-a").copied(), Some("b"));
        assert!(!has_duplicate_names(&call));
    }

    #[test]
    fn gemini_auth_header_is_x_goog_api_key() {
        let m = member(Protocol::Gemini, r#"{"x-goog-api-key":"custom","X-A":"b"}"#);
        let chat = json!({"model":"m","contents":[{"role":"user","parts":[{"text":"hi"}]}]});
        let (call, _) =
            build_upstream_call(&m, &chat, false, "sk-provider", &[], "test", "fb-sess").unwrap();
        let map: std::collections::HashMap<&str, &str> = call
            .headers
            .iter()
            .map(|(n, v)| (n.as_str(), v.to_str().unwrap()))
            .collect();
        assert_eq!(map.get("x-goog-api-key").copied(), Some("sk-provider"));
        assert_eq!(map.get("x-a").copied(), Some("b"));
        assert!(!has_duplicate_names(&call));
    }

    #[test]
    fn invalid_custom_header_is_ignored() {
        for raw in ["not-json", "[]", r#"{"x":123}"#, r#"{"x":"v","y":1}"#] {
            let m = member(Protocol::OpenAiCompat, raw);
            let chat = json!({"model":"m","messages":[{"role":"user","content":"hi"}]});
            let (call, _) =
                build_upstream_call(&m, &chat, false, "sk-provider", &[], "test", "fb-sess")
                    .unwrap();
            // 非对象 JSON / 非字符串值整体跳过：只剩协议鉴权头。
            let map: std::collections::HashMap<&str, &str> = call
                .headers
                .iter()
                .map(|(n, v)| (n.as_str(), v.to_str().unwrap()))
                .collect();
            assert_eq!(
                map.get("authorization").copied(),
                Some("Bearer sk-provider")
            );
        }
    }

    #[test]
    fn opencode_session_fallback_is_stable_uuid_per_key() {
        let a = opencode_session_fallback("itest-key");
        assert_eq!(
            a,
            opencode_session_fallback("itest-key"),
            "同 Key 派生应稳定"
        );
        assert_ne!(a, opencode_session_fallback("other-key"), "换 Key 应换会话");
        assert!(Uuid::parse_str(&a).is_ok());
    }

    #[test]
    fn opencode_member_injects_session_fallback() {
        let m = member_with_base_url(
            Protocol::OpenAiCompat,
            "{}",
            "https://opencode.ai/zen/go/v1",
        );
        let chat = json!({"model":"m","messages":[{"role":"user","content":"hi"}]});
        let (call, _) =
            build_upstream_call(&m, &chat, false, "sk-provider", &[], "test", "sess-a").unwrap();
        let map: std::collections::HashMap<&str, &str> = call
            .headers
            .iter()
            .map(|(n, v)| (n.as_str(), v.to_str().unwrap()))
            .collect();
        assert_eq!(map.get("x-opencode-session").copied(), Some("sess-a"));
        assert!(!has_duplicate_names(&call));
    }

    #[test]
    fn opencode_session_client_and_custom_values_win_over_fallback() {
        let chat = json!({"model":"m","messages":[{"role":"user","content":"hi"}]});
        let forwarded = vec![(
            HeaderName::from_static("x-opencode-session"),
            hv("from-client"),
        )];
        // 客户端自带值优先于回退（回退仅在四层组装后仍无该头时注入）。
        let m = member_with_base_url(
            Protocol::OpenAiCompat,
            "{}",
            "https://opencode.ai/zen/go/v1",
        );
        let (call, _) =
            build_upstream_call(&m, &chat, false, "sk-provider", &forwarded, "test", "fb").unwrap();
        let map: std::collections::HashMap<&str, &str> = call
            .headers
            .iter()
            .map(|(n, v)| (n.as_str(), v.to_str().unwrap()))
            .collect();
        assert_eq!(map.get("x-opencode-session").copied(), Some("from-client"));

        // custom_header（第 3 层）不覆盖客户端透传（同名下游值优先），但仍优先于回退。
        let m2 = member_with_base_url(
            Protocol::OpenAiCompat,
            r#"{"x-opencode-session":"from-custom"}"#,
            "https://opencode.ai/zen/go/v1",
        );
        let (call2, _) =
            build_upstream_call(&m2, &chat, false, "sk-provider", &forwarded, "test", "fb")
                .unwrap();
        let map2: std::collections::HashMap<&str, &str> = call2
            .headers
            .iter()
            .map(|(n, v)| (n.as_str(), v.to_str().unwrap()))
            .collect();
        assert_eq!(map2.get("x-opencode-session").copied(), Some("from-client"));

        // 无客户端值时 custom_header 仍优先于回退。
        let (call3, _) =
            build_upstream_call(&m2, &chat, false, "sk-provider", &[], "test", "fb").unwrap();
        let map3: std::collections::HashMap<&str, &str> = call3
            .headers
            .iter()
            .map(|(n, v)| (n.as_str(), v.to_str().unwrap()))
            .collect();
        assert_eq!(map3.get("x-opencode-session").copied(), Some("from-custom"));
    }

    #[test]
    fn non_opencode_member_does_not_inject_session_fallback() {
        let m = member(Protocol::OpenAiCompat, "{}");
        let chat = json!({"model":"m","messages":[{"role":"user","content":"hi"}]});
        let (call, _) =
            build_upstream_call(&m, &chat, false, "sk-provider", &[], "test", "fb-sess").unwrap();
        let joined = names(&call).join(",");
        assert!(
            !joined.contains("x-opencode-session"),
            "非 opencode 上游不应注入回退会话头：{joined}"
        );
    }

    #[test]
    fn template_default_user_agent_fills_opencode_and_kimi_hosts() {
        let chat = json!({"model":"m","messages":[{"role":"user","content":"hi"}]});
        let user_agent = |base_url: &str| {
            let m = member_with_base_url(Protocol::OpenAiCompat, "{}", base_url);
            let (call, _) =
                build_upstream_call(&m, &chat, false, "sk-provider", &[], "test", "fb").unwrap();
            let map: std::collections::HashMap<&str, &str> = call
                .headers
                .iter()
                .map(|(n, v)| (n.as_str(), v.to_str().unwrap()))
                .collect();
            map.get("user-agent").copied().unwrap_or("").to_string()
        };
        // OpenCode：pi 同款动态 UA（内核版本随宿主机变化，只断言形状）。
        let opencode_ua = user_agent("https://opencode.ai/zen/go/v1");
        assert!(opencode_ua.starts_with("pi ("), "{opencode_ua}");
        assert!(opencode_ua.ends_with(')'), "{opencode_ua}");
        // Kimi For Coding：官方 kimi-cli 当前版本 UA。
        assert_eq!(
            user_agent("https://api.kimi.com/coding/v1"),
            provider_template::KIMI_CODE_USER_AGENT
        );
        // 非模板 host 不注入默认 UA。
        assert_eq!(user_agent("https://api.example.com/v1"), "");
    }

    #[test]
    fn custom_and_forwarded_user_agent_beat_template_default() {
        let chat = json!({"model":"m","messages":[{"role":"user","content":"hi"}]});
        let m = member_with_base_url(
            Protocol::OpenAiCompat,
            r#"{"User-Agent":"custom/9"}"#,
            "https://opencode.ai/zen/go/v1",
        );
        let (call, _) =
            build_upstream_call(&m, &chat, false, "sk-provider", &[], "test", "fb").unwrap();
        let map: std::collections::HashMap<&str, &str> = call
            .headers
            .iter()
            .map(|(n, v)| (n.as_str(), v.to_str().unwrap()))
            .collect();
        // custom_header 优先于模板默认头。
        assert_eq!(map.get("user-agent").copied(), Some("custom/9"));

        // 下游透传又优先于 custom_header。
        let forwarded = vec![(HeaderName::from_static("user-agent"), hv("from-client"))];
        let (call2, _) =
            build_upstream_call(&m, &chat, false, "sk-provider", &forwarded, "test", "fb").unwrap();
        let map2: std::collections::HashMap<&str, &str> = call2
            .headers
            .iter()
            .map(|(n, v)| (n.as_str(), v.to_str().unwrap()))
            .collect();
        assert_eq!(map2.get("user-agent").copied(), Some("from-client"));
        assert!(!has_duplicate_names(&call));
        assert!(!has_duplicate_names(&call2));
    }

    #[test]
    fn forward_allowlist_includes_opencode_session() {
        let map = header_map(&[("x-opencode-session", "client-sess"), ("x-other", "v")]);
        let out = select_forwardable_headers(&map, forward_allowlist());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0.as_str(), "x-opencode-session");
        assert_eq!(out[0].1, hv("client-sess"));
    }

    #[test]
    fn forward_allowlist_forwards_downstream_user_agent() {
        let map = header_map(&[("user-agent", "zcode/1.2.3"), ("x-other", "v")]);
        let out = select_forwardable_headers(&map, forward_allowlist());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0.as_str(), "user-agent");
        assert_eq!(out[0].1, hv("zcode/1.2.3"));
    }

    /// 断言出站头无同名重复。
    fn has_duplicate_names(call: &UpstreamCall) -> bool {
        let mut seen = std::collections::HashSet::new();
        call.headers
            .iter()
            .any(|(n, _)| !seen.insert(n.as_str().to_ascii_lowercase()))
    }

    #[test]
    fn never_outbound_headers_never_reach_upstream_call_headers() {
        // 通过 merge_custom_headers 直接验证剥离清单在 custom_header 层生效。
        let m = member(
            Protocol::OpenAiCompat,
            r#"{"connection":"keep-alive","host":"evil","x-custom":"ok"}"#,
        );
        let chat = json!({"model":"m","messages":[{"role":"user","content":"hi"}]});
        let (call, _) =
            build_upstream_call(&m, &chat, false, "sk-provider", &[], "test", "fb-sess").unwrap();
        let joined = names(&call).join(",");
        assert!(
            !joined.contains("connection"),
            "connection 应被剥离：{joined}"
        );
        assert!(!joined.contains("host"), "host 应被剥离：{joined}");
        assert!(joined.contains("x-custom"), "x-custom 应保留：{joined}");
        assert!(!has_duplicate_names(&call));
    }
}

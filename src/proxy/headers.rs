use super::*;

/// 降级标记响应头：值固定为 history（工具轮历史缺签名块）。
pub(crate) const THINKING_DROPPED_HEADER: &str = "x-llm-gateway-thinking-dropped";

/// 按需给响应附加降级标记头。
pub(crate) fn with_thinking_dropped_header(mut response: Response, dropped: bool) -> Response {
    if dropped {
        response.headers_mut().insert(
            HeaderName::from_static(THINKING_DROPPED_HEADER),
            HeaderValue::from_static("history"),
        );
    }
    response
}
// ─── 上游出站头组装：四层覆盖模型（见 .scratch/upstream-header-forwarding/） ───

/// 剥离清单：命中即不进上游（下游头与 custom_header 都受此约束）。
/// 分为两类：凭据名与协议保留名（绝不出站/不得作为自定义覆盖名），
/// 以及框架头保留名（`Host`/`Content-Length`/`Content-Type`/`accept` 由网关生成）。
/// 框架名也列入禁止名，避免组装层写入后与发送端第 1 层重复。
pub(crate) const NEVER_OUTBOUND: &[&str] = &[
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
pub(crate) const OPENCODE_SESSION_HEADER: &str = "x-opencode-session";

/// 按 API Key 派生稳定的回退会话 ID（UUIDv5）：网关无会话概念，取
/// 「每个 API Key 一个稳定会话」作为会话亲和的近似——重启不漂移，换 Key 即换会话。
/// 客户端自带的 `x-opencode-session` 透传值优先于此回退。
pub(crate) fn opencode_session_fallback(api_key_name: &str) -> String {
    Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("llm-gateway/opencode-session/{api_key_name}").as_bytes(),
    )
    .to_string()
}

/// 判定 header 名是否落在剥离/禁止清单。
pub fn is_never_outbound(name: &HeaderName) -> bool {
    NEVER_OUTBOUND
        .iter()
        .any(|reserved| name.as_str().eq_ignore_ascii_case(reserved))
}

/// 原生透传端点（/v1/messages・/v1/responses）的下游头选择：
/// 黑名单兜底的全量透传——仅剥离剥离清单内的凭据/hop-by-hop/框架头
/// （鉴权头由网关重新生成）；anthropic-beta、OpenAI-Beta 等 feature 头
/// 自然透传。allowlist 机制不适用（原生客户端特性头默认不在白名单）。
pub fn select_passthrough_headers(downstream: &HeaderMap) -> Vec<(HeaderName, HeaderValue)> {
    downstream
        .iter()
        .filter(|(name, _)| !is_never_outbound(name))
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect()
}

/// 从下游请求头中选择可透传的子集（allowlist 命中项）。
/// - allowlist 命中项 first-wins、单值（HTTP 语义上重复等同逗号列表的项我们不透传）。
/// - 剥离清单命中项即使 allowlist 里写了也不透传（黑名单优先）。
///
/// allowlist 来自设置项 `downstream_request_header_allow_list` 的进程内缓存
/// （`AppSettings::downstream_header_allow_list`），设置页更新后热生效。
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
pub(crate) fn merge_custom_headers(
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
pub(crate) fn merge_template_default_headers(
    host: &str,
    headers: &mut Vec<(HeaderName, HeaderValue)>,
) {
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
pub(crate) fn protocol_auth_header_names(protocol: Protocol) -> &'static [&'static str] {
    match protocol {
        Protocol::OpenAiCompat | Protocol::OpenAiResponses => &["authorization"],
        Protocol::Anthropic => &["x-api-key", "anthropic-version"],
        Protocol::Gemini => &["x-goog-api-key"],
    }
}

/// 组装第 2 层协议鉴权/必需头并 `insert` 覆盖低层同名（第 3 层 custom_header
/// 与第 4 层透传都不允许覆盖协议头，D3）。
pub(crate) fn apply_protocol_auth_headers(
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

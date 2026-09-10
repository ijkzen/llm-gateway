use super::*;

/// 出站头四层组装（两种调用构建共用）：下游透传子集 → provider
/// custom_header（同名不覆盖透传层，协议保留名跳过并告警）→ 模板默认头
/// （按 host 查漏补缺）→ 协议鉴权/必需头（覆盖以上所有层，D3）→ OpenCode
/// Go 会话亲和头（透传/custom_header 已带则不覆盖）。
fn assemble_outbound_headers(
    member: &Member,
    forwarded: &[(HeaderName, HeaderValue)],
    upstream_host: &str,
    request_id: &str,
    opencode_session: &str,
    api_key: &str,
) -> Vec<(HeaderName, HeaderValue)> {
    let mut headers: Vec<(HeaderName, HeaderValue)> = forwarded.to_vec();
    merge_custom_headers(
        &member.custom_header,
        member.protocol,
        request_id,
        &mut headers,
    );
    merge_template_default_headers(upstream_host, &mut headers);
    apply_protocol_auth_headers(member.protocol, api_key, &mut headers);
    if provider_template::is_opencode_host(upstream_host)
        && !headers
            .iter()
            .any(|(n, _)| n.as_str().eq_ignore_ascii_case(OPENCODE_SESSION_HEADER))
        && let Ok(value) = HeaderValue::from_str(opencode_session)
    {
        headers.push((HeaderName::from_static(OPENCODE_SESSION_HEADER), value));
    }
    headers
}

pub(crate) async fn build_upstream_call(
    member: &Member,
    chat: &Value,
    client_stream: bool,
    api_key: &str,
    forwarded: &[(HeaderName, HeaderValue)],
    request_id: &str,
    opencode_session: &str,
) -> Result<(UpstreamCall, convert::RequestFlags), String> {
    let (body, flags, sub_path) = match member.protocol {
        Protocol::OpenAiCompat => {
            let body = openai::build_request_body(chat, &member.model_id);
            (
                body,
                crate::proxy::convert::RequestFlags::default(),
                "chat/completions".to_string(),
            )
        }
        Protocol::OpenAiResponses => {
            let body = responses::build_request_body(chat, &member.model_id)?;
            (
                body,
                crate::proxy::convert::RequestFlags::default(),
                "responses".to_string(),
            )
        }
        Protocol::Anthropic => {
            let (body, anthropic_flags) = anthropic::build_request_body(chat, &member.model_id)?;
            (body, anthropic_flags, "messages".to_string())
        }
        Protocol::Gemini => {
            let mut body = gemini::build_request_body(chat, &member.model_id)?;
            // 远程 http(s) 图片下载转 inlineData（fileData 仅接受 GCS/Files API）。
            let proxy_addr = member.proxy_enabled.then_some(member.proxy_addr.as_str());
            gemini::inline_remote_images(&mut body, proxy_addr, request_id).await;
            let action = gemini::generate_action(client_stream);
            let model_path = if member.model_id.starts_with("models/") {
                member.model_id.clone()
            } else {
                format!("models/{}", member.model_id)
            };
            (
                body,
                crate::proxy::convert::RequestFlags::default(),
                format!("{model_path}:{action}"),
            )
        }
    };

    let url = build_upstream_url(&member.base_url, member.protocol_code(), &sub_path);
    let upstream_host = provider_template::host_of(&member.base_url).unwrap_or_default();
    let body_bytes = Bytes::from(body.to_string());
    let headers = assemble_outbound_headers(
        member,
        forwarded,
        &upstream_host,
        request_id,
        opencode_session,
        api_key,
    );

    Ok((
        UpstreamCall {
            url,
            headers,
            body: body_bytes,
        },
        flags,
    ))
}

/// 构造原生透传上游调用：仅改写 model，其余字段与下游头原样出站
/// （剥离清单兜底 + 协议鉴权头由网关注入）。
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_native_upstream_call(
    endpoint: NativeEndpoint,
    member: &Member,
    client_body: &Value,
    api_key: &str,
    forwarded: &[(HeaderName, HeaderValue)],
    request_id: &str,
    opencode_session: &str,
) -> Result<UpstreamCall, String> {
    let mut body = client_body.clone();
    body["model"] = Value::String(member.model_id.clone());
    let url = build_upstream_url(
        &member.base_url,
        member.protocol_code(),
        endpoint.sub_path(),
    );
    let upstream_host = provider_template::host_of(&member.base_url).unwrap_or_default();
    let headers = assemble_outbound_headers(
        member,
        forwarded,
        &upstream_host,
        request_id,
        opencode_session,
        api_key,
    );
    Ok(UpstreamCall {
        url,
        headers,
        body: Bytes::from(body.to_string()),
    })
}

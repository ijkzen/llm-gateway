use super::*;

// ─── /v1/messages・/v1/responses 原生协议透传 ───

/// 原生透传端点：请求/响应体不做协议转换，仅改写 model 并原样中继。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeEndpoint {
    /// Anthropic Messages（/v1/messages → 虚拟模型接口类型 2）。
    AnthropicMessages,
    /// OpenAI Responses（/v1/responses → 虚拟模型接口类型 1）。
    OpenAiResponses,
}

impl NativeEndpoint {
    fn interface_type(self) -> i32 {
        match self {
            NativeEndpoint::AnthropicMessages => virtual_model::INTERFACE_ANTHROPIC_MESSAGES,
            NativeEndpoint::OpenAiResponses => virtual_model::INTERFACE_OPENAI_RESPONSES,
        }
    }

    pub(crate) fn sub_path(self) -> &'static str {
        match self {
            NativeEndpoint::AnthropicMessages => "messages",
            NativeEndpoint::OpenAiResponses => "responses",
        }
    }

    pub(crate) fn member_protocol(self) -> Protocol {
        match self {
            NativeEndpoint::AnthropicMessages => Protocol::Anthropic,
            NativeEndpoint::OpenAiResponses => Protocol::OpenAiResponses,
        }
    }

    /// 原生错误响应（Anthropic type/error 结构 / OpenAI error 结构）。
    pub(crate) fn error(
        self,
        status: StatusCode,
        error_type: &str,
        message: impl Into<String>,
    ) -> Response {
        match self {
            NativeEndpoint::AnthropicMessages => {
                crate::auth::anthropic_error(status, error_type, message)
            }
            NativeEndpoint::OpenAiResponses => {
                // OpenAI 枚举没有 not_found_error，映射为 invalid_request_error。
                let openai_type = if error_type == "not_found_error" {
                    "invalid_request_error"
                } else {
                    error_type
                };
                openai_error(status, message, openai_type, "upstream_error")
            }
        }
    }
}

/// POST /v1/messages・/v1/responses：原生协议透传转发。
///
/// 与 `forward_chat` 共用虚拟模型路由、LB 排序与 failover 骨架；差异：
/// 请求体仅改写 model 字段（无协议转换）、下游头黑名单兜底全量透传、
/// usage 从原生响应解析（旁路扫描）、错误响应用端点对应协议的原生格式。
pub async fn forward_native(
    state: &AppState,
    api_key: AuthedApiKey,
    endpoint: NativeEndpoint,
    downstream_headers: &HeaderMap,
    body_bytes: Bytes,
) -> Response {
    let request_id = Uuid::new_v4().to_string();
    let opencode_session = opencode_session_fallback(&api_key.name);
    let body: Value = match serde_json::from_slice::<Value>(&body_bytes) {
        Ok(value) if value.is_object() => value,
        _ => {
            return endpoint.error(
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                "请求体不是合法的 JSON 对象",
            );
        }
    };
    let requested_model = body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if requested_model.is_empty() {
        return endpoint.error(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            "请求缺少 model 字段",
        );
    }
    let client_stream = body.get("stream").and_then(Value::as_bool).unwrap_or(false);
    let forwarded = select_passthrough_headers(downstream_headers);

    // 路由解析前半段（与 chat 共用 resolve_and_order）：display_id 路由 +
    // 接口类型严格对应 + 成员加载 + 防御性协议过滤（成员匹配规则保证，
    // 兜底跳过异协议成员）+ LB 排序；错误信封按端点协议格式映射。
    let route = match resolve_and_order(
        state,
        &request_id,
        &requested_model,
        |vm| vm.interface_type == endpoint.interface_type(),
        |member| member.protocol == endpoint.member_protocol(),
    )
    .await
    {
        Ok(route) => route,
        Err(RouteError::NotFound) => {
            return endpoint.error(
                StatusCode::NOT_FOUND,
                "not_found_error",
                format!("model '{requested_model}' does not exist"),
            );
        }
        Err(RouteError::QueryFailed(message)) => {
            return endpoint.error(StatusCode::INTERNAL_SERVER_ERROR, "api_error", message);
        }
        Err(RouteError::NoMembers) => {
            return endpoint.error(
                StatusCode::SERVICE_UNAVAILABLE,
                "api_error",
                format!("虚拟模型 '{requested_model}' 没有可用的成员"),
            );
        }
    };
    let virtual_model = route.virtual_model;
    let ordered = route.ordered;
    let retry_enabled = route.retry_enabled;

    match forward_through_members(
        state,
        ForwardFlavor::Native { endpoint, body },
        virtual_model.virtual_model_id,
        &api_key.name,
        &request_id,
        &requested_model,
        client_stream,
        retry_enabled,
        &opencode_session,
        &forwarded,
        &ordered,
    )
    .await
    {
        MemberLoopOutcome::Failed(response) => response,
        MemberLoopOutcome::Succeeded(success) => {
            // 成功：按端点协议原样中继响应。
            dispatch_native_success(
                state,
                endpoint,
                success.request_id,
                virtual_model.virtual_model_id,
                api_key.name.clone(),
                success.start_time,
                success.member,
                success.reply,
                client_stream,
            )
            .await
        }
    }
}
pub(crate) enum NativeUsageScanner {
    Anthropic(convert::anthropic::AnthropicStreamUsageScanner),
    Responses(convert::responses::ResponsesStreamUsageScanner),
}

impl NativeUsageScanner {
    fn feed(&mut self, bytes: &[u8]) {
        match self {
            NativeUsageScanner::Anthropic(scanner) => scanner.feed(bytes),
            NativeUsageScanner::Responses(scanner) => scanner.feed(bytes),
        }
    }

    fn take_content_seen(&mut self) -> bool {
        match self {
            NativeUsageScanner::Anthropic(scanner) => scanner.take_content_seen(),
            NativeUsageScanner::Responses(scanner) => scanner.take_content_seen(),
        }
    }

    fn usage(&self) -> Option<Usage> {
        match self {
            NativeUsageScanner::Anthropic(scanner) => scanner.usage(),
            NativeUsageScanner::Responses(scanner) => scanner.usage(),
        }
    }
}

/// 原生透传成功路径：非流式读全量响应体解析 usage 后原样返回；
/// 流式按原始字节中继 SSE（不重帧，`event:` 行保持原样），旁路扫描 usage。
#[allow(clippy::too_many_arguments)]
pub(crate) async fn dispatch_native_success(
    state: &AppState,
    endpoint: NativeEndpoint,
    request_id: String,
    virtual_model_id: i32,
    api_key_name: String,
    start_time: i64,
    member: Member,
    reply: UpstreamReply,
    client_stream: bool,
) -> Response {
    if !client_stream {
        let body = match upstream::read_body(reply.body).await {
            Ok(body) => body,
            Err(e) => {
                // 上游 200 后读体失败（超时/截断）：原样透传空体会造成假成功，
                // 与 chat 各协议非流式读失败同款 502 + 失败落库。
                let message = format!("读取上游响应失败：{e}");
                record_failure(
                    &state.db,
                    &request_id,
                    virtual_model_id,
                    &member,
                    &api_key_name,
                    start_time,
                    false,
                    &message,
                    reply.start_at_ms,
                );
                return endpoint.error(StatusCode::BAD_GATEWAY, "api_error", message);
            }
        };
        let parsed: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        let usage = match endpoint {
            NativeEndpoint::AnthropicMessages => parsed
                .get("usage")
                .filter(|usage| usage.is_object())
                .map(convert::anthropic::extract_usage)
                .unwrap_or_default(),
            NativeEndpoint::OpenAiResponses => parsed
                .get("usage")
                .and_then(convert::responses::ResponsesStreamConverter::extract_usage)
                .unwrap_or_default(),
        };
        let end_time = now_ms();
        RequestRecord {
            request_id,
            virtual_model_id,
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
            api_key_name,
        }
        .insert(&state.db);
        return (StatusCode::OK, [("content-type", "application/json")], body).into_response();
    }

    // 流式：原始字节直通 + 旁路 usage 扫描（SseSplitter 会丢弃 event: 行，
    // 原生客户端依赖其语义，故不重帧）。
    let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(32);
    let db = state.db.clone();
    let mut scanner = match endpoint {
        NativeEndpoint::AnthropicMessages => {
            NativeUsageScanner::Anthropic(convert::anthropic::AnthropicStreamUsageScanner::default())
        }
        NativeEndpoint::OpenAiResponses => {
            NativeUsageScanner::Responses(convert::responses::ResponsesStreamUsageScanner::default())
        }
    };
    let reply_start_at = reply.start_at_ms;
    let mut stream_metrics = StreamMetrics::new(reply.start_at_ms);
    tokio::spawn(async move {
        let mut body = reply.body;
        let mut disconnect = false;
        // 原生透传不重帧，无法补 error 帧；上游流中断时客户端收到裸截断
        // （协议原生语义），但指标必须按失败记。
        let mut upstream_failed: Option<String> = None;
        'outer: while let Some(frame) = body.frame().await {
            let bytes = match frame {
                Ok(frame) => frame.into_data().unwrap_or_default(),
                Err(e) => {
                    let message = format!("读取上游流失败：{e}");
                    upstream_failed = Some(message);
                    let _ = tx.send(Err(std::io::Error::other(e.to_string()))).await;
                    break 'outer;
                }
            };
            scanner.feed(&bytes);
            if scanner.take_content_seen() {
                stream_metrics.on_token();
            }
            if tx.send(Ok(bytes)).await.is_err() {
                disconnect = true;
                break 'outer;
            }
        }
        let end_time = now_ms();
        // 记账口径与 relay 统一（StreamOutcome；原生无转换错误概念，仅上游读错）。
        let outcome = StreamOutcome::from_parts(upstream_failed, None, None, disconnect);
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
            usage: scanner.usage().unwrap_or_default(),
            success: outcome.success(),
            fail_reason: outcome.fail_reason(),
            api_key_name,
        }
        .insert(&db);
    });
    sse_response(ReceiverStream::new(rx))
}

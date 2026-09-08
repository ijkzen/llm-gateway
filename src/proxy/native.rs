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

    // 路由：display_id 精确匹配 + 接口类型严格对应（鉴权失败与路由未命中不落 request 表）。
    let virtual_model = match virtual_model::Entity::find()
        .filter(virtual_model::Column::DisplayId.eq(&requested_model))
        .filter(virtual_model::Column::Enable.eq(true))
        .one(&state.db)
        .await
    {
        Ok(Some(model)) => model,
        Ok(None) => {
            return endpoint.error(
                StatusCode::NOT_FOUND,
                "not_found_error",
                format!("model '{requested_model}' does not exist"),
            );
        }
        Err(e) => {
            return endpoint.error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "api_error",
                format!("查询虚拟模型失败：{e}"),
            );
        }
    };
    if virtual_model.interface_type != endpoint.interface_type() {
        return endpoint.error(
            StatusCode::NOT_FOUND,
            "not_found_error",
            format!("model '{requested_model}' does not exist"),
        );
    }

    let members = match load_members(&state.db, virtual_model.virtual_model_id).await {
        Ok(members) => members,
        Err(e) => {
            return endpoint.error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "api_error",
                format!("查询模型成员失败：{e}"),
            );
        }
    };
    // 防御性过滤：成员协议须与端点对应（成员匹配规则保证，兜底跳过异协议成员）。
    let members: Vec<_> = members
        .into_iter()
        .filter(|member| member.protocol == endpoint.member_protocol())
        .collect();
    if members.is_empty() {
        return endpoint.error(
            StatusCode::SERVICE_UNAVAILABLE,
            "api_error",
            format!("虚拟模型 '{requested_model}' 没有可用的成员"),
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
    let retry_enabled = virtual_model.fallback_strategy == 1;
    if let Some(first) = ordered.first() {
        tracing::info!(
            request_id,
            virtual_model_id = virtual_model.virtual_model_id,
            requested_model = %requested_model,
            endpoint = ?endpoint,
            member_count = ordered.len(),
            selected_provider_id = first.provider_id,
            selected_model_id = %first.model_id,
            "原生透传 LB 选路结果",
        );
    }

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
        let body = upstream::read_body(reply.body).await.unwrap_or_default();
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
        'outer: while let Some(frame) = body.frame().await {
            let bytes = match frame {
                Ok(frame) => frame.into_data().unwrap_or_default(),
                Err(e) => {
                    let _ = tx.send(Err(std::io::Error::other(e.to_string()))).await;
                    break;
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
            success: true,
            fail_reason: disconnect.then(|| "客户端提前断开".to_string()),
            api_key_name,
        }
        .insert(&db);
    });
    sse_response(ReceiverStream::new(rx))
}

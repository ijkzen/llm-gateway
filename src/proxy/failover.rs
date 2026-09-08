use super::*;

pub(crate) struct SuccessContext {
    pub(crate) request_id: String,
    pub(crate) virtual_model_id: i32,
    pub(crate) api_key_name: String,
    pub(crate) requested_model: String,
    pub(crate) start_time: i64,
    pub(crate) member: Member,
    pub(crate) reply: UpstreamReply,
    pub(crate) client_stream: bool,
    pub(crate) include_usage: bool,
    pub(crate) json_mode_tool: bool,
    pub(crate) thinking_dropped: bool,
    pub(crate) reasoning_exclude: bool,
}

/// 成员尝试的请求构建产物：上游调用 + 协议转换侧标记（原生透传恒默认）。
pub(crate) struct AttemptBuild {
    pub(crate) call: UpstreamCall,
    pub(crate) flags: RequestFlags,
}

/// 成员尝试循环的终局：成功携带分派所需上下文（由调用方按端点分派），
/// 失败已按端点协议整形为最终响应。
pub(crate) enum MemberLoopOutcome {
    Succeeded(AttemptSuccess),
    Failed(Response),
}

/// 一次成功尝试携带出循环的数据：响应分派在调用方完成，循环内只选路。
pub(crate) struct AttemptSuccess {
    pub(crate) request_id: String,
    pub(crate) member: Member,
    pub(crate) reply: UpstreamReply,
    pub(crate) start_time: i64,
    pub(crate) flags: RequestFlags,
}

/// 成员尝试循环的两类端点形态：chat 协议转换 vs 原生透传。循环体唯一，
/// 差异（请求构建、错误/空候选响应整形）收敛在 flavor 上；
/// 成功后的响应分派由调用方完成。
pub(crate) enum ForwardFlavor {
    Chat {
        client_body: Value,
    },
    Native {
        endpoint: NativeEndpoint,
        body: Value,
    },
}

impl ForwardFlavor {
    /// 按端点协议构建成员请求：chat 走协议转换（带回转换侧标记），
    /// 原生透传仅改写 model。
    async fn build(
        &self,
        member: &Member,
        decrypted_key: &str,
        client_stream: bool,
        request_id: &str,
        opencode_session: &str,
        forwarded: &[(HeaderName, HeaderValue)],
    ) -> Result<AttemptBuild, String> {
        match self {
            ForwardFlavor::Chat { client_body, .. } => {
                let (call, flags) = build_upstream_call(
                    member,
                    client_body,
                    client_stream,
                    decrypted_key,
                    forwarded,
                    request_id,
                    opencode_session,
                )
                .await?;
                Ok(AttemptBuild { call, flags })
            }
            ForwardFlavor::Native { endpoint, body } => {
                let call = build_native_upstream_call(
                    *endpoint,
                    member,
                    body,
                    client_stream,
                    decrypted_key,
                    forwarded,
                    request_id,
                    opencode_session,
                )?;
                Ok(AttemptBuild {
                    call,
                    flags: RequestFlags::default(),
                })
            }
        }
    }

    /// 上游失败终态的响应整形：错误类型按端点协议从状态推导。
    fn fail_response(&self, status: StatusCode, message: impl Into<String>) -> Response {
        let message = message.into();
        match self {
            ForwardFlavor::Chat { .. } => {
                let error_type = if status.is_client_error() {
                    "invalid_request_error"
                } else {
                    "api_error"
                };
                openai_error(status, message, error_type, "upstream_error")
            }
            ForwardFlavor::Native { endpoint, .. } => {
                let error_type = if status == StatusCode::NOT_FOUND {
                    "not_found_error"
                } else if status.is_client_error() {
                    "invalid_request_error"
                } else {
                    "api_error"
                };
                endpoint.error(status, error_type, message)
            }
        }
    }

    /// 无可用候选（成员全被额度剔除）的响应：chat 沿用 server_error
    /// 语义，原生端点用协议错误信封；两条路径在此不再分叉。
    fn empty_ordered_response(&self, requested_model: &str) -> Response {
        let message = format!("虚拟模型 '{requested_model}' 没有可用的成员（订阅制额度均已耗尽）");
        match self {
            ForwardFlavor::Chat { .. } => openai_error(
                StatusCode::SERVICE_UNAVAILABLE,
                message,
                "server_error",
                "no_available_members",
            ),
            ForwardFlavor::Native { endpoint, .. } => {
                endpoint.error(StatusCode::SERVICE_UNAVAILABLE, "api_error", message)
            }
        }
    }
}

/// 在排序后的成员上执行统一尝试循环：逐个 解密 → 构建 → 调用 → 失败判定，
/// 可降级则记录并重试下一成员，否则按端点协议整形终态错误；成功清零
/// 连续失败计数并把成功上下文交给调用方分派。空候选（成员全被额度剔除）
/// 在此统一返回 503，chat 与原生透传共享同一条语义。
#[allow(clippy::too_many_arguments)]
pub(crate) async fn forward_through_members(
    state: &AppState,
    flavor: ForwardFlavor,
    virtual_model_id: i32,
    api_key_name: &str,
    request_id: &str,
    requested_model: &str,
    client_stream: bool,
    retry_enabled: bool,
    opencode_session: &str,
    forwarded: &[(HeaderName, HeaderValue)],
    ordered: &[Member],
) -> MemberLoopOutcome {
    if ordered.is_empty() {
        tracing::warn!(
            request_id,
            virtual_model_id,
            requested_model = %requested_model,
            "虚拟模型成员全部因额度耗尽不可用，无可用候选",
        );
        return MemberLoopOutcome::Failed(flavor.empty_ordered_response(requested_model));
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
                virtual_model_id,
                member,
                api_key_name,
                start_time,
                client_stream,
                message,
                ttft_start_ms,
            );
        };

        let decrypted_key = match crypto::decrypt(&member.api_key_encrypted) {
            Ok(key) => key,
            Err(e) => {
                let message = format!("解密供应商密钥失败：{e}");
                note_member_failure(state, member, request_id, &mut counted_failures).await;
                if retry_enabled && has_more {
                    tracing::warn!(
                        request_id,
                        virtual_model_id,
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
                    request_id,
                    virtual_model_id,
                    member,
                    api_key_name,
                    start_time,
                    client_stream,
                    &message,
                    start_time,
                );
                return MemberLoopOutcome::Failed(
                    flavor.fail_response(StatusCode::BAD_GATEWAY, message),
                );
            }
        };

        let build = flavor
            .build(
                member,
                &decrypted_key,
                client_stream,
                request_id,
                opencode_session,
                forwarded,
            )
            .await;
        let (call, flags) = match build {
            Ok(build) => (build.call, build.flags),
            Err(message) => {
                note_member_failure(state, member, request_id, &mut counted_failures).await;
                if retry_enabled && has_more {
                    tracing::warn!(
                        request_id,
                        virtual_model_id,
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
                    request_id,
                    virtual_model_id,
                    member,
                    api_key_name,
                    start_time,
                    client_stream,
                    &message,
                    start_time,
                );
                return MemberLoopOutcome::Failed(
                    flavor.fail_response(StatusCode::BAD_GATEWAY, message),
                );
            }
        };

        // Member 已由 resolve_proxy 归一：proxy_enabled 时地址必非空。
        let proxy = member.proxy_enabled.then_some(member.proxy_addr.as_str());
        let reply = match upstream::call(call, &state.upstream_pool, proxy).await {
            Ok(reply) => reply,
            Err(e) => {
                let message = e.fail_reason();
                note_member_failure(state, member, request_id, &mut counted_failures).await;
                if retry_enabled && has_more {
                    tracing::warn!(
                        request_id,
                        virtual_model_id,
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
                    request_id,
                    virtual_model_id,
                    member,
                    api_key_name,
                    start_time,
                    client_stream,
                    &message,
                    start_time,
                );
                return MemberLoopOutcome::Failed(
                    flavor.fail_response(StatusCode::BAD_GATEWAY, message),
                );
            }
        };

        if reply.status.as_u16() >= 400 {
            let body = upstream::read_body(reply.body).await.unwrap_or_default();
            let message = extract_error_message(&String::from_utf8_lossy(&body));
            let status = reply.status;
            note_member_failure(state, member, request_id, &mut counted_failures).await;
            if retry_enabled && has_more {
                tracing::warn!(
                    request_id,
                    virtual_model_id,
                    provider_id = member.provider_id,
                    model_id = %member.model_id,
                    attempt_index = index,
                    http_status = status.as_u16(),
                    fail_reason = %message,
                    "上游成员返回错误，降级重试下一成员",
                );
                record_degraded(&message, reply.start_at_ms);
                last_failure = Some((member.clone(), message, status));
                continue;
            }
            record_failure(
                &state.db,
                request_id,
                virtual_model_id,
                member,
                api_key_name,
                start_time,
                client_stream,
                &message,
                reply.start_at_ms,
            );
            return MemberLoopOutcome::Failed(flavor.fail_response(status, message));
        }

        // 成功即清零该供应商的连续失败计数（偶发失败不累积），
        // 随后把成功上下文交还调用方按端点分派。
        state.failure_counter.reset(member.provider_id);
        return MemberLoopOutcome::Succeeded(AttemptSuccess {
            request_id: request_id.to_string(),
            member: member.clone(),
            reply,
            start_time,
            flags,
        });
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
        virtual_model_id,
        provider_id = member.provider_id,
        model_id = %member.model_id,
        http_status = status.as_u16(),
        fail_reason = %message,
        "虚拟模型全部成员失败",
    );
    record_failure(
        &state.db,
        request_id,
        virtual_model_id,
        &member,
        api_key_name,
        now_ms(),
        client_stream,
        &message,
        now_ms(),
    );
    MemberLoopOutcome::Failed(flavor.fail_response(status, message))
}

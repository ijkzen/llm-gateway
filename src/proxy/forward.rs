use super::*;

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
    // exclude:true：模型照常思考，但网关剥除响应中的思考内容再交客户端
    //（OpenRouter reasoning.exclude 语义；OpenAI 直通为字节直通不生效）。
    let reasoning_exclude = chat_reasoning(&client_body)
        .enabled()
        .is_some_and(|reasoning| reasoning.exclude);

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

    // chat/completions 只服务 OpenAI Compatible / Full Compatible 类型；
    // Responses/Messages 专用模型按模型不存在处理（与 /v1/models 过滤一致）。
    if !virtual_model::CHAT_SERVED_TYPES.contains(&virtual_model.interface_type) {
        return openai_error(
            StatusCode::NOT_FOUND,
            format!("The model '{requested_model}' does not exist"),
            "invalid_request_error",
            "model_not_found",
        );
    }

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

    match forward_through_members(
        state,
        ForwardFlavor::Chat { client_body },
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
            dispatch_success(
                state,
                SuccessContext {
                    request_id: success.request_id,
                    virtual_model_id: virtual_model.virtual_model_id,
                    api_key_name: api_key.name.clone(),
                    requested_model: requested_model.clone(),
                    start_time: success.start_time,
                    member: success.member,
                    reply: success.reply,
                    client_stream,
                    include_usage,
                    json_mode_tool: success.flags.json_mode_tool,
                    thinking_dropped: success.flags.thinking_dropped,
                    reasoning_exclude,
                },
            )
            .await
        }
    }
}
/// 管理后台聊天请求写入 request 表时的来源标记：不属于任何虚拟模型，
/// 虚拟模型维度记 0；api_key_name 记 `chat` 便于数据面板区分。
const CHAT_VIRTUAL_MODEL_ID: i32 = 0;
const CHAT_API_KEY_NAME: &str = "chat";

/// 由供应商与供应商模型行构造转发成员（协议：模型单独指定优先，其次供应商；
/// 代理：模型级优先，其次供应商级）。
pub(crate) fn build_member(provider: &provider::Model, model: &provider_model::Model) -> Member {
    let (proxy_enabled, proxy_addr) = resolve_proxy(model, provider);
    let protocol_value = model.protocol_type.unwrap_or(provider.protocol_type);
    Member {
        provider_id: provider.id,
        model_id: model.provider_model_id.clone(),
        protocol: Protocol::from_i32(protocol_value),
        billing_mode: provider.billing_mode,
        base_url: provider.base_url.clone(),
        api_key_encrypted: provider.api_key.clone(),
        custom_header: provider.custom_header.clone(),
        proxy_enabled,
        proxy_addr,
    }
}

/// 管理后台聊天直连：按供应商 + 模型单成员转发（复用协议转换与连接池），
/// 强制流式返回 OpenAI chunk 风格 SSE。无 failover；失败不触碰可用性
/// 状态机（后台试用不该累积生产供应商的连续失败计数）。
pub async fn forward_chat_direct(
    state: &AppState,
    provider_id: i32,
    model_pk: i32,
    messages: Vec<Value>,
) -> Response {
    let provider = match provider::Entity::find_by_id(provider_id)
        .one(&state.db)
        .await
    {
        Ok(Some(row)) => row,
        Ok(None) => {
            return crate::response::not_found::<()>(format!("供应商 {provider_id} 不存在"))
                .into_response();
        }
        Err(e) => {
            return crate::response::internal_error::<()>(format!("查询供应商失败：{e}"))
                .into_response();
        }
    };
    // 可用性口径与选路一致：启用且无停用原因（读侧统一谓词）。
    if !crate::availability::traffic_available(&provider) {
        return crate::response::bad_request::<()>(format!(
            "供应商「{}」已停用，无法对话",
            provider.name
        ))
        .into_response();
    }
    let model = match provider_model::Entity::find_by_id(model_pk)
        .one(&state.db)
        .await
    {
        Ok(Some(row)) => row,
        Ok(None) => {
            return crate::response::not_found::<()>(format!("模型 {model_pk} 不存在"))
                .into_response();
        }
        Err(e) => {
            return crate::response::internal_error::<()>(format!("查询模型失败：{e}"))
                .into_response();
        }
    };
    if model.provider_id != provider_id {
        return crate::response::not_found::<()>(format!(
            "模型 {model_pk} 不属于供应商 {provider_id}"
        ))
        .into_response();
    }

    let member = build_member(&provider, &model);
    let request_id = Uuid::new_v4().to_string();
    let client_stream = true;
    let mut client_body = json!({
        "model": model.provider_model_id,
        "stream": true,
        "messages": messages,
    });
    // 思考模型显式申请思考输出，否则上游默认不返回思考内容
    //（实测 Command Code/Kimi/StepFun/SiliconFlow/Ant/OpenRouter/Xiaomi 均
    // 接受 reasoning_effort 且返回 reasoning_content）。
    if model.reasoning {
        client_body["reasoning_effort"] = json!("medium");
    }
    // 聊天无下游请求头（管理面发起）：透传子集为空。
    let start_time = now_ms();

    // 失败统一落库后以统一响应结构返回 502。
    let fail = |message: &str, ttft_start_ms: i64| {
        record_failure(
            &state.db,
            &request_id,
            CHAT_VIRTUAL_MODEL_ID,
            &member,
            CHAT_API_KEY_NAME,
            start_time,
            client_stream,
            message,
            ttft_start_ms,
        );
        crate::response::bad_gateway::<()>(message.to_string()).into_response()
    };

    let decrypted_key = match crypto::decrypt(&member.api_key_encrypted) {
        Ok(key) => key,
        Err(e) => return fail(&format!("解密供应商密钥失败：{e}"), start_time),
    };

    let (call, flags) = match build_upstream_call(
        &member,
        &client_body,
        client_stream,
        &decrypted_key,
        &[],
        &request_id,
        &opencode_session_fallback(CHAT_API_KEY_NAME),
    )
    .await
    {
        Ok(result) => result,
        Err(message) => return fail(&message, start_time),
    };

    // Member 已由 resolve_proxy 归一：proxy_enabled 时地址必非空。
    let proxy = member.proxy_enabled.then_some(member.proxy_addr.as_str());
    let reply = match upstream::call(call, &state.upstream_pool, proxy).await {
        Ok(reply) => reply,
        Err(e) => return fail(&e.fail_reason(), start_time),
    };

    if reply.status.as_u16() >= 400 {
        let body = upstream::read_body(reply.body).await.unwrap_or_default();
        let message = extract_error_message(&String::from_utf8_lossy(&body));
        let status = reply.status;
        return fail(&format!("上游返回 {status}：{message}"), reply.start_at_ms);
    }

    dispatch_success(
        state,
        SuccessContext {
            request_id,
            virtual_model_id: CHAT_VIRTUAL_MODEL_ID,
            api_key_name: CHAT_API_KEY_NAME.to_string(),
            requested_model: model.provider_model_id.clone(),
            start_time,
            member,
            reply,
            client_stream,
            include_usage: false,
            json_mode_tool: flags.json_mode_tool,
            thinking_dropped: flags.thinking_dropped,
            reasoning_exclude: false,
        },
    )
    .await
}

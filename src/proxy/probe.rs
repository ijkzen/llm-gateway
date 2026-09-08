use super::*;

/// 测试请求写入 request 表时用于标记来源的虚拟模型 ID 与 API Key 名。
/// 测试流量不属于任何虚拟模型，虚拟模型维度记 0；api_key_name 记 `test` 便于数据面板区分。
pub(crate) const TEST_VIRTUAL_MODEL_ID: i32 = 0;
pub(crate) const TEST_API_KEY_NAME: &str = "test";

/// 测试提示词：固定「你好」。部分模型可能拒答或返回空文本，但只要上游
/// 受理请求（HTTP 2xx）即判定模型有效（连通性验证）。
pub(crate) const TEST_PROMPT: &str = "你好";

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
    let member = build_member(provider_row, model);

    let chat = json!({
        "model": model.provider_model_id,
        "stream": false,
        "max_tokens": model.max_output_tokens,
        "messages": [{"role": "user", "content": TEST_PROMPT}],
    });
    // test_model 无下游请求头（管理面手动触发）：透传子集为空。
    let request_id = format!("test-{}", Uuid::new_v4());
    let (call, _flags) = build_upstream_call(
        &member,
        &chat,
        false,
        api_key,
        &[],
        &request_id,
        &opencode_session_fallback(TEST_API_KEY_NAME),
    )
    .await?;

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

/// 自动探活失败类型：`Skipped` = 无法探活（缺模型/密钥，本轮跳过不算失败）；
/// `Failed` = 最小测试请求真实失败（上游非 2xx 或网络错误）。
pub enum ProbeFailure {
    Skipped(String),
    Failed(String),
}

/// 自动探活：取该供应商 model_id 最小的模型发最小测试请求（与模型弹窗测速、
/// 失败恢复探测同一 `test_model` 入口）。用于用量刷新的订阅制边界探活：
/// 成功返回耗时；无法探活返回 `Skipped`；请求失败返回 `Failed` 与人类可读原因。
pub async fn probe_provider(
    state: &AppState,
    provider_row: &provider::Model,
) -> Result<i64, ProbeFailure> {
    let model = provider_model::Entity::find()
        .filter(provider_model::Column::ProviderId.eq(provider_row.id))
        .order_by_asc(provider_model::Column::ModelId)
        .one(&state.db)
        .await
        .map_err(|e| ProbeFailure::Skipped(format!("查询模型失败：{e}")))?;
    let Some(model) = model else {
        return Err(ProbeFailure::Skipped(
            "该供应商没有模型，无法探活".to_string(),
        ));
    };
    let api_key = match crypto::decrypt(&provider_row.api_key) {
        Ok(key) if !key.is_empty() => key,
        Ok(_) => {
            return Err(ProbeFailure::Skipped(
                "未配置 API Key，无法探活".to_string(),
            ));
        }
        Err(e) => return Err(ProbeFailure::Skipped(format!("API Key 解密失败：{e}"))),
    };
    test_model(state, provider_row, &model, &api_key)
        .await
        .map_err(ProbeFailure::Failed)
}

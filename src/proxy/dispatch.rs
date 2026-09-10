use super::*;

use super::relay::{Converter, chunk_has_content, strip_reasoning_delta};

/// exclude:true 时剥除非流式响应 message 中的思考内容（模型照常思考，客户端不收）。
fn strip_reasoning_message(completion: &mut Value) {
    if let Some(message) = completion
        .pointer_mut("/choices/0/message")
        .and_then(Value::as_object_mut)
    {
        message.remove("reasoning_content");
        message.remove("reasoning_details");
    }
}

/// 把流式转换出的 chunk 列表聚合为非流式 chat.completion（Responses 出站非流式路径）。
pub fn accumulate_chunks(chunks: &[Value], usage: &Usage) -> Value {
    let mut id = String::from("chatcmpl");
    let mut model = String::new();
    let mut content = String::new();
    let mut reasoning = String::new();
    let mut reasoning_details: Vec<Value> = Vec::new();
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
        if let Some(details) = delta.get("reasoning_details").and_then(Value::as_array) {
            reasoning_details.extend(details.iter().cloned());
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
            .map(|(_, (call_id, name, arguments))| {
                json!({
                    "id": call_id,
                    "type": "function",
                    "function": {"name": name, "arguments": arguments},
                })
            })
            .collect();
        message.insert("tool_calls".to_string(), Value::Array(calls));
    }
    if !reasoning.is_empty() {
        message.insert("reasoning_content".to_string(), json!(reasoning));
    }
    attach_reasoning_details(&mut message, reasoning_details);

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

#[allow(clippy::too_many_lines)]
pub(crate) async fn dispatch_success(state: &AppState, ctx: SuccessContext) -> Response {
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
        thinking_dropped,
        reasoning_exclude,
    } = ctx;

    /// 200 后失败路径的日志（03-03）：上游已接收请求、内容未成功交付，属排障盲区，
    /// 与 failover 降级/终态日志同字段形状（request_id/provider/model/fail_reason）。
    fn log_dispatch_failure(request_id: &str, member: &Member, message: &str) {
        tracing::warn!(
            request_id,
            provider_id = member.provider_id,
            model_id = %member.model_id,
            fail_reason = %message,
            "上游响应处理失败，返回错误响应",
        );
    }

    // 成功即清零该供应商的连续失败计数（偶发失败不累积）。
    state.failure_counter.reset(member.provider_id);

    match (member.protocol, client_stream) {
        // OpenAI Compat 非流式：JSON 原样透传。
        (Protocol::OpenAiCompat, false) => {
            let body = match upstream::read_body(reply.body).await {
                Ok(body) => body,
                Err(e) => {
                    let message = format!("读取上游响应失败：{e}");
                    log_dispatch_failure(&request_id, &member, &message);
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
                    return openai_error(
                        StatusCode::BAD_GATEWAY,
                        message,
                        "api_error",
                        "upstream_error",
                    );
                }
            };
            let text = String::from_utf8_lossy(&body).to_string();
            // 200 但响应体不是合法 JSON（空体/被截断）同样是上游故障：透传空壳会
            // 造成客户端收到假成功，按 Anthropic/Gemini 非流式同款 502 处理。
            let parsed: Value = match serde_json::from_str(&text) {
                Ok(value) => value,
                Err(e) => {
                    let message = format!("解析上游响应失败：{e}");
                    log_dispatch_failure(&request_id, &member, &message);
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
                    return openai_error(
                        StatusCode::BAD_GATEWAY,
                        message,
                        "api_error",
                        "upstream_error",
                    );
                }
            };
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
        // OpenAI Compat 流式：事件直通（重帧）+ 旁路扫描统计；客户端未请求
        // include_usage 时过滤注入产生的 usage 尾块。（泵骨架已收拢至 relay。）
        (Protocol::OpenAiCompat, true) => {
            let response = relay_stream(
                state.db.clone(),
                reply,
                PumpSource::OpenAi {
                    scanner: openai::OpenAiStreamScanner::default(),
                    include_usage,
                },
                TailSpec::Plain,
                RecordCtx {
                    request_id,
                    virtual_model_id,
                    member,
                    api_key_name,
                    start_time,
                },
            );
            with_thinking_dropped_header(response, thinking_dropped)
        }
        // Responses 出站：上游强制流式。
        // Responses 出站恒为上游流式；客户端 stream=true 时 live 逐事件转换
        // 转发（不再整条缓冲后回放：TTFB=首个转换事件耗时，峰值内存=单帧）。
        (Protocol::OpenAiResponses, true) => {
            let response = relay_stream(
                state.db.clone(),
                reply,
                PumpSource::Convert {
                    converter: Converter::Responses(Box::new(
                        responses::ResponsesStreamConverter::new(&request_id, &requested_model),
                    )),
                    reasoning_exclude,
                },
                TailSpec::ResponsesUsage { include_usage },
                RecordCtx {
                    request_id,
                    virtual_model_id,
                    member,
                    api_key_name,
                    start_time,
                },
            );
            with_thinking_dropped_header(response, thinking_dropped)
        }
        // Responses 出站恒为上游流式；客户端非流式时收集整条流后聚合为 JSON。
        (Protocol::OpenAiResponses, false) => {
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
                    false,
                    &error,
                    reply.start_at_ms,
                );
                let status = StatusCode::BAD_GATEWAY;
                return openai_error(status, error, "api_error", "upstream_error");
            }
            let usage = converter.usage().unwrap_or_default();
            let mut completion = accumulate_chunks(&events.chunks, &usage);
            if reasoning_exclude {
                strip_reasoning_message(&mut completion);
            }
            let end_time = now_ms();
            RequestRecord {
                request_id,
                virtual_model_id,
                provider_id: member.provider_id,
                model_id: member.model_id.clone(),
                stream: false,
                ttft: events.stream_metrics.ttft_ms(),
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
            with_thinking_dropped_header(
                (StatusCode::OK, axum::Json(completion)).into_response(),
                thinking_dropped,
            )
        }
        // Anthropic / Gemini：非流式直接转换；流式逐事件转换后转发。
        (protocol, client_stream) => {
            if !client_stream {
                let body = upstream::read_body(reply.body).await.unwrap_or_default();
                let text = String::from_utf8_lossy(&body).to_string();
                let parsed: Value = match serde_json::from_str(&text) {
                    Ok(value) => value,
                    Err(e) => {
                        let message = format!("解析上游响应失败：{e}");
                        log_dispatch_failure(&request_id, &member, &message);
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
                    Ok((mut completion, usage)) => {
                        if reasoning_exclude {
                            strip_reasoning_message(&mut completion);
                        }
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
                        with_thinking_dropped_header(
                            (StatusCode::OK, axum::Json(completion)).into_response(),
                            thinking_dropped,
                        )
                    }
                    Err(message) => {
                        log_dispatch_failure(&request_id, &member, &message);
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
                        openai_error(
                            StatusCode::BAD_GATEWAY,
                            message,
                            "api_error",
                            "upstream_error",
                        )
                    }
                };
            }

            // 流式：逐事件转换并推送给客户端（泵骨架已收拢至 relay；转换失败
            // 与带内错误统一按失败记账——不再记假成功）。
            let converter = match protocol {
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
            let response = relay_stream(
                state.db.clone(),
                reply,
                PumpSource::Convert {
                    converter,
                    reasoning_exclude,
                },
                TailSpec::ConvertFinish {
                    include_usage,
                    requested_model,
                },
                RecordCtx {
                    request_id,
                    virtual_model_id,
                    member,
                    api_key_name,
                    start_time,
                },
            );
            with_thinking_dropped_header(response, thinking_dropped)
        }
    }
}

/// 上游流式事件收集（Responses 聚合路径，仅非流式客户端使用）。
pub(crate) struct CollectedEvents {
    chunks: Vec<Value>,
    stream_metrics: StreamMetrics,
    error: Option<String>,
}

pub(crate) async fn collect_stream_events(
    body: &mut PooledBody,
    converter: &mut Converter,
    stream_metrics: &mut StreamMetrics,
) -> CollectedEvents {
    let mut splitter = crate::proxy::sse::SseSplitter::default();
    let mut chunks = Vec::new();
    let mut error = None;
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
    CollectedEvents {
        chunks,
        stream_metrics: std::mem::take(stream_metrics),
        error,
    }
}

pub(crate) fn sse_response(stream: ReceiverStream<Result<Bytes, std::io::Error>>) -> Response {
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
pub(crate) fn record_failure(
    db: &DatabaseConnection,
    request_id: &str,
    virtual_model_id: i32,
    member: &Member,
    api_key_name: &str,
    start_time: i64,
    stream: bool,
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

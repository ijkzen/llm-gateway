use super::*;

pub(crate) enum Converter {
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

/// exclude:true 时剥除流式 delta 中的思考增量。
fn strip_reasoning_delta(chunk: &mut Value) {
    if let Some(delta) = chunk
        .pointer_mut("/choices/0/delta")
        .and_then(Value::as_object_mut)
    {
        delta.remove("reasoning_content");
        delta.remove("reasoning_details");
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

    // 成功即清零该供应商的连续失败计数（偶发失败不累积）。
    state.failure_counter.reset(member.provider_id);

    match (member.protocol, client_stream) {
        // OpenAI Compat 非流式：JSON 原样透传。
        (Protocol::OpenAiCompat, false) => {
            let body = match upstream::read_body(reply.body).await {
                Ok(body) => body,
                Err(e) => {
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
        // include_usage 时过滤注入产生的 usage 尾块。
        (Protocol::OpenAiCompat, true) => {
            let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(32);
            let db = state.db.clone();
            let mut scanner = openai::OpenAiStreamScanner::default();
            let reply_start_at = reply.start_at_ms;
            let mut stream_metrics = StreamMetrics::new(reply.start_at_ms);
            tokio::spawn(async move {
                let mut body = reply.body;
                let mut splitter = crate::proxy::sse::SseSplitter::default();
                let mut disconnect = false;
                // 上游流中断（hyper 帧错误/连接重置）与客户端断开是两种结局：
                // 前者记失败并补 error 帧 + [DONE] 收尾，后者客户端已不在。
                let mut upstream_failed: Option<String> = None;
                'outer: while let Some(frame) = body.frame().await {
                    let bytes = match frame {
                        Ok(frame) => frame.into_data().unwrap_or_default(),
                        Err(e) => {
                            let message = format!("读取上游流失败：{e}");
                            upstream_failed = Some(message.clone());
                            let error_frame = format!(
                                "data: {}\n\n",
                                json!({"error": {"message": message, "type": "api_error", "code": "upstream_error"}})
                            );
                            let _ = tx.send(Ok(Bytes::from(error_frame))).await;
                            break 'outer;
                        }
                    };
                    let text = String::from_utf8_lossy(&bytes).to_string();
                    for event in splitter.feed(&text) {
                        scanner.feed_event(&event);
                        if scanner.saw_content {
                            scanner.saw_content = false;
                            stream_metrics.on_token();
                        }
                        // include_usage 注入只为统计指标；客户端未请求时，
                        // 空 choices 的 usage 尾块不透出（OpenAI 规范语义）。
                        if !include_usage && openai::is_usage_only_chunk(&event) {
                            continue;
                        }
                        if tx
                            .send(Ok(Bytes::from(crate::proxy::sse::sse_frame(&event))))
                            .await
                            .is_err()
                        {
                            disconnect = true;
                            break 'outer;
                        }
                    }
                }
                // 上游中断时补 [DONE] 收尾（正常路径 [DONE] 由上游自带）。
                if upstream_failed.is_some() {
                    let _ = tx
                        .send(Ok(Bytes::from("data: [DONE]\n\n".to_string())))
                        .await;
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
                    success: upstream_failed.is_none(),
                    fail_reason: upstream_failed
                        .or(disconnect.then(|| "客户端提前断开".to_string())),
                    api_key_name,
                }
                .insert(&db);
            });
            with_thinking_dropped_header(sse_response(ReceiverStream::new(rx)), thinking_dropped)
        }
        // Responses 出站：上游强制流式。
        // Responses 出站恒为上游流式；客户端 stream=true 时 live 逐事件转换
        // 转发（不再整条缓冲后回放：TTFB=首个转换事件耗时，峰值内存=单帧）。
        (Protocol::OpenAiResponses, true) => {
            let mut converter = Converter::Responses(Box::new(
                responses::ResponsesStreamConverter::new(&request_id, &requested_model),
            ));
            let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(32);
            let db = state.db.clone();
            let reply_start_at = reply.start_at_ms;
            let mut stream_metrics = StreamMetrics::new(reply.start_at_ms);
            tokio::spawn(async move {
                let mut body = reply.body;
                let mut splitter = crate::proxy::sse::SseSplitter::default();
                let mut disconnect = false;
                // 上游帧错误/转换失败与客户端断开分开记账：前者补 error 帧并按
                // 失败落库，收尾的 usage 尾块仅在无错误时补发。
                let mut failed: Option<String> = None;
                'outer: while let Some(frame) = body.frame().await {
                    let bytes = match frame {
                        Ok(frame) => frame.into_data().unwrap_or_default(),
                        Err(e) => {
                            let message = format!("读取上游流失败：{e}");
                            failed = Some(message.clone());
                            let error_frame = format!(
                                "data: {}\n\n",
                                json!({"error": {"message": message, "type": "api_error", "code": "upstream_error"}})
                            );
                            let _ = tx.send(Ok(Bytes::from(error_frame))).await;
                            break 'outer;
                        }
                    };
                    let text = String::from_utf8_lossy(&bytes).to_string();
                    for event in splitter.feed(&text) {
                        match converter.convert_event(&event) {
                            Ok(chunks) => {
                                for mut chunk in chunks {
                                    if chunk_has_content(&chunk) {
                                        stream_metrics.on_token();
                                    }
                                    if reasoning_exclude {
                                        strip_reasoning_delta(&mut chunk);
                                    }
                                    let frame = crate::proxy::sse::sse_frame(&chunk.to_string());
                                    if tx.send(Ok(Bytes::from(frame))).await.is_err() {
                                        disconnect = true;
                                        break 'outer;
                                    }
                                }
                            }
                            Err(message) => {
                                // 转换失败（上游事件畸形/语义错误）：发 error 帧收尾。
                                failed = Some(message.clone());
                                let error_frame = format!(
                                    "data: {}\n\n",
                                    json!({"error": {"message": message, "type": "api_error", "code": "upstream_error"}})
                                );
                                let _ = tx.send(Ok(Bytes::from(error_frame))).await;
                                break 'outer;
                            }
                        }
                        if converter.is_finished() {
                            break 'outer;
                        }
                    }
                }
                if failed.is_none()
                    && let Some(converter_error) = converter.error()
                {
                    failed = Some(converter_error);
                }
                // 正常收尾补 usage 尾块（include_usage 注入只为统计口径透出）。
                if failed.is_none()
                    && include_usage
                    && let Some(usage) = converter.usage()
                {
                    let frame = crate::proxy::sse::sse_frame(
                        &usage_chunk_json(
                            &converter.completion_id(),
                            &converter.completion_model(),
                            cached_client_usage_json(&usage),
                        )
                        .to_string(),
                    );
                    let _ = tx.send(Ok(Bytes::from(frame))).await;
                }
                let _ = tx.send(Ok(Bytes::from("data: [DONE]\n\n"))).await;
                let end_time = now_ms();
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
                    success: failed.is_none(),
                    fail_reason: failed.or(disconnect.then(|| "客户端提前断开".to_string())),
                    api_key_name,
                }
                .insert(&db);
            });
            with_thinking_dropped_header(sse_response(ReceiverStream::new(rx)), thinking_dropped)
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

            // 流式：逐事件转换并推送给客户端。
            let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(32);
            let db = state.db.clone();
            let reply_start_at = reply.start_at_ms;
            let mut stream_metrics = StreamMetrics::new(reply.start_at_ms);
            tokio::spawn(async move {
                let mut body = reply.body;
                let mut splitter = crate::proxy::sse::SseSplitter::default();
                let mut disconnect = false;
                // 上游流中断与客户端断开分开记账：前者发 error 帧并按失败落库，
                // 收尾的 finish/usage 补发仅在无错误时进行。
                let mut upstream_failed: Option<String> = None;
                'outer: while let Some(frame) = body.frame().await {
                    let bytes = match frame {
                        Ok(frame) => frame.into_data().unwrap_or_default(),
                        Err(e) => {
                            let message = format!("读取上游流失败：{e}");
                            upstream_failed = Some(message.clone());
                            let error_frame = format!(
                                "data: {}\n\n",
                                json!({"error": {"message": message, "type": "api_error", "code": "upstream_error"}})
                            );
                            let _ = tx.send(Ok(Bytes::from(error_frame))).await;
                            break 'outer;
                        }
                    };
                    let text = String::from_utf8_lossy(&bytes).to_string();
                    for event in splitter.feed(&text) {
                        match converter.convert_event(&event) {
                            Ok(chunks) => {
                                for mut chunk in chunks {
                                    if chunk_has_content(&chunk) {
                                        stream_metrics.on_token();
                                    }
                                    if reasoning_exclude {
                                        strip_reasoning_delta(&mut chunk);
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
                // 补发缺失的 finish / usage / [DONE]（上游中断时跳过，error 帧后仅收 [DONE]）。
                if converter.error().is_none() && upstream_failed.is_none() {
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
                        // 各协议统一带缓存明细（与非流式口径一致）。
                        let usage_chunk = usage_chunk_json(
                            &converter.completion_id(),
                            &requested_model,
                            cached_client_usage_json(&usage),
                        );
                        let frame = usage_chunk.to_string();
                        let _ = tx
                            .send(Ok(Bytes::from(crate::proxy::sse::sse_frame(&frame))))
                            .await;
                    }
                }
                let _ = tx.send(Ok(Bytes::from("data: [DONE]\n\n"))).await;
                let end_time = now_ms();
                let success = upstream_failed.is_none() && converter.error().is_none();
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
                    fail_reason: upstream_failed
                        .or_else(|| converter.error().clone())
                        .or(disconnect.then(|| "客户端提前断开".to_string())),
                    api_key_name,
                }
                .insert(&db);
            });
            with_thinking_dropped_header(sse_response(ReceiverStream::new(rx)), thinking_dropped)
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

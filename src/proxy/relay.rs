//! 流式转运泵（relay）：OpenAI 直通 / Responses・Anthropic・Gemini 转换四臂
//! 的统一实现。泵骨架（channel(32) → spawn → SSE 拆分 → 发送 → 收尾 → 落库）
//! 单点持有；协议差异收敛在 PumpSource（事件源）与 TailSpec（收尾策略）；
//! 成功/失败记账口径（StreamOutcome）三臂同一语义：上游 200 后任何中断或
//! 转换失败都不算成功（E1/E2），native 原生透传共享同一口径。

use super::*;

/// 流式转换事件源的统一门面（Responses/Anthropic/Gemini 三转换器共用），
/// 原定义在 dispatch.rs；随泵收拢迁至 relay（泵与聚合收集共用）。
pub(crate) enum Converter {
    Anthropic(Box<anthropic::AnthropicStreamConverter>),
    Responses(Box<responses::ResponsesStreamConverter>),
    Gemini(Box<gemini::GeminiStreamConverter>),
}

impl Converter {
    pub(crate) fn convert_event(&mut self, data: &str) -> Result<Vec<Value>, String> {
        match self {
            Converter::Anthropic(c) => c.convert_event(data),
            Converter::Responses(c) => c.convert_event(data),
            Converter::Gemini(c) => c.convert_event(data),
        }
    }

    pub(crate) fn usage(&self) -> Option<Usage> {
        match self {
            Converter::Anthropic(c) => c.usage().cloned(),
            Converter::Responses(c) => c.usage().cloned(),
            Converter::Gemini(c) => c.usage().cloned(),
        }
    }

    pub(crate) fn is_finished(&self) -> bool {
        match self {
            Converter::Anthropic(c) => c.is_finished(),
            Converter::Responses(c) => c.is_finished(),
            Converter::Gemini(c) => c.is_finished(),
        }
    }

    pub(crate) fn error(&self) -> Option<String> {
        match self {
            Converter::Anthropic(c) => c.error().cloned(),
            Converter::Responses(c) => c.error().cloned(),
            Converter::Gemini(c) => c.error().cloned(),
        }
    }

    pub(crate) fn has_finish(&self) -> bool {
        match self {
            Converter::Anthropic(c) => c.has_finish(),
            Converter::Responses(c) => c.has_finish(),
            Converter::Gemini(c) => c.has_finish(),
        }
    }

    pub(crate) fn final_chunk(&mut self) -> Option<Value> {
        match self {
            Converter::Anthropic(_) => None,
            Converter::Responses(_) => None,
            Converter::Gemini(c) => c.final_chunk(),
        }
    }

    pub(crate) fn completion_model(&self) -> String {
        match self {
            Converter::Responses(c) => c.completion_model().to_string(),
            Converter::Anthropic(_) | Converter::Gemini(_) => {
                unreachable!("only Responses uses upstream completion metadata")
            }
        }
    }

    pub(crate) fn completion_id(&self) -> String {
        match self {
            Converter::Anthropic(c) => c.completion_id().to_string(),
            Converter::Responses(c) => c.completion_id().to_string(),
            Converter::Gemini(c) => c.completion_id().to_string(),
        }
    }
}

/// chunk 是否携带内容（用于 ttft / 末 token 时刻统计）。
pub(crate) fn chunk_has_content(chunk: &Value) -> bool {
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

/// exclude:true 时剥除流式 delta 中的思考增量。
pub(crate) fn strip_reasoning_delta(chunk: &mut Value) {
    if let Some(delta) = chunk
        .pointer_mut("/choices/0/delta")
        .and_then(Value::as_object_mut)
    {
        delta.remove("reasoning_content");
        delta.remove("reasoning_details");
    }
}

/// 流式结局的统一记账：上游读错 / 转换失败（Err 返回）/ 转换器内部错误态
/// （带内错误事件）/ 客户端断开 → success / fail_reason。
/// 客户端断开不影响 success（上游交付完整），仅补记原因。
#[derive(Default)]
pub(crate) struct StreamOutcome {
    upstream_err: Option<String>,
    convert_err: Option<String>,
    source_err: Option<String>,
    disconnect: bool,
}

impl StreamOutcome {
    pub(crate) fn from_parts(
        upstream_err: Option<String>,
        convert_err: Option<String>,
        source_err: Option<String>,
        disconnect: bool,
    ) -> Self {
        Self {
            upstream_err,
            convert_err,
            source_err,
            disconnect,
        }
    }

    pub(crate) fn success(&self) -> bool {
        self.upstream_err.is_none() && self.convert_err.is_none() && self.source_err.is_none()
    }

    pub(crate) fn fail_reason(&self) -> Option<String> {
        self.upstream_err
            .clone()
            .or_else(|| self.convert_err.clone())
            .or_else(|| self.source_err.clone())
            .or(self.disconnect.then(|| "客户端提前断开".to_string()))
    }
}

/// 事件源：每 SSE 事件的处理结果。
enum PumpStep {
    /// 先发送已产出的帧，再按失败收尾（带内错误事件已产生内容增量的场景）。
    FramesThenFailed(Vec<Bytes>, String),
    /// 正常产出（0..n 帧待发送；被过滤时为 0 帧）。
    Frames(Vec<Bytes>),
    /// 转换失败（Err 返回）：记 convert_err、发 error 帧、终止。
    Failed(String),
}

/// 泵事件源（协议差异点一）：直通扫描 vs 逐事件转换。
pub(crate) enum PumpSource {
    /// OpenAI Compat 直通：旁路扫描统计 + 原事件重帧；客户端未请求
    /// include_usage 时过滤注入产生的 usage 尾块（OpenAI 规范语义）。
    OpenAi {
        scanner: openai::OpenAiStreamScanner,
        include_usage: bool,
    },
    /// 转换系（Responses/Anthropic/Gemini）：逐事件转换，reasoning_exclude
    /// 时剥除思考增量。
    Convert {
        converter: Converter,
        reasoning_exclude: bool,
    },
}

impl PumpSource {
    fn on_event(&mut self, event: &str, stream_metrics: &mut StreamMetrics) -> PumpStep {
        match self {
            PumpSource::OpenAi {
                scanner,
                include_usage,
            } => {
                scanner.feed_event(event);
                if scanner.saw_content {
                    scanner.saw_content = false;
                    stream_metrics.on_token();
                }
                if !*include_usage && openai::is_usage_only_chunk(event) {
                    return PumpStep::Frames(Vec::new());
                }
                PumpStep::Frames(vec![Bytes::from(crate::proxy::sse::sse_frame(event))])
            }
            PumpSource::Convert {
                converter,
                reasoning_exclude,
            } => {
                let chunks = match converter.convert_event(event) {
                    Ok(chunks) => chunks,
                    Err(message) => return PumpStep::Failed(message),
                };
                let mut frames = Vec::with_capacity(chunks.len());
                for mut chunk in chunks {
                    if chunk_has_content(&chunk) {
                        stream_metrics.on_token();
                    }
                    if *reasoning_exclude {
                        strip_reasoning_delta(&mut chunk);
                    }
                    frames.push(Bytes::from(crate::proxy::sse::sse_frame(
                        &chunk.to_string(),
                    )));
                }
                // 03-01：上游 200 SSE 流内的错误事件（Responses error/response.failed、
                // Anthropic error、Gemini {"error":..}）只把转换器置 error 态而不产出
                // 错误帧——不补发客户端会收到「内容截断但流正常结束」的假成功。
                if let Some(message) = self.error() {
                    if frames.is_empty() {
                        return PumpStep::Failed(message);
                    }
                    return PumpStep::FramesThenFailed(frames, message);
                }
                PumpStep::Frames(frames)
            }
        }
    }

    /// 事件源是否已标记流结束（含最终内容的事件先发帧、再查此态，帧不丢）。
    fn finished(&self) -> bool {
        match self {
            PumpSource::OpenAi { .. } => false,
            PumpSource::Convert { converter, .. } => converter.is_finished(),
        }
    }

    fn usage(&self) -> Option<Usage> {
        match self {
            PumpSource::OpenAi { scanner, .. } => scanner.usage.clone(),
            PumpSource::Convert { converter, .. } => converter.usage(),
        }
    }

    fn error(&self) -> Option<String> {
        match self {
            PumpSource::OpenAi { .. } => None,
            PumpSource::Convert { converter, .. } => converter.error(),
        }
    }

    /// 是否已收到上游 [DONE]（仅 OpenAI 直通有该语义）。
    fn saw_done(&self) -> bool {
        match self {
            PumpSource::OpenAi { scanner, .. } => scanner.saw_done,
            PumpSource::Convert { .. } => false,
        }
    }
}

/// 收尾策略（协议差异点二）：无错误时执行；错误收尾（error 帧 + [DONE]）
/// 由泵统一处理。
pub(crate) enum TailSpec {
    /// OpenAI：无注入（上游自带 [DONE] 直通）；上游中断时仅补 [DONE]。
    Plain,
    /// Responses：无错误时 include_usage 补 usage 尾块（id/model 取自
    /// converter）；恒补 [DONE]。
    ResponsesUsage { include_usage: bool },
    /// Anthropic/Gemini：无错误时 final_chunk + 缺 finish 合成 + include_usage
    /// usage 尾块（model 用请求模型）；恒补 [DONE]。
    ConvertFinish {
        include_usage: bool,
        requested_model: String,
    },
}

fn error_frame(message: &str) -> Bytes {
    Bytes::from(format!(
        "data: {}\n\n",
        json!({"error": {"message": message, "type": "api_error", "code": "upstream_error"}})
    ))
}

/// 落库上下文（与 RequestRecord 字段一一对应，泵只消费不解释）。
pub(crate) struct RecordCtx {
    pub(crate) request_id: String,
    pub(crate) virtual_model_id: i32,
    pub(crate) member: Member,
    pub(crate) api_key_name: String,
    pub(crate) start_time: i64,
}

/// 统一流式泵：逐帧读上游 → 事件源处理 → 发送 → 收尾 → 按统一口径落库，
/// 返回客户端 SSE 响应。
///
/// 泵内联循环与 `dispatch.rs::collect_stream_events` 是同构的两份实现（那份
/// 整流收集给非流式客户端）：两份必须同步演化——03-01 的假成功 bug 正是
/// 「整流侧已处理 converter.error()、泵侧漏了」的分叉产物；改动对照另一份。
pub(crate) fn relay_stream(
    db: DatabaseConnection,
    reply: UpstreamReply,
    source: PumpSource,
    tail: TailSpec,
    record: RecordCtx,
) -> Response {
    let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(32);
    let reply_start_at = reply.start_at_ms;
    let mut stream_metrics = StreamMetrics::new(reply.start_at_ms);
    tokio::spawn(async move {
        let mut body = reply.body;
        let mut splitter = crate::proxy::sse::SseSplitter::default();
        let mut upstream_err: Option<String> = None;
        let mut convert_err: Option<String> = None;
        let mut disconnect = false;
        let mut source = source;
        'outer: while let Some(frame) = body.frame().await {
            let bytes = match frame {
                Ok(frame) => frame.into_data().unwrap_or_default(),
                Err(e) => {
                    // 03-02：上游 [DONE] 之后的读错误属连接 teardown 噪音——内容已
                    // 完整交付，不再补 error 帧/翻失败（否则客户端会在 [DONE] 后收到
                    // 第二个错误帧与第二个 [DONE]，且整单被记失败）。
                    if source.saw_done() && matches!(tail, TailSpec::Plain) {
                        tracing::debug!(
                            request_id = %record.request_id,
                            "上游 [DONE] 之后的读错误（连接收尾噪音），按成功结束"
                        );
                        break 'outer;
                    }
                    let message = format!("读取上游流失败：{e}");
                    tracing::warn!(
                        request_id = %record.request_id,
                        provider_id = record.member.provider_id,
                        model_id = %record.member.model_id,
                        fail_reason = %message,
                        "流式转运中断，向客户端补发错误帧",
                    );
                    upstream_err = Some(message.clone());
                    let _ = tx.send(Ok(error_frame(&message))).await;
                    break 'outer;
                }
            };
            let text = String::from_utf8_lossy(&bytes).to_string();
            for event in splitter.feed(&text) {
                match source.on_event(&event, &mut stream_metrics) {
                    PumpStep::Frames(frames) => {
                        for frame in frames {
                            if tx.send(Ok(frame)).await.is_err() {
                                disconnect = true;
                                break 'outer;
                            }
                        }
                    }
                    PumpStep::FramesThenFailed(frames, message) => {
                        for frame in frames {
                            if tx.send(Ok(frame)).await.is_err() {
                                disconnect = true;
                                break 'outer;
                            }
                        }
                        tracing::warn!(
                            request_id = %record.request_id,
                            provider_id = record.member.provider_id,
                            model_id = %record.member.model_id,
                            fail_reason = %message,
                            "上游流内错误事件，向客户端补发错误帧",
                        );
                        convert_err = Some(message.clone());
                        let _ = tx.send(Ok(error_frame(&message))).await;
                        break 'outer;
                    }
                    PumpStep::Failed(message) => {
                        tracing::warn!(
                            request_id = %record.request_id,
                            provider_id = record.member.provider_id,
                            model_id = %record.member.model_id,
                            fail_reason = %message,
                            "流式事件转换失败，向客户端补发错误帧",
                        );
                        convert_err = Some(message.clone());
                        let _ = tx.send(Ok(error_frame(&message))).await;
                        break 'outer;
                    }
                }
                if source.finished() {
                    break 'outer;
                }
            }
        }
        let clean = upstream_err.is_none() && convert_err.is_none() && source.error().is_none();

        // 收尾：无错误时按策略注入；[DONE] 规则按策略（Plain 仅上游中断后补，
        // 其余恒补——正常路径 [DONE] 由上游自带直通）。
        if clean {
            match (&mut source, &tail) {
                (
                    PumpSource::Convert { converter, .. },
                    TailSpec::ResponsesUsage { include_usage },
                ) => {
                    if *include_usage && let Some(usage) = converter.usage() {
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
                }
                (
                    PumpSource::Convert { converter, .. },
                    TailSpec::ConvertFinish {
                        include_usage,
                        requested_model,
                    },
                ) => {
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
                            requested_model,
                            json!({}),
                            Some("stop"),
                        );
                        let _ = tx
                            .send(Ok(Bytes::from(crate::proxy::sse::sse_frame(
                                &finish.to_string(),
                            ))))
                            .await;
                    }
                    if *include_usage && let Some(usage) = converter.usage() {
                        let usage_chunk = usage_chunk_json(
                            &converter.completion_id(),
                            requested_model,
                            cached_client_usage_json(&usage),
                        );
                        let _ = tx
                            .send(Ok(Bytes::from(crate::proxy::sse::sse_frame(
                                &usage_chunk.to_string(),
                            ))))
                            .await;
                    }
                }
                _ => {}
            }
        }
        let send_done = match tail {
            TailSpec::Plain => upstream_err.is_some(),
            _ => true,
        };
        if send_done {
            let _ = tx.send(Ok(Bytes::from("data: [DONE]\n\n"))).await;
        }

        let outcome =
            StreamOutcome::from_parts(upstream_err, convert_err, source.error(), disconnect);
        if !outcome.success() {
            tracing::warn!(
                request_id = %record.request_id,
                provider_id = record.member.provider_id,
                model_id = %record.member.model_id,
                fail_reason = outcome.fail_reason().unwrap_or_default(),
                "流式请求终态失败，落库并结束",
            );
        }
        let end_time = now_ms();
        RequestRecord {
            request_id: record.request_id,
            virtual_model_id: record.virtual_model_id,
            provider_id: record.member.provider_id,
            model_id: record.member.model_id.clone(),
            stream: true,
            ttft: stream_metrics.ttft_ms(),
            output_tokens_time: stream_metrics.output_duration_ms(),
            ttft_start_ms: reply_start_at,
            start_time: record.start_time,
            end_time,
            usage: source.usage().unwrap_or_default(),
            success: outcome.success(),
            fail_reason: outcome.fail_reason(),
            api_key_name: record.api_key_name,
        }
        .insert(&db);
    });
    sse_response(ReceiverStream::new(rx))
}

#[cfg(test)]
mod tests {
    use super::StreamOutcome;

    fn outcome(u: Option<&str>, c: Option<&str>, s: Option<&str>, d: bool) -> StreamOutcome {
        StreamOutcome::from_parts(
            u.map(str::to_string),
            c.map(str::to_string),
            s.map(str::to_string),
            d,
        )
    }

    /// 无错误源、无断开：成功且无原因。
    #[test]
    fn outcome_clean_stream_is_success_without_reason() {
        let outcome = outcome(None, None, None, false);
        assert!(outcome.success());
        assert_eq!(outcome.fail_reason(), None);
    }

    /// 三个错误源各自单独出现时都判失败，且原因取自身。
    #[test]
    fn outcome_each_error_source_fails_with_own_reason() {
        for (u, c, s) in [
            (Some("读上游失败"), None, None),
            (None, Some("转换失败"), None),
            (None, None, Some("带内错误")),
        ] {
            let outcome = outcome(u, c, s, false);
            assert!(!outcome.success(), "任一错误源都应判失败");
            assert!(outcome.fail_reason().is_some());
        }
    }

    /// 多源并存时原因优先级稳定：上游读错 > 转换失败 > 带内错误 > 客户端断开。
    #[test]
    fn outcome_reason_priority_is_stable() {
        assert_eq!(
            outcome(Some("读上游失败"), Some("转换失败"), Some("带内错误"), true)
                .fail_reason()
                .as_deref(),
            Some("读上游失败")
        );
        assert_eq!(
            outcome(None, Some("转换失败"), Some("带内错误"), true)
                .fail_reason()
                .as_deref(),
            Some("转换失败")
        );
        assert_eq!(
            outcome(None, None, Some("带内错误"), true)
                .fail_reason()
                .as_deref(),
            Some("带内错误")
        );
    }

    /// 客户端断开不影响成功判定，仅补记原因（entity/request.rs 文档口径）。
    #[test]
    fn outcome_disconnect_keeps_success_and_records_reason() {
        let outcome = outcome(None, None, None, true);
        assert!(outcome.success(), "客户端取消不算上游失败");
        assert_eq!(outcome.fail_reason().as_deref(), Some("客户端提前断开"));
    }

    /// 断开与上游错误并存时：判失败，原因是上游侧（断开只是附加信息）。
    #[test]
    fn outcome_disconnect_with_error_reports_error_reason() {
        let outcome = outcome(Some("读上游失败"), None, None, true);
        assert!(!outcome.success());
        assert_eq!(outcome.fail_reason().as_deref(), Some("读上游失败"));
    }
}

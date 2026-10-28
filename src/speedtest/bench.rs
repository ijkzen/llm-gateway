//! 单模型测速：流式采样 TPS。
//!
//! 与 `proxy::probe::test_model`（连通性探测，非流式、只要 2xx）刻意分开：
//! 探测关心「能不能用」，只需上游受理；测速关心「吐字有多快」，必须读到
//! 首 token 与末 token 才能算出分阶段的耗时。
//!
//! 全部复用既有转发件，不另写第五份流式解析：请求体走 `build_upstream_call`
//! （四协议统一），事件解析走 `PumpSource`（直通扫描与三协议转换的唯一实现），
//! 计时走 `StreamMetrics`，TPS 走 `RequestRecord::tps_value`——与生产流量的
//! `request.tps` 完全同口径。

use http_body_util::BodyExt;
use serde_json::json;
use uuid::Uuid;

use crate::entity::{provider, provider_model, speed_test_result};
use crate::proxy::convert::extract_error_message;
use crate::proxy::convert::openai;
use crate::proxy::metrics::{RequestRecord, StreamMetrics, now_ms};
use crate::proxy::upstream;
use crate::proxy::{
    Converter, Protocol, PumpSource, TEST_API_KEY_NAME, TEST_VIRTUAL_MODEL_ID, build_member,
    build_upstream_call, opencode_session_fallback, record_failure,
};
use crate::speedtest::{SPEED_TEST_PROMPT, SPEED_TEST_REQUEST_PREFIX, target_max_tokens};
use crate::state::AppState;

/// 单个模型的测速结果（未落库前的内存形态）。
#[derive(Debug, Clone)]
pub struct BenchOutcome {
    /// 本次请求在 `request` 表的主键（失败行也有），供页面跳转审计。
    pub request_id: String,
    pub ttft_ms: Option<i64>,
    pub output_tokens: Option<i64>,
    pub output_tokens_time_ms: Option<i64>,
    pub total_ms: Option<i64>,
    pub tps: Option<f64>,
    /// 非 None 即该行失败（非 2xx / 网络错误 / 超出预算 / 流内错误事件）。
    pub failure: Option<String>,
}

impl BenchOutcome {
    fn failed(request_id: String, total_ms: Option<i64>, message: String) -> Self {
        Self {
            request_id,
            ttft_ms: None,
            output_tokens: None,
            output_tokens_time_ms: None,
            total_ms,
            tps: None,
            failure: Some(message),
        }
    }
}

/// 按上游协议构造事件源。直通臂用旁路扫描器（不改变字节，本来就只采样），
/// 转换臂用对应转换器——与转发管线同一选择逻辑，测速结果才与真实流量可比。
fn build_source(protocol: Protocol, requested_model: &str) -> PumpSource {
    match protocol {
        Protocol::OpenAiCompat => PumpSource::OpenAi {
            scanner: openai::OpenAiStreamScanner::default(),
            include_usage: true,
        },
        Protocol::OpenAiResponses => PumpSource::Convert {
            converter: Converter::Responses(Box::new(
                crate::proxy::convert::responses::ResponsesStreamConverter::new(
                    "",
                    requested_model,
                ),
            )),
            reasoning_exclude: false,
        },
        Protocol::Anthropic => PumpSource::Convert {
            converter: Converter::Anthropic(Box::new(
                crate::proxy::convert::anthropic::AnthropicStreamConverter::new(
                    "",
                    requested_model,
                    false,
                ),
            )),
            reasoning_exclude: false,
        },
        Protocol::Gemini => PumpSource::Convert {
            converter: Converter::Gemini(Box::new(
                crate::proxy::convert::gemini::GeminiStreamConverter::new("", requested_model),
            )),
            reasoning_exclude: false,
        },
    }
}

/// 对单个供应商模型做一次长文流式测速，并把结果写入 `request` 表。
///
/// 失败不触碰可用性状态机（无 `note_member_failure`、无飞书通知）：一次批量
/// 测速不该把供应商打成连续失败禁用。失败原因截断到 200 字符，与
/// `record_failure` 同口径。
pub async fn bench_one(
    state: &AppState,
    provider_row: &provider::Model,
    model: &provider_model::Model,
) -> BenchOutcome {
    let request_id = format!("{SPEED_TEST_REQUEST_PREFIX}{}", Uuid::new_v4());
    let max_tokens = target_max_tokens(model.max_output_tokens);

    let api_key = match crate::crypto::decrypt(&provider_row.api_key) {
        Ok(key) if !key.is_empty() => key,
        Ok(_) => {
            return BenchOutcome::failed(request_id.clone(), None, "未配置 API Key".to_string());
        }
        Err(e) => {
            return BenchOutcome::failed(
                request_id.clone(),
                None,
                format!("API Key 解密失败：{e}"),
            );
        }
    };

    let member = build_member(provider_row, model);
    let chat = json!({
        "model": model.provider_model_id,
        "stream": true,
        "max_tokens": max_tokens,
        "messages": [{"role": "user", "content": SPEED_TEST_PROMPT}],
    });

    // 测速无下游请求头（管理面手动触发）：透传子集为空。
    let (call, _flags) = match build_upstream_call(
        &member,
        &chat,
        true,
        &api_key,
        &[],
        &request_id,
        &opencode_session_fallback(TEST_API_KEY_NAME),
    )
    .await
    {
        Ok(pair) => pair,
        Err(message) => return BenchOutcome::failed(request_id.clone(), None, message),
    };

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
                true,
                &message,
                start_time,
            );
            return BenchOutcome::failed(request_id, None, message);
        }
    };

    if !reply.status.is_success() {
        let status = reply.status.as_u16();
        let body = upstream::read_body(reply.body).await.unwrap_or_default();
        let message = format!(
            "{status} {}",
            extract_error_message(&String::from_utf8_lossy(&body))
        );
        record_failure(
            &state.db,
            &request_id,
            TEST_VIRTUAL_MODEL_ID,
            &member,
            TEST_API_KEY_NAME,
            start_time,
            true,
            &message,
            reply.start_at_ms,
        );
        return BenchOutcome::failed(request_id, Some(now_ms() - reply.start_at_ms), message);
    }

    // 逐帧读取 + 采样。不设单模型硬超时：模型自然生成完为止，耗时由上游客户端
    // 自身的连接/读超时兜底（`proxy::upstream` 四常量）。曾取 180 秒硬预算，
    // 但推理模型会把预算烧在思考上，硬砍只会把「能出数」变成「超时失败」。
    let mut source = build_source(member.protocol, &model.provider_model_id);
    let mut metrics = StreamMetrics::new(reply.start_at_ms);
    let mut body = reply.body;
    let mut splitter = crate::proxy::sse::SseSplitter::default();
    let mut stream_error: Option<String> = None;

    loop {
        let frame = match body.frame().await {
            Some(Ok(frame)) => frame,
            Some(Err(e)) => {
                // 已收到 [DONE] 后的读错误属连接收尾噪音（与 relay 同判定）。
                if source.saw_done() {
                    break;
                }
                stream_error = Some(format!("读取上游流失败：{e}"));
                break;
            }
            None => break,
        };
        let bytes = frame.into_data().unwrap_or_default();
        let text = String::from_utf8_lossy(&bytes).to_string();
        let mut failed = false;
        for event in splitter.feed(&text) {
            if let Err(message) = source.feed_for_metrics(&event, &mut metrics) {
                stream_error = Some(message);
                failed = true;
                break;
            }
        }
        if failed {
            break;
        }
        if source.finished() {
            break;
        }
    }

    if let Some(message) = stream_error {
        record_failure(
            &state.db,
            &request_id,
            TEST_VIRTUAL_MODEL_ID,
            &member,
            TEST_API_KEY_NAME,
            start_time,
            true,
            &message,
            reply.start_at_ms,
        );
        return BenchOutcome::failed(request_id, Some(now_ms() - reply.start_at_ms), message);
    }

    let usage = source.usage().unwrap_or_default();
    let end_time = now_ms();
    let record = RequestRecord {
        request_id: request_id.clone(),
        virtual_model_id: TEST_VIRTUAL_MODEL_ID,
        provider_id: member.provider_id,
        model_id: member.model_id.clone(),
        stream: true,
        ttft: metrics.ttft_ms(),
        output_tokens_time: metrics.output_duration_ms(),
        ttft_start_ms: reply.start_at_ms,
        start_time,
        end_time,
        usage: usage.clone(),
        success: true,
        fail_reason: None,
        api_key_name: TEST_API_KEY_NAME.to_string(),
    };
    // 同一 `tps_value` 既落 request 表也被结果行使用，两处口径不可能走散。
    let tps = record.tps_value();
    record.insert(&state.db);

    BenchOutcome {
        request_id,
        ttft_ms: metrics.ttft_ms(),
        output_tokens: usage.output_tokens,
        output_tokens_time_ms: metrics.output_duration_ms(),
        total_ms: Some((end_time - reply.start_at_ms).max(0)),
        tps: Some(tps),
        failure: None,
    }
}

/// 把内存结果转成结果行（`run_id` 与快照字段由调用方补齐）。
pub fn to_result_model(
    run_id: i32,
    provider_row: &provider::Model,
    model: &provider_model::Model,
    outcome: &BenchOutcome,
) -> speed_test_result::ActiveModel {
    use sea_orm::Set;

    let finished_at = now_ms();
    match &outcome.failure {
        Some(reason) => speed_test_result::ActiveModel {
            run_id: Set(run_id),
            provider_id: Set(provider_row.id),
            provider_name: Set(provider_row.name.clone()),
            provider_model_pk: Set(model.model_id),
            model_id: Set(model.provider_model_id.clone()),
            status: Set("failed".to_string()),
            ttft_ms: Set(None),
            output_tokens: Set(None),
            output_tokens_time_ms: Set(None),
            total_ms: Set(outcome.total_ms),
            tps: Set(None),
            request_id: Set(Some(outcome.request_id.clone())),
            fail_reason: Set(Some(truncate_fail_reason(reason))),
            finished_at: Set(Some(finished_at)),
            ..Default::default()
        },
        None => speed_test_result::ActiveModel {
            run_id: Set(run_id),
            provider_id: Set(provider_row.id),
            provider_name: Set(provider_row.name.clone()),
            provider_model_pk: Set(model.model_id),
            model_id: Set(model.provider_model_id.clone()),
            status: Set("done".to_string()),
            ttft_ms: Set(outcome.ttft_ms),
            output_tokens: Set(outcome.output_tokens),
            output_tokens_time_ms: Set(outcome.output_tokens_time_ms),
            total_ms: Set(outcome.total_ms),
            tps: Set(outcome.tps),
            request_id: Set(Some(outcome.request_id.clone())),
            fail_reason: Set(None),
            finished_at: Set(Some(finished_at)),
            ..Default::default()
        },
    }
}

/// 失败原因截断到 200 字符（`record_failure` 同口径，按字符而非字节）。
fn truncate_fail_reason(message: &str) -> String {
    const MAX_CHARS: usize = 200;
    if message.chars().count() <= MAX_CHARS {
        return message.to_string();
    }
    let truncated: String = message.chars().take(MAX_CHARS).collect();
    format!("{truncated}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fail_reason_truncates_by_chars() {
        let long = "错".repeat(300);
        let out = truncate_fail_reason(&long);
        assert_eq!(out.chars().count(), 201, "200 字符 + 省略号");
        assert!(out.ends_with('…'));

        let short = "401 Unauthorized";
        assert_eq!(truncate_fail_reason(short), short);
    }

    #[test]
    fn failed_outcome_has_no_metrics() {
        let out = BenchOutcome::failed("speedtest-x".into(), Some(12), "boom".into());
        assert!(out.tps.is_none());
        assert!(out.ttft_ms.is_none());
        assert_eq!(out.failure.as_deref(), Some("boom"));
    }
}

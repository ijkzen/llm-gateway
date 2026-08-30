//! request 表记录与流式指标追踪。

use std::time::Instant;

use sea_orm::{ActiveModelTrait, DatabaseConnection, Set};

use crate::entity::request::ActiveModel;

/// 归一化 usage（口径见 `entity::request` 文档注释）。
#[derive(Debug, Default, Clone)]
pub struct Usage {
    /// 输入 token（含缓存命中部分）；上游未返回 usage 时为 None。
    pub input_tokens: Option<i64>,
    /// 缓存命中 token（OpenAI cached_tokens / Anthropic cache_read+cache_creation /
    /// Gemini cachedContentTokenCount）。
    pub cache_tokens: i64,
    /// 输出 token（含推理/思考 token）；usage 缺失时为 None。
    pub output_tokens: Option<i64>,
}

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// 流式响应的指标追踪（首/末 token 时刻）。
#[derive(Debug, Default)]
pub struct StreamMetrics {
    /// TTFT 起点（wall-clock）：新建连接=建连开始，复用连接=请求发出。
    connect_done_at: Option<i64>,
    first_token_at: Option<i64>,
    last_token_at: Option<i64>,
}

impl StreamMetrics {
    pub fn new(start_at_ms: i64) -> Self {
        Self {
            connect_done_at: Some(start_at_ms),
            ..Default::default()
        }
    }

    /// 首个/最新内容 token 时刻（收到含内容的数据块时调用）。
    pub fn on_token(&mut self) {
        let now = now_ms();
        if self.first_token_at.is_none() {
            self.first_token_at = Some(now);
        }
        self.last_token_at = Some(now);
    }

    /// 上游响应头到达时刻到现在的耗时（非流式的输出阶段耗时口径）。
    pub fn non_stream_output_ms(&self, headers_at_ms: i64, body_done_ms: i64) -> i64 {
        (body_done_ms - headers_at_ms).max(0)
    }

    /// 首 token 耗时（ttft，毫秒）：TTFT 起点 → 首个内容 token。
    /// 覆盖建连（新建连接）或请求发出（复用连接）到首个内容块的全程。
    /// 未收到内容 token 时为 None。
    pub fn ttft_ms(&self) -> Option<i64> {
        match (self.connect_done_at, self.first_token_at) {
            (Some(start), Some(first)) => Some((first - start).max(0)),
            _ => None,
        }
    }

    /// 输出阶段耗时（毫秒）：末 token − 首 token。
    pub fn output_duration_ms(&self) -> Option<i64> {
        match (self.first_token_at, self.last_token_at) {
            (Some(first), Some(last)) => Some((last - first).max(0)),
            _ => None,
        }
    }
}

/// 一次请求的最终记录，由转发管线在响应完成后填充并落库。
#[derive(Debug, Clone)]
pub struct RequestRecord {
    pub request_id: String,
    pub virtual_model_id: i32,
    pub provider_id: i32,
    pub model_id: String,
    pub stream: bool,
    pub ttft: Option<i64>,
    pub output_tokens_time: Option<i64>,
    /// tps 分母起点（wall-clock 毫秒）：流式为 TTFT 起点（= ttft 的起点），
    /// 非流式为请求发出时刻（无 TTFT 时借用）。均不含网关路由/LB/构造/DNS。
    pub ttft_start_ms: i64,
    pub start_time: i64,
    pub end_time: i64,
    pub usage: Usage,
    pub success: bool,
    pub fail_reason: Option<String>,
    pub api_key_name: String,
}

impl RequestRecord {
    /// tps = output_tokens / 网络阶段总耗时（秒）。流式分母 = ttft + 输出耗时
    /// （末 token − TTFT 起点，起点=建连开始或请求发出）；非流式分母 =
    /// end_time − ttft_start_ms（请求发出 → 读完）。两者均不含网关前置，
    /// 含建连耗时与上游处理耗时；分母≤0 或无输出 token 记 0。
    pub fn tps_value(&self) -> f64 {
        if self.stream {
            match (self.usage.output_tokens, self.ttft, self.output_tokens_time) {
                (Some(output), Some(ttft_ms), Some(output_ms)) => {
                    let denominator_ms = ttft_ms + output_ms;
                    if denominator_ms > 0 {
                        output as f64 / (denominator_ms as f64 / 1000.0)
                    } else {
                        0.0
                    }
                }
                // 流式但缺首/末 token（异常流），无法计算。
                _ => 0.0,
            }
        } else {
            match self.usage.output_tokens {
                Some(output) => {
                    let denominator_ms = self.end_time - self.ttft_start_ms;
                    if denominator_ms > 0 {
                        output as f64 / (denominator_ms as f64 / 1000.0)
                    } else {
                        0.0
                    }
                }
                None => 0.0,
            }
        }
    }

    /// 计算派生字段（cache_rate / tps / total_tokens）并异步落库。
    pub fn insert(self, db: &DatabaseConnection) {
        let input_cache_rate = match self.usage.input_tokens {
            // 保存到小数点后 5 位（如 0.99789），避免浮点长尾与展示端误舍入。
            Some(input) if input > 0 => {
                let rate = self.usage.cache_tokens as f64 / input as f64;
                (rate * 100_000.0).round() / 100_000.0
            }
            _ => 0.0,
        };
        let tps = self.tps_value();
        let total_tokens = match (self.usage.input_tokens, self.usage.output_tokens) {
            (Some(input), Some(output)) => Some(input + output),
            _ => None,
        };
        let active = ActiveModel {
            request_id: Set(self.request_id),
            virtual_model_id: Set(self.virtual_model_id),
            provider_id: Set(self.provider_id),
            model_id: Set(self.model_id),
            stream: Set(self.stream),
            ttft: Set(self.ttft),
            input_tokens: Set(self.usage.input_tokens),
            input_cache_tokens: Set(self.usage.cache_tokens),
            input_cache_rate: Set(input_cache_rate),
            output_tokens: Set(self.usage.output_tokens),
            output_tokens_time: Set(self.output_tokens_time),
            tps: Set(tps),
            start_time: Set(self.start_time),
            end_time: Set(self.end_time),
            request_time: Set((self.end_time - self.start_time).max(0)),
            success: Set(self.success),
            fail_reason: Set(self.fail_reason),
            total_tokens: Set(total_tokens),
            api_key_name: Set(self.api_key_name),
        };
        let db = db.clone();
        tokio::spawn(async move {
            if let Err(e) = active.insert(&db).await {
                tracing::warn!("Failed to insert request record: {e}");
            }
        });
    }
}

/// 上游请求发起时刻的计时起点（Instant，用于流式统计）。
#[derive(Debug, Clone)]
pub struct Stopwatch {
    started: Instant,
}

impl Default for Stopwatch {
    fn default() -> Self {
        Self::new()
    }
}

impl Stopwatch {
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
        }
    }

    pub fn elapsed_ms(&self) -> i64 {
        i64::try_from(self.started.elapsed().as_millis()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一个最小 RequestRecord（派生字段计算只看 tps 相关字段）。
    fn record(
        stream: bool,
        output: Option<i64>,
        ttft: Option<i64>,
        output_ms: Option<i64>,
        start: i64,
        end: i64,
    ) -> RequestRecord {
        RequestRecord {
            request_id: "r".into(),
            virtual_model_id: 1,
            provider_id: 1,
            model_id: "m".into(),
            stream,
            ttft,
            output_tokens_time: output_ms,
            ttft_start_ms: start,
            start_time: start,
            end_time: end,
            usage: Usage {
                input_tokens: None,
                cache_tokens: 0,
                output_tokens: output,
            },
            success: true,
            fail_reason: None,
            api_key_name: "k".into(),
        }
    }

    #[test]
    fn tps_stream_uses_ttft_plus_output_duration() {
        // 流式：分母 = ttft(300) + 输出耗时(200) = 500ms → 100 / 0.5 = 200。
        let r = record(true, Some(100), Some(300), Some(200), 0, 0);
        assert_eq!(r.tps_value(), 200.0);
    }

    #[test]
    fn tps_non_stream_uses_end_minus_start() {
        // 非流式：分母 = end(5000) − 请求发出(4000) = 1000ms → 50 / 1.0 = 50。
        let r = record(false, Some(50), None, None, 4000, 5000);
        assert_eq!(r.tps_value(), 50.0);
    }

    #[test]
    fn tps_zero_when_denominator_invalid() {
        // 流式分母为 0 → 0。
        let r = record(true, Some(100), Some(0), Some(0), 0, 0);
        assert_eq!(r.tps_value(), 0.0);
        // 非流式分母 ≤ 0 → 0。
        let r = record(false, Some(50), None, None, 5000, 4000);
        assert_eq!(r.tps_value(), 0.0);
        // 无输出 token → 0。
        let r = record(false, None, None, None, 0, 1000);
        assert_eq!(r.tps_value(), 0.0);
    }
}

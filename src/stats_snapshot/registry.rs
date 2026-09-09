//! 指标与主体注册表（单一事实源）：生成端 SQL 与读端取数共用同一套名字/分类，
//! 避免口径漂移。各指标的口径谓词与实时端点 SQL 逐条对账（ADR-0021、spec §指标注册表）。

/// 主体类型（entity_type 列）。whole 的 entity 为空串。
pub(crate) const ENTITY_WHOLE: &str = "whole";
pub(crate) const ENTITY_PROVIDER: &str = "provider";
pub(crate) const ENTITY_MODEL: &str = "model";
pub(crate) const ENTITY_VIRTUAL_MODEL: &str = "virtual_model";
pub(crate) const ENTITY_API_KEY: &str = "api_key";
/// 交叉主体：虚拟模型成员（entity = "vmId,providerModelId"）。
pub(crate) const ENTITY_VM_MEMBER: &str = "virtual_model_member";
/// 交叉主体：API Key × 供应商模型（entity = "apiKeyId,providerModelId"）。
pub(crate) const ENTITY_API_KEY_MODEL: &str = "api_key_model";

pub(crate) const ALL_ENTITY_TYPES: [&str; 7] = [
    ENTITY_WHOLE,
    ENTITY_PROVIDER,
    ENTITY_MODEL,
    ENTITY_VIRTUAL_MODEL,
    ENTITY_API_KEY,
    ENTITY_VM_MEMBER,
    ENTITY_API_KEY_MODEL,
];

/// 指标种类：可加和原语（跨桶加总后再算比率/均值）或闭桶标量（分位，逐桶使用
/// 不跨桶加总）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MetricKind {
    /// 加和/计数原语（整数列以 REAL 存储，2^53 内无损）。
    Additive,
    /// 闭桶时对该桶原始值精确计算的分位标量（仅 hour/day 行）。
    Percentile,
}

/// 指标值常量。命名约定：无后缀 = 成功请求全集（success = 1）；`_all` 后缀 =
/// 全量请求全集（成功+失败），与实时端点 SQL 谓词一一对应。
pub(crate) mod metrics {
    // 全量全集（summary / charts 趋势与分布 / insight 失败与吞吐）。
    pub(crate) const CALLS: &str = "calls"; // COUNT(*)
    pub(crate) const FAIL_CALLS: &str = "fail_calls"; // SUM(success=0)
    pub(crate) const STREAM_CALLS: &str = "stream_calls"; // SUM(stream)
    pub(crate) const TOKENS_ALL: &str = "tokens_all"; // SUM(total_tokens)
    pub(crate) const INPUT_TOKENS_ALL: &str = "input_tokens_all"; // summary 口径 SUM(input_tokens)
    pub(crate) const CACHE_TOKENS_ALL: &str = "cache_tokens_all"; // summary 口径 SUM(input_cache_tokens)
    // 成功全集（rank/metrics 六指标、insight token/延迟、charts token 不走此集）。
    pub(crate) const SUCCESS_CALLS: &str = "success_calls"; // COUNT(success=1)
    pub(crate) const TOTAL_TOKENS: &str = "total_tokens";
    pub(crate) const INPUT_TOKENS: &str = "input_tokens";
    pub(crate) const CACHE_TOKENS: &str = "cache_tokens"; // input_cache_tokens
    pub(crate) const OUTPUT_TOKENS: &str = "output_tokens";
    /// Σ ttft（AVG 分子；ttft 为 NULL 的行 SUM/COUNT 双跳过，与 AVG 一致）。
    pub(crate) const TTFT_SUM: &str = "ttft_sum";
    /// ttft 非空行数（AVG 分母）。
    pub(crate) const TTFT_N: &str = "ttft_n";
    /// Σ request_time（成功全集；request_time 恒非空）。
    pub(crate) const REQUEST_TIME_SUM: &str = "request_time_sum";
    /// TPS 分母：tps>0 ∧ output_tokens>0 行的 Σ output_tokens / tps（毫秒）；
    /// 分子 = OUTPUT_TOKENS（tps_sql 分子即成功全集 Σ output_tokens），无独立键。
    pub(crate) const TPS_TIME_SUM: &str = "tps_time_sum";
    /// insight 输出速率：成功行 output_tokens_time>0 时 Σ output_tokens/(output_tokens_time/1000)。
    pub(crate) const OUT_SEC_SUM: &str = "out_sec_sum";

    // 闭桶分位标量（Percentile，hour/day 行；month/year 不存，接口语义为空）。
    pub(crate) const TTFT_P50: &str = "ttft_p50";
    pub(crate) const TTFT_P90: &str = "ttft_p90";
    pub(crate) const TTFT_P95: &str = "ttft_p95";
    pub(crate) const TTFT_P99: &str = "ttft_p99";
    pub(crate) const REQUEST_TIME_P50: &str = "request_time_p50";
    pub(crate) const REQUEST_TIME_P90: &str = "request_time_p90";
    pub(crate) const REQUEST_TIME_P95: &str = "request_time_p95";
    pub(crate) const REQUEST_TIME_P99: &str = "request_time_p99";
}

/// 该粒度是否存分位标量（hour/day 存，month/year 不存——接口现语义月/年分位为空）。
pub(crate) fn percentile_level_ok(level: super::Level) -> bool {
    matches!(level, super::Level::Hour | super::Level::Day)
}

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

// 指标种类说明：METRICS 表为可加和原语（跨桶加总后再算比率/均值；整数列以
// REAL 存储，2^53 内无损）；分位标量（metrics::*_P50/90/95/99）为闭桶时对
// 该桶原始值精确计算的标量，仅 hour/day 行，逐桶使用不跨桶加总。

/// 可加和指标的集合归属。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MetricSet {
    /// 全量全集（表达式无 success 条件）。
    All,
    /// 成功全集（CASE WHEN success = 1 条件求和，与实时端点「WHERE success = 1
    /// 后聚合」严格等价），但不在九原语读路径契约内。
    Success,
    /// 成功全集且属九原语契约（rank/metrics 读路径；表内顺序即契约顺序）。
    SuccessPrim,
}

/// 可加和指标规格（读写两侧唯一事实源）：键 + SQL 表达式 + 集合归属。
/// 生成端按全表单遍扫描固化；读侧按声明键序列取表达式拼 SELECT——
/// 谓词文本只此一份，不再各端点手抄。
pub(crate) struct MetricSpec {
    pub(crate) key: &'static str,
    pub(crate) expr: &'static str,
    pub(crate) set: MetricSet,
}

/// 全部 16 个可加和指标（表序即生成端 SELECT 列序；九原语段顺序即
/// rank_snap::Prims 下标契约——success_calls 起连续 9 个 SuccessPrim）。
pub(crate) const METRICS: [MetricSpec; 16] = [
    // 全量全集
    MetricSpec {
        key: metrics::CALLS,
        expr: "COUNT(*)",
        set: MetricSet::All,
    },
    MetricSpec {
        key: metrics::FAIL_CALLS,
        expr: "SUM(CASE WHEN r.success = 0 THEN 1 ELSE 0 END)",
        set: MetricSet::All,
    },
    MetricSpec {
        key: metrics::STREAM_CALLS,
        expr: "SUM(CASE WHEN r.stream THEN 1 ELSE 0 END)",
        set: MetricSet::All,
    },
    MetricSpec {
        key: metrics::TOKENS_ALL,
        expr: "SUM(r.total_tokens)",
        set: MetricSet::All,
    },
    MetricSpec {
        key: metrics::INPUT_TOKENS_ALL,
        expr: "SUM(r.input_tokens)",
        set: MetricSet::All,
    },
    MetricSpec {
        key: metrics::CACHE_TOKENS_ALL,
        expr: "SUM(r.input_cache_tokens)",
        set: MetricSet::All,
    },
    // 九原语段（SuccessPrim，顺序契约：success_calls 起连续 9 个）
    MetricSpec {
        key: metrics::SUCCESS_CALLS,
        expr: "SUM(CASE WHEN r.success = 1 THEN 1 ELSE 0 END)",
        set: MetricSet::SuccessPrim,
    },
    MetricSpec {
        key: metrics::TOTAL_TOKENS,
        expr: "SUM(CASE WHEN r.success = 1 THEN r.total_tokens END)",
        set: MetricSet::SuccessPrim,
    },
    MetricSpec {
        key: metrics::INPUT_TOKENS,
        expr: "SUM(CASE WHEN r.success = 1 THEN r.input_tokens END)",
        set: MetricSet::SuccessPrim,
    },
    MetricSpec {
        key: metrics::CACHE_TOKENS,
        expr: "SUM(CASE WHEN r.success = 1 THEN r.input_cache_tokens END)",
        set: MetricSet::SuccessPrim,
    },
    MetricSpec {
        key: metrics::OUTPUT_TOKENS,
        expr: "SUM(CASE WHEN r.success = 1 THEN r.output_tokens END)",
        set: MetricSet::SuccessPrim,
    },
    MetricSpec {
        key: metrics::TTFT_SUM,
        expr: "SUM(CASE WHEN r.success = 1 THEN r.ttft END)",
        set: MetricSet::SuccessPrim,
    },
    MetricSpec {
        key: metrics::TTFT_N,
        expr: "SUM(CASE WHEN r.success = 1 AND r.ttft IS NOT NULL THEN 1 ELSE 0 END)",
        set: MetricSet::SuccessPrim,
    },
    MetricSpec {
        key: metrics::REQUEST_TIME_SUM,
        expr: "SUM(CASE WHEN r.success = 1 THEN r.request_time END)",
        set: MetricSet::SuccessPrim,
    },
    MetricSpec {
        key: metrics::TPS_TIME_SUM,
        expr: "SUM(CASE WHEN r.success = 1 AND r.tps > 0 AND r.output_tokens > 0 \
                      THEN r.output_tokens / r.tps ELSE 0 END)",
        set: MetricSet::SuccessPrim,
    },
    // 成功全集（非九原语）
    MetricSpec {
        key: metrics::OUT_SEC_SUM,
        expr: "SUM(CASE WHEN r.success = 1 AND r.output_tokens_time > 0 \
                      THEN r.output_tokens / (r.output_tokens_time / 1000.0) ELSE 0 END)",
        set: MetricSet::Success,
    },
];

/// 全部可加和指标 (键, 表达式)：生成端单遍扫描固化用（表序即列序）。
pub(crate) fn metric_exprs() -> impl Iterator<Item = (&'static str, &'static str)> {
    METRICS.iter().map(|s| (s.key, s.expr))
}

/// 九原语 (键, 表达式)：rank/metrics 读路径契约（顺序固定，与 Prims 下标一致）。
pub(crate) fn success_prims() -> impl Iterator<Item = (&'static str, &'static str)> {
    METRICS
        .iter()
        .filter(|s| matches!(s.set, MetricSet::SuccessPrim))
        .map(|s| (s.key, s.expr))
}

/// 按键查表达式（键均为 metrics 模块常量，查不到即程序错误）。
pub(crate) fn expr_of(key: &str) -> &'static str {
    METRICS
        .iter()
        .find(|s| s.key == key)
        .map(|s| s.expr)
        .unwrap_or_else(|| panic!("未注册的指标键: {key}"))
}

/// 按键序列拼 SELECT 列（"{expr} AS {key}"）：读侧 trend/series/summary SQL 用。
pub(crate) fn select_list(keys: &[&str]) -> String {
    keys.iter()
        .map(|k| format!("{} AS {k}", expr_of(k)))
        .collect::<Vec<_>>()
        .join(", ")
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

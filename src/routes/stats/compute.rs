use super::*;

/// 分位值（0~1）：升序样本的线性插值，与业界 P95 口径一致（N·p 位置插值）。
/// 空样本返回 0.0。
pub(crate) fn percentile(sorted_values: &[f64], p: f64) -> f64 {
    let n = sorted_values.len();
    if n == 0 {
        return 0.0;
    }
    if n == 1 {
        return sorted_values[0];
    }
    let rank = p * (n - 1) as f64;
    let lo = rank.floor() as usize;
    let hi = rank.ceil() as usize;
    if lo == hi {
        sorted_values[lo]
    } else {
        let weight = rank - lo as f64;
        sorted_values[lo] * (1.0 - weight) + sorted_values[hi] * weight
    }
}

/// 保留 5 位小数（0.123456 → 0.12346），与 SQL 侧 ROUND(…, 5) 一致。
pub(crate) fn round_5(value: f64) -> f64 {
    (value * 100_000.0).round() / 100_000.0
}

/// 加权比率：part / total，统一保留 5 位小数；total 为 0 时记 0。
/// 与 SQL 侧 cache_hit_rate_sql 的口径（ROUND(…, 5)）保持一致，
/// 供 summary 等 Rust 层聚合复用，避免与 SQL 侧口径漂移。
pub(crate) fn weighted_ratio(part: f64, total: f64) -> f64 {
    if total <= 0.0 {
        return 0.0;
    }
    round_5(part / total)
}

/// 解析必填时间窗口 [start, end)：缺失或 end <= start 返回双语错误文案。
/// rank 与四个 metrics 端点共用，避免各自手抄同一段校验。
pub(crate) fn required_time_range(
    start_time: Option<i64>,
    end_time: Option<i64>,
) -> Result<(i64, i64), &'static str> {
    let (Some(start), Some(end)) = (start_time, end_time) else {
        return Err(AppSettings::lang_sync().tr(
            "缺少 startTime / endTime 参数",
            "missing startTime / endTime parameters",
        ));
    };
    if end <= start {
        return Err(AppSettings::lang_sync().tr(
            "endTime 必须大于 startTime",
            "endTime must be greater than startTime",
        ));
    }
    Ok((start, end))
}

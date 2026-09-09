//! 共享数值口径：与 stats 计算层同源（routes/stats/compute.rs 委托到这里，
//! 避免两份分位/舍入实现漂移）。

/// 分位值（0~1）：升序样本的线性插值（N·p 位置插值，业界 P95 口径）。
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
pub(crate) fn weighted_ratio(part: f64, total: f64) -> f64 {
    if total <= 0.0 {
        return 0.0;
    }
    round_5(part / total)
}

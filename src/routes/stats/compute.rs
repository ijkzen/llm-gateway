use super::*;

// 数值口径统一收敛在 stats_snapshot::math（分位/5 位舍入/加权比率），
// 此处为 stats 层的同源委托，避免第二份实现漂移。
pub(crate) fn percentile(sorted_values: &[f64], p: f64) -> f64 {
    crate::stats_snapshot::percentile(sorted_values, p)
}

pub(crate) fn round_5(value: f64) -> f64 {
    crate::stats_snapshot::round_5(value)
}

pub(crate) fn weighted_ratio(part: f64, total: f64) -> f64 {
    crate::stats_snapshot::weighted_ratio(part, total)
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

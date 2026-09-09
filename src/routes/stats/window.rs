use super::*;

pub(crate) const HOUR_MS: i64 = 3_600_000;
pub(crate) const DAY_MS: i64 = 24 * HOUR_MS;
pub(crate) const TREND_BUCKETS: i64 = 24;

/// 图表趋势桶粒度（显式指定；缺省时按窗口长度回退推断）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Granularity {
    Hour,
    Day,
    Month,
    Year,
}

impl Granularity {
    pub(crate) fn parse(value: Option<&str>) -> Result<Option<Self>, &'static str> {
        match value {
            None => Ok(None),
            Some("hour") => Ok(Some(Self::Hour)),
            Some("day") => Ok(Some(Self::Day)),
            Some("month") => Ok(Some(Self::Month)),
            Some("year") => Ok(Some(Self::Year)),
            Some(_) => Err(AppSettings::lang_sync().tr(
                "不支持的 granularity，可选 hour/day/month/year",
                "unsupported granularity; choose hour/day/month/year",
            )),
        }
    }
}

/// 某时刻在指定 IANA 时区下的固定偏移（分钟）。偏移只求一次并按窗口起点
/// 定桶：跨 DST 切换的窗口沿用起点偏移（与既有固定偏移模型一致；默认
/// Asia/Shanghai 无 DST，管理后台为单一时区视角）。
pub(crate) fn tz_offset_minutes_at(tz: chrono_tz::Tz, at_ms: i64) -> i32 {
    let Some(dt) = chrono::DateTime::from_timestamp_millis(at_ms) else {
        return 0;
    };
    tz.offset_from_utc_datetime(&dt.naive_utc())
        .fix()
        .local_minus_utc()
        / 60
}

/// 统计口径的固定时区偏移（分钟）：读设置表 timezone（缺省 Asia/Shanghai，
/// 与用量口径同一来源 `timezone_sync`），按窗口起点时刻求该时区偏移；客户端
/// 不再提供 tzOffsetMinutes。
pub(crate) fn stats_tz_offset_minutes(window_start_hint: Option<i64>) -> i32 {
    let at = window_start_hint
        .filter(|v| *v > 0)
        .unwrap_or_else(|| chrono::Utc::now().timestamp_millis());
    tz_offset_minutes_at(crate::app_settings::timezone_sync(), at)
}

/// 解析后的图表窗口：时间区间 + 桶粒度 + 时区偏移（设置表时区在窗口起点的偏移）。
#[derive(Clone, Copy, Debug)]
pub(crate) struct ChartWindow {
    pub(crate) start: i64,
    pub(crate) end: i64,
    pub(crate) bucket_ms: i64,
    pub(crate) granularity: Granularity,
    pub(crate) tz_offset_minutes: i32,
}

impl ChartWindow {
    /// 桶起点表达式（SQL 侧）：把请求时间按客户端时区对齐到桶边界。
    pub(crate) fn bucket_expr(&self) -> String {
        let offset_ms = i64::from(self.tz_offset_minutes) * 60_000;
        if matches!(self.granularity, Granularity::Month | Granularity::Year) {
            format!("(r.start_time + {offset_ms}) / {DAY_MS}")
        } else {
            format!("(r.start_time + {offset_ms}) / {}", self.bucket_ms)
        }
    }

    /// 桶起点（毫秒时间戳）：与 bucket_expr 严格互逆（bucket_ms * 索引 - offset_ms）。
    pub(crate) fn bucket_start_ms(&self, bucket: i64) -> i64 {
        let offset_ms = i64::from(self.tz_offset_minutes) * 60_000;
        bucket * self.bucket_ms - offset_ms
    }

    /// 桶索引区间（含两端）：小时/天桶补零用。tz 偏移并入（桶对齐本地边界），
    /// end 为开区间故末桶取 end-1；窗口内恒至少一个桶（start 桶兜底）。
    pub(crate) fn bucket_range(&self) -> std::ops::RangeInclusive<i64> {
        let offset_ms = i64::from(self.tz_offset_minutes) * 60_000;
        let first = (self.start + offset_ms) / self.bucket_ms;
        let last = ((self.end - 1 + offset_ms).max(self.start + offset_ms)) / self.bucket_ms;
        first..=last
    }
}

/// 解析图表窗口参数（与 charts 端点同一套缺省/回退规则）：
/// 显式 granularity 优先；缺省按窗口长度推断桶粒度（≤48h 小时、≤62d 天、其余 30 天块）。
/// 无 startTime/endTime 时回退过去 24 小时（小时桶）。
pub(crate) fn resolve_chart_window(
    start_time: Option<i64>,
    end_time: Option<i64>,
    granularity: Option<Granularity>,
    tz_offset_minutes: i32,
) -> ChartWindow {
    let (start, end, bucket_ms, granularity) = match granularity {
        Some(g) => {
            let bucket_ms = match g {
                Granularity::Hour => HOUR_MS,
                Granularity::Day => DAY_MS,
                Granularity::Month | Granularity::Year => DAY_MS,
            };
            let (start, end) = match (start_time, end_time) {
                (Some(start), Some(end)) if end > start => (start, end),
                _ => {
                    // 缺省窗口：过去 24 小时（小时桶）。
                    let now_ms = chrono::Utc::now().timestamp_millis();
                    let current_bucket = now_ms / HOUR_MS;
                    let first_bucket = current_bucket - (TREND_BUCKETS - 1);
                    (first_bucket * HOUR_MS, now_ms)
                }
            };
            (start, end, bucket_ms, g)
        }
        None => {
            let (start, end, bucket_ms) = match (start_time, end_time) {
                (Some(start), Some(end)) if end > start => {
                    // 显式窗口：按长度选桶粒度（小时/天/月）。
                    let bucket = if end - start <= 48 * HOUR_MS {
                        HOUR_MS
                    } else if end - start <= 62 * DAY_MS {
                        DAY_MS
                    } else {
                        30 * DAY_MS
                    };
                    (start, end, bucket)
                }
                _ => {
                    // 缺省：过去 24 小时（小时桶）。
                    let now_ms = chrono::Utc::now().timestamp_millis();
                    let current_bucket = now_ms / HOUR_MS;
                    let first_bucket = current_bucket - (TREND_BUCKETS - 1);
                    (first_bucket * HOUR_MS, now_ms, HOUR_MS)
                }
            };
            (start, end, bucket_ms, Granularity::Hour)
        }
    };
    ChartWindow {
        start,
        end,
        bucket_ms,
        granularity,
        tz_offset_minutes,
    }
}

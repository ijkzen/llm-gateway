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

#[allow(dead_code)] // 仅 stats 单测直测（端点已改用快照归并/本地历法迭代）
/// 月/年桶：把窗口内每个「本地日索引」归并到自然月/年（键 year 或 (year, month)），
/// 对窗口首日所在月/年到末日所在月/年补零，输出按桶起点（该月/年 1 日 0 点，本地）排序。
///
/// 返回 (桶起点列表, call 值序列, token 值序列)，三组长度一致；两序列共享同一组桶起点。
pub(crate) fn merge_natural_periods(
    call_day_indexes: &[(i64, i64)],
    token_day_indexes: &[(i64, i64)],
    window_start: i64,
    window_end: i64,
    tz: chrono::FixedOffset,
    month_mode: bool,
) -> (Vec<i64>, Vec<i64>, Vec<i64>) {
    // 本地日索引 = (ts + offset) / DAY_MS；本地 0 点毫秒 = index * DAY_MS - offset_ms。
    // 注意 local_minus_utc() 返回秒（+08:00 → 28800），需 ×1000 转毫秒。
    let offset_ms = i64::from(tz.local_minus_utc()) * 1000;
    let mut call_map: std::collections::BTreeMap<(i32, u32), i64> =
        std::collections::BTreeMap::new();
    let mut token_map: std::collections::BTreeMap<(i32, u32), i64> =
        std::collections::BTreeMap::new();
    let collect = |index: i64| -> Option<(i32, u32)> {
        let day_ms = index * DAY_MS - offset_ms;
        chrono::DateTime::from_timestamp_millis(day_ms)
            .map(|t| t.with_timezone(&tz))
            .map(|local| {
                if month_mode {
                    (local.year(), local.month())
                } else {
                    (local.year(), 0)
                }
            })
    };
    for &(index, value) in call_day_indexes {
        if let Some(key) = collect(index) {
            *call_map.entry(key).or_insert(0) += value;
        }
    }
    for &(index, value) in token_day_indexes {
        if let Some(key) = collect(index) {
            *token_map.entry(key).or_insert(0) += value;
        }
    }

    // 补零：窗口首日/末日所在月（年）及其间的全部自然月（年）。
    let first_local =
        chrono::DateTime::from_timestamp_millis(window_start).map(|t| t.with_timezone(&tz));
    let last_local =
        chrono::DateTime::from_timestamp_millis(window_end - 1).map(|t| t.with_timezone(&tz));
    if let (Some(first), Some(last)) = (first_local, last_local) {
        if month_mode {
            let mut y = first.year();
            let mut m = first.month();
            let (ly, lm) = (last.year(), last.month());
            loop {
                call_map.entry((y, m)).or_insert(0);
                token_map.entry((y, m)).or_insert(0);
                if (y, m) == (ly, lm) {
                    break;
                }
                m += 1;
                if m > 12 {
                    m = 1;
                    y += 1;
                }
            }
        } else {
            for y in first.year()..=last.year() {
                call_map.entry((y, 0)).or_insert(0);
                token_map.entry((y, 0)).or_insert(0);
            }
        }
    }

    // 桶起点 = 该月/年 1 日 0 点（本地）。
    let to_bucket_start = |(y, m): (i32, u32)| -> Option<i64> {
        let (by, bm) = if month_mode { (y, m) } else { (y, 1) };
        chrono::NaiveDate::from_ymd_opt(by, bm, 1)
            .and_then(|d| d.and_hms_opt(0, 0, 0))
            .and_then(|dt| dt.and_local_timezone(tz).single())
            .map(|dt| dt.timestamp_millis())
    };
    let mut starts = Vec::with_capacity(call_map.len());
    let mut calls = Vec::with_capacity(call_map.len());
    let mut tokens = Vec::with_capacity(call_map.len());
    for (key, call_value) in call_map {
        if let Some(start_ms) = to_bucket_start(key) {
            let token_value = token_map.get(&key).copied().unwrap_or(0);
            starts.push(start_ms);
            calls.push(call_value);
            tokens.push(token_value);
        }
    }
    (starts, calls, tokens)
}

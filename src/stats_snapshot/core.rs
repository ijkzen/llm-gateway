//! 桶帧与窗口分解（纯函数，无 DB）：
//! 桶对齐 = 与统计读取端一致的固定偏移模型（ADR-0015：设置表时区在参考时刻的
//! 偏移一次性定桶；默认 Asia/Shanghai 无 DST）。小时/天为 epoch 定长帧，
//! 月/年为本地自然历法帧（跨年/闰月由 chrono 处理）。

use chrono::{Datelike, NaiveDate};

pub(crate) const HOUR_MS: i64 = 3_600_000;
pub(crate) const DAY_MS: i64 = 24 * HOUR_MS;
/// 固化余量：桶终点再往后 60 分钟才算闭桶，闭桶前读路径对该桶保持实时（ADR-0021）。
pub(crate) const MARGIN_MS: i64 = 60 * 60_000;

/// 快照时间粒度（与快照表 duration_type 一一对应）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Hour,
    Day,
    Month,
    Year,
}

impl Level {
    pub fn key(self) -> &'static str {
        match self {
            Level::Hour => "hour",
            Level::Day => "day",
            Level::Month => "month",
            Level::Year => "year",
        }
    }

    /// 下一级更细粒度（分解边缘/未闭帧时下钻用）；Hour 为最细。
    pub(crate) fn finer(self) -> Option<Level> {
        match self {
            Level::Hour => None,
            Level::Day => Some(Level::Hour),
            Level::Month => Some(Level::Day),
            Level::Year => Some(Level::Month),
        }
    }
}

/// 一个桶帧：[start, end) 半开区间（毫秒），边界按 level 对齐。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    pub level: Level,
    pub start: i64,
    pub end: i64,
}

/// 闭桶判定：桶终点 + 固化余量已过（桶的请求行集合此后不再增长）。
pub(crate) fn bucket_closed(frame_end: i64, now_ms: i64, margin_ms: i64) -> bool {
    now_ms >= frame_end + margin_ms
}

fn offset_ms(offset_minutes: i32) -> i64 {
    i64::from(offset_minutes) * 60_000
}

/// 本地历法周期起点（epoch ms）：naive(y, m, 1) 0 点 − offset（与快照帧同一算术）。
pub(crate) fn period_start_ms(y: i32, m: u32, off_ms: i64) -> Option<i64> {
    NaiveDate::from_ymd_opt(y, m, 1)
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|dt| dt.and_utc().timestamp_millis() - off_ms)
}

/// 覆盖窗口 [start, end) 首末自然历法周期（month/year）的列表：
/// 元素 (year, month, 周期起点 ms)，按时间升序；Year 粒度 month 恒为 1（历法真值，
/// 调用方的 map 键惯例 (y, 0) 由调用方自折）。不可表示的日期跳过不产出。
pub(crate) fn natural_periods(
    level: Level,
    off_ms: i64,
    start: i64,
    end: i64,
) -> Vec<(i32, u32, i64)> {
    debug_assert!(matches!(level, Level::Month | Level::Year));
    let wall = |ts: i64| chrono::DateTime::from_timestamp_millis(ts + off_ms);
    let (Some(first), Some(last)) = (wall(start), wall(end - 1)) else {
        return Vec::new();
    };
    let month_mode = matches!(level, Level::Month);
    let mut y = first.year();
    let mut m = if month_mode { first.month() } else { 1 };
    let (ly, lm) = (last.year(), if month_mode { last.month() } else { 1 });
    let mut out = Vec::new();
    loop {
        if let Some(start_ms) = period_start_ms(y, m, off_ms) {
            out.push((y, m, start_ms));
        }
        if (y, m) == (ly, lm) {
            break;
        }
        if month_mode {
            m += 1;
            if m > 12 {
                m = 1;
                y += 1;
            }
        } else {
            y += 1;
        }
    }
    out
}

/// 时刻归属的本地历法周期键：Month → (year, month)，Year → (year, 0)
///（map 键惯例）；hour/day 无历法周期，返回 None。
pub(crate) fn period_key_of_ts(ts: i64, off_ms: i64, level: Level) -> Option<(i32, u32)> {
    let wall = chrono::DateTime::from_timestamp_millis(ts + off_ms)?;
    match level {
        Level::Month => Some((wall.year(), wall.month())),
        Level::Year => Some((wall.year(), 0)),
        Level::Hour | Level::Day => None,
    }
}

/// 本地日索引归属的历法周期键（live SQL month/year 路径的 bucket 即日索引；
/// 墙钟当量 = idx * DAY_MS，offset 已在取索引时并入，无需再传）。
pub(crate) fn period_key_of_day_index(idx: i64, level: Level) -> Option<(i32, u32)> {
    let wall = chrono::DateTime::from_timestamp_millis(idx * DAY_MS)?;
    match level {
        Level::Month => Some((wall.year(), wall.month())),
        Level::Year => Some((wall.year(), 0)),
        Level::Hour | Level::Day => None,
    }
}

/// 定长帧（hour/day）覆盖 [start, end) 的全部帧（含越界的首尾帧，由调用方截取）。
fn fixed_frames(level: Level, offset_minutes: i32, start: i64, end: i64) -> Vec<Frame> {
    let bucket_ms = match level {
        Level::Hour => HOUR_MS,
        Level::Day => DAY_MS,
        _ => unreachable!("fixed_frames 只服务 hour/day"),
    };
    let off = offset_ms(offset_minutes);
    let first = (start + off).div_euclid(bucket_ms);
    let last = (end - 1 + off).div_euclid(bucket_ms);
    (first..=last)
        .map(|idx| {
            let f_start = idx * bucket_ms - off;
            Frame {
                level,
                start: f_start,
                end: f_start + bucket_ms,
            }
        })
        .collect()
}

/// 本地历法帧（month/year）覆盖 [start, end) 的全部帧。
/// 墙钟 = epoch + offset（固定偏移模型）；历法起点 = 本地 1 日 0 点。
fn calendar_frames(level: Level, offset_minutes: i32, start: i64, end: i64) -> Vec<Frame> {
    let off = offset_ms(offset_minutes);
    // 本地墙钟当量（epoch + offset）的 UTC 表示，直接读年月。
    let wall = |ts: i64| chrono::DateTime::from_timestamp_millis(ts + off);
    let (first_y, first_m) = wall(start)
        .map(|t| (t.year(), if level == Level::Month { t.month() } else { 1 }))
        .unwrap_or((1970, 1));
    let last_wall = wall(end - 1)
        .unwrap_or_else(|| chrono::DateTime::from_timestamp_millis(0).expect("0 偏移恒可表示"));
    let (last_y, last_m) = if level == Level::Month {
        (last_wall.year(), last_wall.month())
    } else {
        (last_wall.year(), 1)
    };

    // 帧起点（epoch）：本地 (y, m, 1) 0 点的 naive 毫秒 − offset。
    let boundary = |y: i32, m: u32| period_start_ms(y, m, off);
    let mut frames = Vec::new();
    let mut y = first_y;
    let mut m = first_m;
    while let Some(frame_start) = boundary(y, m) {
        let (ny, nm) = match level {
            Level::Month => {
                if m == 12 {
                    (y + 1, 1)
                } else {
                    (y, m + 1)
                }
            }
            _ => (y + 1, 1), // Year
        };
        let Some(frame_end) = boundary(ny, nm) else {
            break;
        };
        if frame_end > start && frame_start < end {
            frames.push(Frame {
                level,
                start: frame_start,
                end: frame_end,
            });
        }
        if (y, m) == (last_y, last_m) {
            break;
        }
        y = ny;
        m = nm;
        if y > last_y + 1 {
            break;
        }
    }
    frames
}

/// 覆盖 [start, end) 的全部 level 帧（含与窗口部分交叠的首尾帧）。
pub(crate) fn frames_covering(
    level: Level,
    offset_minutes: i32,
    start: i64,
    end: i64,
) -> Vec<Frame> {
    match level {
        Level::Hour | Level::Day => fixed_frames(level, offset_minutes, start, end),
        Level::Month | Level::Year => calendar_frames(level, offset_minutes, start, end),
    }
}

/// 覆盖计划的组成块。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Part {
    /// 已闭桶、且完整落在窗口内的桶（读快照行）。
    Snap(Frame),
    /// 实时兑底段（窗口内未被闭桶快照覆盖的部分，聚合 request 表）。
    Live { start: i64, end: i64 },
}

/// 把窗口 [start, end) 按 level 分解为「快照闭桶 + 实时兑底段」：
/// - 帧完整落在窗口内且已闭（end + margin ≤ now）→ Snap；
/// - 窗口边缘部分交叠的帧、或完整但未闭的帧 → 向更细粒度下钻；
/// - hour 仍无法快照（当前/余量内桶）→ Live。
///
/// 相邻 Live 段合并为一段（读侧一次 SQL）。
pub(crate) fn decompose(
    level: Level,
    offset_minutes: i32,
    start: i64,
    end: i64,
    now_ms: i64,
    margin_ms: i64,
) -> Vec<Part> {
    fn cover(
        level: Level,
        offset_minutes: i32,
        s: i64,
        e: i64,
        now_ms: i64,
        margin_ms: i64,
        out: &mut Vec<Part>,
    ) {
        if e <= s {
            return;
        }
        for frame in frames_covering(level, offset_minutes, s, e) {
            let isec_s = s.max(frame.start);
            let isec_e = e.min(frame.end);
            if isec_e <= isec_s {
                continue;
            }
            let full = frame.start >= s && frame.end <= e;
            if full && bucket_closed(frame.end, now_ms, margin_ms) {
                out.push(Part::Snap(frame));
                continue;
            }
            match level.finer() {
                Some(finer) => cover(
                    finer,
                    offset_minutes,
                    isec_s,
                    isec_e,
                    now_ms,
                    margin_ms,
                    out,
                ),
                None => out.push(Part::Live {
                    start: isec_s,
                    end: isec_e,
                }),
            }
        }
    }

    let mut parts = Vec::new();
    cover(
        level,
        offset_minutes,
        start,
        end,
        now_ms,
        margin_ms,
        &mut parts,
    );
    // 合并相邻 Live 段。
    let mut merged: Vec<Part> = Vec::with_capacity(parts.len());
    for part in parts {
        if let (Part::Live { start, end }, Some(Part::Live { end: prev_end, .. })) =
            (part, merged.last_mut())
            && *prev_end == start
        {
            *prev_end = end;
            continue;
        }
        merged.push(part);
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHANGHAI: i32 = 480;

    fn ms(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> i64 {
        NaiveDate::from_ymd_opt(y, mo, d)
            .and_then(|dt| dt.and_hms_opt(h, mi, 0))
            .map(|dt| dt.and_utc().timestamp_millis())
            .unwrap()
    }

    #[test]
    fn natural_periods_covers_window_months_with_gap() {
        // 窗口：2026-06-25 00:00 ~ 2026-08-27 00:00（东八区）→ 6/7/8 三个自然月
        // 全列出（7 月无数据也由调用方补零，列表本身无缺月）。
        let off = 480 * 60_000;
        let start = ms(2026, 6, 24, 16, 0); // 本地 2026-06-25 00:00
        let end = ms(2026, 8, 26, 16, 0); // 本地 2026-08-27 00:00
        let periods = natural_periods(Level::Month, off, start, end);
        assert_eq!(
            periods,
            vec![
                (2026, 6, ms(2026, 5, 31, 16, 0)),
                (2026, 7, ms(2026, 6, 30, 16, 0)),
                (2026, 8, ms(2026, 7, 31, 16, 0)),
            ]
        );
    }

    #[test]
    fn natural_periods_years_use_calendar_month_one() {
        // 窗口：2025-07-01 ~ 2026-09-01（东八区）→ 2025 / 2026 两个年周期，m 恒 1。
        let off = 480 * 60_000;
        let start = ms(2025, 6, 30, 16, 0); // 本地 2025-07-01
        let end = ms(2026, 8, 31, 16, 0); // 本地 2026-09-01
        let periods = natural_periods(Level::Year, off, start, end);
        assert_eq!(
            periods,
            vec![
                (2025, 1, ms(2024, 12, 31, 16, 0)),
                (2026, 1, ms(2025, 12, 31, 16, 0)),
            ]
        );
    }

    #[test]
    fn period_key_of_ts_month_and_year_conventions() {
        let off = 480 * 60_000;
        // UTC 2026-07-15 20:00 = 本地 2026-07-16 04:00。
        let ts = ms(2026, 7, 15, 20, 0);
        assert_eq!(period_key_of_ts(ts, off, Level::Month), Some((2026, 7)));
        assert_eq!(period_key_of_ts(ts, off, Level::Year), Some((2026, 0)));
        assert_eq!(period_key_of_ts(ts, off, Level::Day), None);
    }

    #[test]
    fn period_key_of_day_index_matches_ts_key() {
        // 本地日索引 = (ts + off) / DAY_MS；两条路径应得出同一历法周期键。
        let off = 480 * 60_000;
        let ts = ms(2026, 7, 15, 20, 0); // 本地 2026-07-16
        let idx = (ts + off).div_euclid(DAY_MS);
        assert_eq!(
            period_key_of_day_index(idx, Level::Month),
            period_key_of_ts(ts, off, Level::Month)
        );
        assert_eq!(
            period_key_of_day_index(idx, Level::Year),
            period_key_of_ts(ts, off, Level::Year)
        );
    }

    #[test]
    fn hour_frame_aligned_to_local_boundary() {
        // 上海 +8：UTC 1970-01-01 00:00 = 本地 08:00，属本地 08:00~09:00 桶
        //（帧起点 = UTC 1970-01-01 00:00，即本地 08:00）。
        let frames = frames_covering(Level::Hour, SHANGHAI, 0, HOUR_MS);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].start, 0);
        assert_eq!(frames[0].end, HOUR_MS);
        // 紧邻前一小时桶：[本地 07:00, 08:00) = UTC 前一日 23:00 ~ 00:00。
        let prev = frames_covering(Level::Hour, SHANGHAI, -HOUR_MS, 0);
        assert_eq!(prev.len(), 1);
        assert_eq!(prev[0].start, -HOUR_MS);
        assert_eq!(prev[0].end, 0);
    }

    #[test]
    fn day_frame_spans_local_midnight() {
        // UTC 2024-01-31 20:00 = 上海 2024-02-01 04:00。
        let t = ms(2024, 1, 31, 20, 0);
        let frames = frames_covering(Level::Day, SHANGHAI, t, t + 1);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].start, ms(2024, 1, 31, 16, 0)); // 本地 2/1 0 点
        assert_eq!(frames[0].end, ms(2024, 2, 1, 16, 0));
    }

    #[test]
    fn month_frame_across_year_and_leap() {
        // 上海：2024-01-31T16:00Z = 本地 2024-02-01。2 月（闰年 29 天）。
        let start = ms(2024, 1, 31, 16, 0);
        let frames = frames_covering(Level::Month, SHANGHAI, start, start + 1);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].start, start);
        assert_eq!(frames[0].end, ms(2024, 2, 29, 16, 0)); // 本地 3/1 0 点
        assert_eq!(frames[0].end - frames[0].start, 29 * DAY_MS);
    }

    #[test]
    fn year_frame_boundaries() {
        // 与窗口 [2023-06, 2025-01) 部分交叠的本地年帧：2023/2024/2025（首尾含越界帧）。
        let frames = frames_covering(
            Level::Year,
            SHANGHAI,
            ms(2023, 6, 1, 0, 0),
            ms(2025, 1, 1, 0, 0),
        );
        let starts: Vec<i64> = frames.iter().map(|f| f.start).collect();
        assert_eq!(
            starts,
            vec![
                ms(2022, 12, 31, 16, 0), // 本地 2023-01-01
                ms(2023, 12, 31, 16, 0), // 本地 2024-01-01
                ms(2024, 12, 31, 16, 0), // 本地 2025-01-01（与窗口末端交叠）
            ]
        );
        assert!(
            frames
                .iter()
                .all(|f| f.end - f.start == 365 * DAY_MS || f.end - f.start == 366 * DAY_MS)
        );
    }

    #[test]
    fn bucket_closed_respects_margin() {
        let end = ms(2024, 1, 1, 10, 0);
        assert!(!bucket_closed(end, end + 60 * HOUR_MS - 1, 60 * HOUR_MS));
        assert!(bucket_closed(end, end + 60 * HOUR_MS, 60 * HOUR_MS));
    }

    #[test]
    fn decompose_hour_window_closed_middle() {
        // 窗口 = 连续 4 个整点桶，now 已过全部 4 桶的余量 → 全 Snap。
        let start = ms(2024, 1, 1, 8, 0); // UTC 对齐 +8 本地整点？UTC 8:00 = 本地 16:00
        let end = start + 4 * HOUR_MS;
        let now = end + 2 * HOUR_MS; // 距最后桶终点 2h > 1h 余量
        let parts = decompose(Level::Hour, SHANGHAI, start, end, now, MARGIN_MS);
        assert_eq!(parts.len(), 4);
        assert!(parts.iter().all(|p| matches!(p, Part::Snap(_))));
    }

    #[test]
    fn decompose_recent_tail_is_live() {
        // 最后两个桶未过余量：应下钻为 Live 段（hour 最细），且与前面 Snap 不重叠。
        let start = ms(2024, 1, 1, 8, 0);
        let end = start + 4 * HOUR_MS;
        let now = end - 30 * 60_000; // 距窗口终点 30 分钟 → 最后 1 桶（+ 部分）未闭
        let parts = decompose(Level::Hour, SHANGHAI, start, end, now, MARGIN_MS);
        let live: Vec<_> = parts
            .iter()
            .filter_map(|p| match p {
                Part::Live { start, end } => Some((*start, *end)),
                _ => None,
            })
            .collect();
        assert_eq!(live.len(), 1, "相邻 live 应合并: {parts:?}");
        let (live_start, live_end) = live[0];
        assert!(
            live_start >= start + 2 * HOUR_MS && live_end == end,
            "{parts:?}"
        );
    }

    #[test]
    fn decompose_month_partial_edges_drill_down() {
        // 月粒度窗口跨 2024-12 中旬 → 2025-02 上旬：12 月与 2 月部分帧下钻为
        // 天/小时，1 月整帧闭桶 → Snap(month)。now 取远期，全部历史闭桶。
        let start = ms(2024, 12, 15, 0, 0);
        let end = ms(2025, 2, 10, 0, 0);
        let now = ms(2026, 1, 1, 0, 0);
        let parts = decompose(Level::Month, SHANGHAI, start, end, now, MARGIN_MS);
        let snap_months: Vec<i64> = parts
            .iter()
            .filter_map(|p| match p {
                Part::Snap(f) if f.level == Level::Month => Some(f.start),
                _ => None,
            })
            .collect();
        assert_eq!(snap_months, vec![ms(2024, 12, 31, 16, 0)]); // 本地 2025-01
        // 首尾边缘不再出现 month Snap（部分帧不下钻整月）。
        assert!(!parts.iter().any(
            |p| matches!(p, Part::Snap(f) if f.level == Level::Month && f.start != snap_months[0])
        ));
    }

    #[test]
    fn decompose_mid_window_unclosed_never_snap() {
        // 荒谬场景守卫：任何 Snap 帧终点必须已过 now + margin。
        let start = ms(2024, 1, 1, 0, 0);
        let end = start + 10 * DAY_MS;
        let now = start + 2 * DAY_MS;
        let parts = decompose(Level::Day, SHANGHAI, start, end, now, MARGIN_MS);
        for part in parts {
            if let Part::Snap(f) = part {
                assert!(bucket_closed(f.end, now, MARGIN_MS));
            }
        }
    }

    #[test]
    fn decompose_arbitrary_window_mixed_levels() {
        // 无粒度窗口（rank/metrics 语义）：天级起点 + 部分天 → 小时 → live 尾部。
        let start = ms(2024, 3, 5, 7, 30); // 非整点 → 首日部分
        let end = start + 3 * DAY_MS - 2 * HOUR_MS;
        let now = end + 5 * HOUR_MS;
        let parts = decompose(Level::Day, SHANGHAI, start, end, now, MARGIN_MS);
        // 首日（部分）与末日（部分）无 Snap(day)；完整中间天有 Snap(day)。
        let snap_days: Vec<i64> = parts
            .iter()
            .filter_map(|p| match p {
                Part::Snap(f) if f.level == Level::Day => Some(f.start),
                _ => None,
            })
            .collect();
        let first_full_day = (start + DAY_MS - 1).div_euclid(DAY_MS) * DAY_MS;
        let first_full_local_day_start = first_full_day - 8 * HOUR_MS; // 本地午夜起点 +8
        assert!(
            snap_days.contains(&first_full_local_day_start),
            "{snap_days:?}"
        );
        assert!(parts.iter().any(|p| matches!(p, Part::Live { .. })));
    }
}

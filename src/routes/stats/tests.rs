
use super::*;
use chrono::TimeZone;

fn tz480() -> chrono::FixedOffset {
    chrono::FixedOffset::east_opt(480 * 60).unwrap()
}

/// 本地日期 → UTC 毫秒时间戳（东八区）。
fn local_ms(y: i32, m: u32, d: u32, h: u32, min: u32) -> i64 {
    tz480()
        .with_ymd_and_hms(y, m, d, h, min, 0)
        .single()
        .unwrap()
        .timestamp_millis()
}

#[test]
fn parse_granularity_accepts_all_kinds() {
    assert_eq!(Granularity::parse(None), Ok(None));
    assert_eq!(
        Granularity::parse(Some("hour")),
        Ok(Some(Granularity::Hour))
    );
    assert_eq!(Granularity::parse(Some("day")), Ok(Some(Granularity::Day)));
    assert_eq!(
        Granularity::parse(Some("month")),
        Ok(Some(Granularity::Month))
    );
    assert_eq!(
        Granularity::parse(Some("year")),
        Ok(Some(Granularity::Year))
    );
    assert!(Granularity::parse(Some("week")).is_err());
}

#[test]
fn merge_natural_periods_groups_and_zero_fills_months() {
    // 窗口：2026-06-25 00:00 ~ 2026-08-27 00:00（东八区）。
    let start = local_ms(2026, 6, 25, 0, 0);
    let end = local_ms(2026, 8, 27, 0, 0);
    // 日索引数据：6/25 一次、7/15 一次、7/16 一次、8/26 一次。
    // 本地 0 点的 UTC 毫秒 / DAY_MS 即本地日索引（东八区 +480）。
    let day_indexes = vec![
        (start / DAY_MS, 1),
        (local_ms(2026, 7, 15, 0, 0) / DAY_MS, 5),
        (local_ms(2026, 7, 16, 0, 0) / DAY_MS, 7),
        (local_ms(2026, 8, 26, 0, 0) / DAY_MS, 3),
    ];
    let (starts, calls, tokens) =
        merge_natural_periods(&day_indexes, &day_indexes, start, end, tz480(), true);
    assert_eq!(starts.len(), 3);
    assert_eq!(starts[0], local_ms(2026, 6, 1, 0, 0));
    assert_eq!(calls[0], 1);
    assert_eq!(starts[1], local_ms(2026, 7, 1, 0, 0));
    assert_eq!(calls[1], 12);
    assert_eq!(starts[2], local_ms(2026, 8, 1, 0, 0));
    assert_eq!(calls[2], 3);
    assert_eq!(tokens, calls);
}

#[test]
fn merge_natural_periods_zero_fills_gap_months() {
    // 窗口：2026-06-25 ~ 2026-08-27，无 7 月数据 → 7 月补零。
    let start = local_ms(2026, 6, 25, 0, 0);
    let end = local_ms(2026, 8, 27, 0, 0);
    let day_indexes = vec![
        (start / DAY_MS, 1),
        (local_ms(2026, 8, 26, 0, 0) / DAY_MS, 3),
    ];
    let (starts, calls, _) =
        merge_natural_periods(&day_indexes, &day_indexes, start, end, tz480(), true);
    assert_eq!(starts.len(), 3);
    assert_eq!(starts[1], local_ms(2026, 7, 1, 0, 0));
    assert_eq!(calls[1], 0);
}

#[test]
fn merge_natural_periods_groups_years() {
    // 窗口：2025-07-01 ~ 2026-09-01 → 2025 / 2026 两个年桶。
    let start = local_ms(2025, 7, 1, 0, 0);
    let end = local_ms(2026, 9, 1, 0, 0);
    let day_indexes = vec![
        (start / DAY_MS, 2),
        (local_ms(2026, 3, 5, 0, 0) / DAY_MS, 4),
    ];
    let (starts, calls, _) =
        merge_natural_periods(&day_indexes, &day_indexes, start, end, tz480(), false);
    assert_eq!(starts.len(), 2);
    assert_eq!(starts[0], local_ms(2025, 1, 1, 0, 0));
    assert_eq!(calls[0], 2);
    assert_eq!(starts[1], local_ms(2026, 1, 1, 0, 0));
    assert_eq!(calls[1], 4);
}

fn window_at(start: i64, end: i64, bucket_ms: i64, tz_offset_minutes: i32) -> ChartWindow {
    ChartWindow {
        start,
        end,
        bucket_ms,
        granularity: Granularity::Hour,
        tz_offset_minutes,
    }
}

#[test]
fn bucket_range_covers_window_buckets_with_offset() {
    // UTC+8、小时桶、start 非整点对齐：硬编码期望区间作为独立预言
    //（1700000000000 + 8h → 首桶 472230；end=start+3h-1 开区间 → 末桶 472233，
    //  窗口跨 4 个小时桶）。
    let w = window_at(
        1_700_000_000_000,
        1_700_000_000_000 + 3 * HOUR_MS - 1,
        HOUR_MS,
        480,
    );
    assert_eq!(w.bucket_range(), 472_230..=472_233);
    // 桶起点回算与 bucket_start_ms 互逆。
    let offset_ms = i64::from(480) * 60_000;
    for bucket in w.bucket_range() {
        assert_eq!(w.bucket_start_ms(bucket) + offset_ms, bucket * HOUR_MS);
    }
}

#[test]
fn bucket_range_never_empty_and_zero_offset() {
    // 不足一桶的窗口：至少含 start 所在桶。
    let w = window_at(1000, 1001, HOUR_MS, 0);
    let mut buckets = w.bucket_range();
    assert_eq!(buckets.next(), Some(0));
    assert_eq!(buckets.next(), None);
}

#[test]
fn percentile_interpolates_and_handles_edges() {
    assert_eq!(percentile(&[], 0.95), 0.0, "空样本返回 0");
    assert_eq!(percentile(&[5.0], 0.95), 5.0);
    let sorted = vec![10.0, 20.0, 30.0, 40.0];
    // (n-1)·p 位置插值：p=0.5 → (4-1)*0.5=1.5 → 25。
    assert_eq!(percentile(&sorted, 0.5), 25.0);
    assert_eq!(percentile(&sorted, 0.0), 10.0);
    assert_eq!(percentile(&sorted, 1.0), 40.0);
    // p=0.75 → 位置 2.25 → 30 + 0.25*10 = 32.5。
    assert!((percentile(&sorted, 0.75) - 32.5).abs() < f64::EPSILON);
}

#[test]
fn round5_and_weighted_ratio_match_sql_rounding() {
    assert_eq!(round_5(0.123456), 0.12346);
    assert_eq!(round_5(0.123454), 0.12345);
    assert_eq!(weighted_ratio(1.0, 3.0), 0.33333, "1/3 保留 5 位");
    assert_eq!(weighted_ratio(3.0, 0.0), 0.0, "分母为 0 记 0");
    assert_eq!(weighted_ratio(0.0, 5.0), 0.0);
}

#[test]
fn required_time_range_validates_pair() {
    assert_eq!(required_time_range(Some(1), Some(2)), Ok((1, 2)));
    assert!(required_time_range(None, Some(2)).is_err());
    assert!(required_time_range(Some(1), None).is_err());
    assert!(
        required_time_range(Some(2), Some(2)).is_err(),
        "end == start 非法"
    );
    assert!(required_time_range(Some(3), Some(2)).is_err());
}

#[test]
fn tz_offset_minutes_matches_fixed_zones() {
    // 2024-01-15T00:00:00Z：上海 +480、东京 +540、UTC 0、纽约冬令时 -300。
    let at = 1_705_276_800_000;
    assert_eq!(tz_offset_minutes_at(chrono_tz::Asia::Shanghai, at), 480);
    assert_eq!(tz_offset_minutes_at(chrono_tz::Asia::Tokyo, at), 540);
    assert_eq!(tz_offset_minutes_at(chrono_tz::UTC, at), 0);
    assert_eq!(tz_offset_minutes_at(chrono_tz::America::New_York, at), -300);
    // 夏令时（2024-07-15）：纽约 -240；上海不变。
    let summer = 1_721_001_600_000;
    assert_eq!(
        tz_offset_minutes_at(chrono_tz::America::New_York, summer),
        -240
    );
    assert_eq!(tz_offset_minutes_at(chrono_tz::Asia::Shanghai, summer), 480);
}

#[test]
fn stats_tz_offset_defaults_to_shanghai_and_ignores_bad_hints() {
    // 无设置时（timezone_sync 缺省 Asia/Shanghai）：+480。
    assert_eq!(stats_tz_offset_minutes(None), 480);
    assert_eq!(stats_tz_offset_minutes(Some(1_705_276_800_000)), 480);
    // 非正起点提示回退 now：偏移必在合法 ±840 分钟内。
    let offset = stats_tz_offset_minutes(Some(0));
    assert!((-840..=840).contains(&offset));
}

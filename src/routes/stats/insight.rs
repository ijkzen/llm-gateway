use super::*;

/// 每桶延迟分位点（毫秒；该桶无样本时字段为 0）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PercentilePoint {
    bucket_start: i64,
    p50: f64,
    p90: f64,
    p95: f64,
    p99: f64,
}

/// 失败原因分布条目（空/缺失原因归「无原因」，由前端文案呈现）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FailureReasonItem {
    reason: String,
    count: i64,
}

/// 按 API Key 聚合的调用量条目。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiKeyRankItem {
    api_key_name: String,
    value: i64,
}

/// 性能与可靠性分析（insight）：一次返回失败诊断 / 延迟分位 / Token 结构 / 吞吐四组数据。
///
/// 口径：失败相关基于全量请求（成功+失败都计数）；延迟与 Token 相关基于成功请求
/// （`success = 1`，与赛马/指标一致）。闭桶（按查询粒度帧）读快照，其余兑底
/// （失败原因与 API Key 排行窗口级明细实时聚合，快照结构无对应维度）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InsightResponse {
    // 失败诊断
    /// 每桶全部调用数（成功+失败；成功/失败堆叠面积图基准）。
    call_trend: Vec<TrendPoint>,
    failure_trend: Vec<TrendPoint>,
    failure_rate_trend: Vec<FloatTrendPoint>,
    failure_reasons: Vec<FailureReasonItem>,
    // 延迟分位
    ttft_percentiles: Vec<PercentilePoint>,
    latency_percentiles: Vec<PercentilePoint>,
    // Token 结构
    input_token_trend: Vec<TrendPoint>,
    output_token_trend: Vec<TrendPoint>,
    cache_hit_rate_trend: Vec<FloatTrendPoint>,
    output_tokens_per_sec_trend: Vec<FloatTrendPoint>,
    // 吞吐 / 调用入口
    api_key_rank: Vec<ApiKeyRankItem>,
    rpm_trend: Vec<TrendPoint>,
    tpm_trend: Vec<FloatTrendPoint>,
    stream_ratio_trend: Vec<FloatTrendPoint>,
}

use crate::stats_snapshot as snap;

/// 每桶可加和序列（hour/day 用桶索引；month/year 用本地 (年, 月|0)）。
#[derive(Default)]
struct BucketSeries {
    idx: std::collections::BTreeMap<i64, f64>,
    period: std::collections::BTreeMap<(i32, u32), f64>,
}

/// 桶记账 key 计算：快照帧起点 → hour/day 桶索引 / month/year (年,月|0)。
fn bucket_key_of(
    ts: i64,
    off_ms: i64,
    bucket_ms: i64,
    month_mode: bool,
) -> Option<(i64, i32, u32)> {
    if month_mode {
        let wall = chrono::DateTime::from_timestamp_millis(ts + off_ms)?;
        if wall.month() == 0 {
            return None;
        }
        Some((0, wall.year(), wall.month()))
    } else {
        Some(((ts + off_ms).div_euclid(bucket_ms), 0, 0))
    }
}

fn period_of_day_index(idx: i64, off_ms: i64, month_mode: bool) -> Option<(i32, u32)> {
    // live SQL 的 bucket = 本地日索引（month/year 路径），日起点 epoch = idx*DAY - off。
    let _ = off_ms;
    let wall = chrono::DateTime::from_timestamp_millis(idx * super::DAY_MS)?;
    if month_mode {
        Some((wall.year(), wall.month()))
    } else {
        Some((wall.year(), 0))
    }
}

/// 系列序列号：0 calls / 1 fails / 2 streams / 3 inputs / 4 outputs / 5 caches /
/// 6 tokens_all / 7 out_sec（与 METRICS 表一一对应）。
const SERIES_METRICS: [&str; 8] = [
    snap::metrics::CALLS,
    snap::metrics::FAIL_CALLS,
    snap::metrics::STREAM_CALLS,
    snap::metrics::INPUT_TOKENS,
    snap::metrics::OUTPUT_TOKENS,
    snap::metrics::CACHE_TOKENS,
    snap::metrics::TOKENS_ALL,
    snap::metrics::OUT_SEC_SUM,
];

/// 把快照行（按帧）并入各序列的桶记账。
fn fold_snapshot_rows(
    series: &mut [BucketSeries; 8],
    rows: Vec<(i64, String, String, String, f64)>, // (start, et, e, metric, value)
    off_ms: i64,
    bucket_ms: i64,
    month_mode: bool,
) {
    for (start, _, _, metric, value) in rows {
        let Some((idx, y, m)) = bucket_key_of(start, off_ms, bucket_ms, month_mode) else {
            continue;
        };
        if let Some(i) = SERIES_METRICS.iter().position(|x| *x == metric) {
            if month_mode {
                *series[i].period.entry((y, m)).or_insert(0.0) += value;
            } else {
                *series[i].idx.entry(idx).or_insert(0.0) += value;
            }
        }
    }
}

/// live 兑底段分桶并入（与旧 group_rows 同口径；month/year 段内按日索引归并）。
#[allow(clippy::too_many_arguments)]
async fn run_live_series(
    db: &sea_orm::DatabaseConnection,
    segs: &[(i64, i64)],
    bucket_expr: &str,
    filter_sql: &str,
    filter_params: &[sea_orm::Value],
    series: &mut [BucketSeries; 8],
    off_ms: i64,
    month_mode: bool,
) -> Result<(), String> {
    for &(s, e) in segs {
        let sql = format!(
            "SELECT {bucket_expr} AS bucket, COUNT(*) AS calls, \
                    COALESCE(SUM(CASE WHEN r.success = 0 THEN 1 ELSE 0 END), 0) AS fail_calls, \
                    COALESCE(SUM(CASE WHEN r.stream THEN 1 ELSE 0 END), 0) AS stream_calls, \
                    COALESCE(SUM(CASE WHEN r.success = 1 THEN r.input_tokens END), 0) AS input_tokens, \
                    COALESCE(SUM(CASE WHEN r.success = 1 THEN r.output_tokens END), 0) AS output_tokens, \
                    COALESCE(SUM(CASE WHEN r.success = 1 THEN r.input_cache_tokens END), 0) AS cache_tokens, \
                    COALESCE(SUM(r.total_tokens), 0) AS tokens_all, \
                    COALESCE(SUM(CASE WHEN r.success = 1 AND r.output_tokens_time > 0 \
                                      THEN r.output_tokens / (r.output_tokens_time / 1000.0) ELSE 0 END), 0) \
                            AS out_sec_sum \
             FROM request r WHERE r.start_time >= {s} AND r.start_time < {e}{filter_sql} \
             GROUP BY bucket"
        );
        let rows = match db
            .query_all_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                sql.clone(),
                filter_params.to_vec(),
            ))
            .await
        {
            Ok(rows) => rows,
            Err(e) => return Err(e.to_string()),
        };
        for row in rows {
            let bucket: i64 = row.try_get("", "bucket").unwrap_or(0);
            for (i, metric) in SERIES_METRICS.iter().enumerate() {
                let value: f64 = row
                    .try_get("", metric)
                    .ok()
                    .or_else(|| row.try_get::<i64>("", metric).ok().map(|v| v as f64))
                    .unwrap_or(0.0);
                if month_mode {
                    if let Some((y, m)) = period_of_day_index(bucket, off_ms, month_mode) {
                        *series[i].period.entry((y, m)).or_insert(0.0) += value;
                    }
                } else {
                    *series[i].idx.entry(bucket).or_insert(0.0) += value;
                }
            }
        }
    }
    Ok(())
}

/// 性能与可靠性分析：失败诊断 / 延迟分位 / Token 结构 / 吞吐 四组聚合。
/// 与 charts 共用窗口/粒度/时区与过滤参数；闭桶（帧级）读快照、其余兑底；
/// 分位在 hour/day 闭桶读 p 标量行、边缘/未闭桶实时取原始值；
/// 月/年桶比值在归并后的桶上重算、分位与 rpm/tpm 维持空数组。
pub(crate) async fn insight(
    State(state): State<AppState>,
    Query(query): Query<ChartsQuery>,
) -> impl IntoResponse {
    let explicit_granularity = match Granularity::parse(query.granularity.as_deref()) {
        Ok(g) => g,
        Err(msg) => return response::bad_request(msg),
    };
    let tz_offset_minutes = stats_tz_offset_minutes(query.start_time);
    let window = resolve_chart_window(
        query.start_time,
        query.end_time,
        explicit_granularity,
        tz_offset_minutes,
    );
    let db = &state.db;
    let now = chrono::Utc::now().timestamp_millis();
    let month_mode = matches!(window.granularity, Granularity::Month | Granularity::Year);
    let year_mode = matches!(window.granularity, Granularity::Year);
    let off_ms = i64::from(tz_offset_minutes) * 60_000;
    let (filter_sql, filter_params) = super::summary_charts::filter_parts(&query);
    let (entity_type, exact_entity) = match super::summary_charts::trend_entity(db, &query).await {
        Some(v) => v,
        None => (snap::ENTITY_WHOLE, None), // 兑底路径仍由 below 的 live 覆盖
    };

    // 覆盖计划：显式 hour/day/month/year 且过滤形态可落快照 → 闭桶读快照；
    // 其余整窗兑底。
    let snap_level = match (window.granularity, window.bucket_ms) {
        (Granularity::Hour, b) if b == HOUR_MS => Some(snap::Level::Hour),
        (Granularity::Day, b) if b == DAY_MS => Some(snap::Level::Day),
        (Granularity::Month, _) => Some(snap::Level::Month),
        (Granularity::Year, _) => Some(snap::Level::Year),
        _ => None,
    };
    let supported = super::summary_charts::trend_entity(db, &query)
        .await
        .is_some();
    let coverage = match (snap_level, supported) {
        (Some(level), true) => {
            match snap::coverage(
                db,
                level,
                tz_offset_minutes,
                window.start,
                window.end,
                now,
                snap::MARGIN_MS,
            )
            .await
            {
                Ok(cov) => match snap::trim_zero_prefix(db, cov).await {
                    Ok(cov) => cov,
                    Err(e) => return response::db_error(e.to_string()),
                },
                Err(e) => return response::db_error(e.to_string()),
            }
        }
        _ => snap::Coverage {
            snapshots: vec![],
            live: vec![(window.start, window.end)],
        },
    };

    let mut series: [BucketSeries; 8] = Default::default();

    // 快照贡献（按帧粒度分组取行）。
    let mut by_level: std::collections::BTreeMap<snap::Level, Vec<snap::Frame>> =
        std::collections::BTreeMap::new();
    for frame in &coverage.snapshots {
        by_level.entry(frame.level).or_default().push(*frame);
    }
    for (level, frames) in &by_level {
        let rows = match snap::snapshot_rows(
            db,
            *level,
            frames,
            entity_type,
            exact_entity.as_deref(),
            &SERIES_METRICS,
        )
        .await
        {
            Ok(rows) => rows,
            Err(e) => return response::db_error(e.to_string()),
        };
        fold_snapshot_rows(&mut series, rows, off_ms, window.bucket_ms, month_mode);
    }

    // 兑底贡献（分段聚合，口径同旧 group_rows 全量/成功两集）。
    let bucket_expr = window.bucket_expr();
    if let Err(e) = run_live_series(
        db,
        &coverage.live,
        &bucket_expr,
        &filter_sql,
        &filter_params,
        &mut series,
        off_ms,
        month_mode,
    )
    .await
    {
        return response::db_error(e);
    }

    // 自然月/年补零归并输出（比值类在归并后重算）。
    let bucket_starts = |series: &BucketSeries| -> Vec<(i64, f64)> {
        if month_mode {
            // 生成桶起点列表（与 charts 同法：窗口首末自然月/年逐月补零）。
            let (Some(first), Some(last)) = (
                chrono::DateTime::from_timestamp_millis(window.start + off_ms),
                chrono::DateTime::from_timestamp_millis(window.end - 1 + off_ms),
            ) else {
                return Vec::new();
            };
            let mut out = Vec::new();
            if year_mode {
                for y in first.year()..=last.year() {
                    let start = chrono::NaiveDate::from_ymd_opt(y, 1, 1)
                        .and_then(|d| d.and_hms_opt(0, 0, 0))
                        .map(|dt| dt.and_utc().timestamp_millis() - off_ms);
                    if let Some(start) = start {
                        out.push((start, series.period.get(&(y, 0)).copied().unwrap_or(0.0)));
                    }
                }
            } else {
                let mut y = first.year();
                let mut m = first.month();
                let (ly, lm) = (last.year(), last.month());
                loop {
                    let start = chrono::NaiveDate::from_ymd_opt(y, m, 1)
                        .and_then(|d| d.and_hms_opt(0, 0, 0))
                        .map(|dt| dt.and_utc().timestamp_millis() - off_ms);
                    if let Some(start) = start {
                        out.push((start, series.period.get(&(y, m)).copied().unwrap_or(0.0)));
                    }
                    if (y, m) == (ly, lm) {
                        break;
                    }
                    m += 1;
                    if m > 12 {
                        m = 1;
                        y += 1;
                    }
                }
            }
            out
        } else {
            window
                .bucket_range()
                .map(|bucket| {
                    (
                        window.bucket_start_ms(bucket),
                        series.idx.get(&bucket).copied().unwrap_or(0.0),
                    )
                })
                .collect()
        }
    };

    let int_values = |s: &BucketSeries| -> Vec<i64> {
        bucket_starts(s)
            .into_iter()
            .map(|(_, v)| v.round() as i64)
            .collect()
    };
    let float_values =
        |s: &BucketSeries| -> Vec<f64> { bucket_starts(s).into_iter().map(|(_, v)| v).collect() };
    let starts_only =
        |s: &BucketSeries| -> Vec<i64> { bucket_starts(s).into_iter().map(|(st, _)| st).collect() };

    let call_starts = starts_only(&series[0]);
    let call_trend = call_starts
        .iter()
        .copied()
        .zip(int_values(&series[0]))
        .map(|(bucket_start, value)| TrendPoint {
            bucket_start,
            value,
        })
        .collect();
    let failure_trend = call_starts
        .iter()
        .copied()
        .zip(int_values(&series[1]))
        .map(|(bucket_start, value)| TrendPoint {
            bucket_start,
            value,
        })
        .collect();
    // 比值在桶起点对齐后现算（同一组补零桶，与旧实现一致）。
    let ratio = |num: &BucketSeries, den: &BucketSeries| -> Vec<FloatTrendPoint> {
        let nums = float_values(num);
        let dens = float_values(den);
        call_starts
            .iter()
            .copied()
            .zip(nums.iter().zip(dens.iter()))
            .map(|(bucket_start, (n, d))| FloatTrendPoint {
                bucket_start,
                value: if *d > 0.0 { n / d } else { 0.0 },
            })
            .collect()
    };
    let failure_rate_trend = ratio(&series[1], &series[0]);
    let stream_ratio_trend = ratio(&series[2], &series[0]);
    let cache_rate_trend = ratio(&series[5], &series[3])
        .into_iter()
        .map(|p| FloatTrendPoint {
            bucket_start: p.bucket_start,
            value: round_5(p.value),
        })
        .collect::<Vec<_>>();

    let to_int = |vals: Vec<i64>| -> Vec<TrendPoint> {
        call_starts
            .iter()
            .copied()
            .zip(vals)
            .map(|(bucket_start, value)| TrendPoint {
                bucket_start,
                value,
            })
            .collect()
    };
    let input_token_trend = to_int(int_values(&series[3]));
    let output_token_trend = to_int(int_values(&series[4]));
    let output_per_sec_trend = call_starts
        .iter()
        .copied()
        .zip(float_values(&series[7]))
        .map(|(bucket_start, value)| FloatTrendPoint {
            bucket_start,
            value,
        })
        .collect::<Vec<_>>();

    // RPM/TPM：仅小时桶有意义（RPM=桶调用数÷窗口小时数；TPM=桶 token÷60）。
    let (rpm_trend, tpm_trend) = if matches!(window.granularity, Granularity::Hour) {
        let hours = ((window.end - window.start) as f64 / HOUR_MS as f64).max(1.0);
        (
            call_starts
                .iter()
                .copied()
                .zip(int_values(&series[0]))
                .map(|(bucket_start, value)| TrendPoint {
                    bucket_start,
                    value: ((value as f64 / hours).round()) as i64,
                })
                .collect(),
            call_starts
                .iter()
                .copied()
                .zip(float_values(&series[6]))
                .map(|(bucket_start, value)| FloatTrendPoint {
                    bucket_start,
                    value: value / 60.0,
                })
                .collect(),
        )
    } else {
        (Vec::new(), Vec::new())
    };

    // 失败原因分布：整窗实时（成功=0 行按原因分组；快照无原因维度列）。
    let mut reason_map: std::collections::BTreeMap<String, i64> = Default::default();
    {
        let mut params: Vec<sea_orm::Value> = vec![window.start.into(), window.end.into()];
        let mut where_sql = String::from("r.start_time >= ? AND r.start_time < ?");
        where_sql.push_str(&filter_sql);
        for v in filter_params.iter() {
            params.push(v.clone());
        }
        let sql = format!(
            "SELECT COALESCE(NULLIF(r.fail_reason, ''), '') AS reason, COUNT(*) AS count \
             FROM request r WHERE {where_sql} AND r.success = 0 GROUP BY reason"
        );
        match db
            .query_all_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                sql,
                params,
            ))
            .await
        {
            Ok(rows) => {
                for row in &rows {
                    let reason: String = row.try_get("", "reason").unwrap_or_default();
                    let count: i64 = row.try_get("", "count").unwrap_or(0);
                    *reason_map.entry(reason).or_insert(0) += count;
                }
            }
            Err(e) => return response::db_error(e.to_string()),
        }
    }

    // 延迟分位：hour/day 桶。全闭桶（与窗口同粒度帧）读快照 p 标量行；
    // 边缘/未闭桶实时取原始值计算。月/年恒为空。
    let group_percentiles = async |entity_type: &str,
                                   exact: Option<&str>,
                                   p_base: &str,
                                   value_sql: &str,
                                   extra_cond: &str|
           -> Vec<PercentilePoint> {
        if month_mode {
            return Vec::new();
        }
        let mut p_rows: std::collections::BTreeMap<i64, [f64; 4]> = Default::default();
        // 整闭桶（与查询粒度同层）的 p 标量行读取；跨层（细帧）不叠加。
        let prims = [
            format!("{p_base}_p50"),
            format!("{p_base}_p90"),
            format!("{p_base}_p95"),
            format!("{p_base}_p99"),
        ];
        let prim_refs: Vec<&str> = prims.iter().map(|s| s.as_str()).collect();
        for (level, frames) in &by_level {
            if !matches!(*level, snap::Level::Hour | snap::Level::Day) {
                continue;
            }
            let rows = snap::snapshot_rows(db, *level, frames, entity_type, exact, &prim_refs)
                .await
                .unwrap_or_default();
            for (start, _, _, metric, value) in rows {
                let idx = (start + off_ms).div_euclid(window.bucket_ms);
                if let Some(pos) = prim_refs.iter().position(|m| *m == metric) {
                    let entry = p_rows.entry(idx).or_insert([0.0; 4]);
                    entry[pos] = value;
                }
            }
        }
        // 未完全闭桶（缺 p 标量）的桶：实时取该桶∩窗口原始值算分位。
        for bucket in window.bucket_range() {
            if p_rows.contains_key(&bucket) {
                continue;
            }
            let b_start = window.bucket_start_ms(bucket);
            let b_end = b_start + window.bucket_ms;
            let s = b_start.max(window.start);
            let e = b_end.min(window.end);
            if e <= s {
                continue;
            }
            let mut values: Vec<f64> = Vec::new();
            let mut params: Vec<sea_orm::Value> = vec![s.into(), e.into()];
            let mut where_sql =
                String::from("r.start_time >= ? AND r.start_time < ? AND r.success = 1");
            where_sql.push_str(extra_cond);
            where_sql.push_str(&filter_sql);
            for v in filter_params.iter() {
                params.push(v.clone());
            }
            let sql = format!("SELECT {value_sql} AS value FROM request r WHERE {where_sql}");
            if let Ok(rows) = db
                .query_all_raw(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    sql,
                    params,
                ))
                .await
            {
                for row in rows {
                    if let Some(v) = row
                        .try_get::<f64>("", "value")
                        .ok()
                        .or_else(|| row.try_get::<i64>("", "value").ok().map(|v| v as f64))
                    {
                        values.push(v);
                    }
                }
            }
            values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            p_rows.insert(
                bucket,
                [
                    percentile(&values, 0.5),
                    percentile(&values, 0.9),
                    percentile(&values, 0.95),
                    percentile(&values, 0.99),
                ],
            );
        }
        // 按桶补零输出（旧实现：无样本桶 0）。
        window
            .bucket_range()
            .map(|bucket| {
                let p = p_rows.get(&bucket).copied().unwrap_or([0.0; 4]);
                PercentilePoint {
                    bucket_start: window.bucket_start_ms(bucket),
                    p50: p[0],
                    p90: p[1],
                    p95: p[2],
                    p99: p[3],
                }
            })
            .collect()
    };
    let (entity_type, exact_entity) = (entity_type, exact_entity);
    let ttft_percentiles = group_percentiles(
        entity_type,
        exact_entity.as_deref(),
        "ttft",
        "r.ttft",
        " AND r.ttft IS NOT NULL",
    )
    .await;
    let latency_percentiles = group_percentiles(
        entity_type,
        exact_entity.as_deref(),
        "request_time",
        "r.request_time",
        "",
    )
    .await;

    // API Key 排行（窗口内调用数降序）：无过滤时闭桶走快照（api_key 行，
    // 按 id→名称归并），有过滤时整窗实时（无交叉行形态）。
    let has_filter = query.provider_id.is_some()
        || query.virtual_model_id.is_some()
        || query.model_id.is_some()
        || query.api_key.is_some();
    let mut key_map: std::collections::BTreeMap<String, i64> = Default::default();
    if has_filter {
        let mut params: Vec<sea_orm::Value> = vec![window.start.into(), window.end.into()];
        let mut where_sql = String::from("r.start_time >= ? AND r.start_time < ?");
        where_sql.push_str(&filter_sql);
        for v in filter_params.iter() {
            params.push(v.clone());
        }
        let sql = format!(
            "SELECT r.api_key_name AS name, COUNT(*) AS value FROM request r \
             WHERE {where_sql} GROUP BY r.api_key_name"
        );
        match db
            .query_all_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                sql,
                params,
            ))
            .await
        {
            Ok(rows) => {
                for row in &rows {
                    let name: String = row.try_get("", "name").unwrap_or_default();
                    let value: i64 = row.try_get("", "value").unwrap_or(0);
                    *key_map.entry(name).or_insert(0) += value;
                }
            }
            Err(e) => return response::db_error(e.to_string()),
        }
    } else {
        // 快照侧：api_key 行 calls 按 id 归并（闭桶；by_level 复用趋势段同一份）。
        let mut by_id: std::collections::BTreeMap<String, f64> = Default::default();
        for (level, frames) in &by_level {
            let rows = snap::snapshot_rows(
                db,
                *level,
                frames,
                snap::ENTITY_API_KEY,
                None,
                &[snap::metrics::CALLS],
            )
            .await
            .unwrap_or_default();
            for (_, _, entity, _, value) in rows {
                *by_id.entry(entity).or_insert(0.0) += value;
            }
        }
        let id_keys: Vec<String> = by_id.keys().cloned().collect();
        let names = match super::rank::resolve_names(db, "api_key", "id", "name", &id_keys).await {
            Ok(map) => map,
            Err(e) => return response::db_error(e),
        };
        for (key, value) in by_id {
            let Some(name) = names.get(&key) else {
                continue; // 已删 Key：快照闭桶贡献按生成语义丢弃
            };
            *key_map.entry(name.clone()).or_insert(0) += value.round() as i64;
        }
        // 兑底侧（孤儿名称行保留）。
        for &(s, e) in &coverage.live {
            let sql = format!(
                "SELECT r.api_key_name AS name, COUNT(*) AS value FROM request r \
                 WHERE r.start_time >= {s} AND r.start_time < {e}{filter_sql} GROUP BY r.api_key_name"
            );
            match db
                .query_all_raw(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    sql,
                    filter_params.to_vec(),
                ))
                .await
            {
                Ok(rows) => {
                    for row in &rows {
                        let name: String = row.try_get("", "name").unwrap_or_default();
                        let value: i64 = row.try_get("", "value").unwrap_or(0);
                        *key_map.entry(name).or_insert(0) += value;
                    }
                }
                Err(e) => return response::db_error(e.to_string()),
            }
        }
    }
    let mut api_key_rank: Vec<ApiKeyRankItem> = key_map
        .into_iter()
        .map(|(api_key_name, value)| ApiKeyRankItem {
            api_key_name,
            value,
        })
        .collect();
    api_key_rank.sort_by_key(|item| std::cmp::Reverse(item.value));

    (
        StatusCode::OK,
        Json(Response::success(InsightResponse {
            call_trend,
            failure_trend,
            failure_rate_trend,
            failure_reasons: reason_map
                .into_iter()
                .map(|(reason, count)| FailureReasonItem { reason, count })
                .collect(),
            ttft_percentiles,
            latency_percentiles,
            input_token_trend,
            output_token_trend,
            cache_hit_rate_trend: cache_rate_trend,
            output_tokens_per_sec_trend: output_per_sec_trend,
            api_key_rank,
            rpm_trend,
            tpm_trend,
            stream_ratio_trend,
        })),
    )
}

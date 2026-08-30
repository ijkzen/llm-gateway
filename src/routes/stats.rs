use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
};
use chrono::Datelike;
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::response::{self, Response};
use crate::state::AppState;

const HOUR_MS: i64 = 3_600_000;
const DAY_MS: i64 = 24 * HOUR_MS;
const TREND_BUCKETS: i64 = 24;

/// 图表趋势桶粒度（显式指定；缺省时按窗口长度回退推断）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Granularity {
    Hour,
    Day,
    Month,
    Year,
}

impl Granularity {
    fn parse(value: Option<&str>) -> Result<Option<Self>, &'static str> {
        match value {
            None => Ok(None),
            Some("hour") => Ok(Some(Self::Hour)),
            Some("day") => Ok(Some(Self::Day)),
            Some("month") => Ok(Some(Self::Month)),
            Some("year") => Ok(Some(Self::Year)),
            Some(_) => Err("不支持的 granularity，可选 hour/day/month/year"),
        }
    }
}

/// 客户端 UTC 偏移（分钟，东八区为 480）。仅在显式 granularity 下生效：
/// 小时/天桶按本地整点/午夜对齐，月/年桶按本地自然月/年归并。
fn parse_tz_offset(value: Option<i32>) -> i32 {
    value
        .filter(|offset| (-14 * 60..=14 * 60).contains(offset))
        .unwrap_or(0)
}

/// 月/年桶：把窗口内每个「本地日索引」归并到自然月/年（键 year 或 (year, month)），
/// 对窗口首日所在月/年到末日所在月/年补零，输出按桶起点（该月/年 1 日 0 点，本地）排序。
///
/// 返回 (桶起点列表, call 值序列, token 值序列)，三组长度一致；两序列共享同一组桶起点。
fn merge_natural_periods(
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
    let mut call_map: std::collections::BTreeMap<(i32, u32), i64> = std::collections::BTreeMap::new();
    let mut token_map: std::collections::BTreeMap<(i32, u32), i64> = std::collections::BTreeMap::new();
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
    let first_local = chrono::DateTime::from_timestamp_millis(window_start).map(|t| t.with_timezone(&tz));
    let last_local = chrono::DateTime::from_timestamp_millis(window_end - 1).map(|t| t.with_timezone(&tz));
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
    let to_bucket_start =
        |(y, m): (i32, u32)| -> Option<i64> {
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

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/summary", get(summary))
        .route("/charts", get(charts))
        .route("/provider-rank", get(provider_rank))
        .route("/virtual-model-rank", get(virtual_model_rank))
        .route("/provider-model-rank", get(provider_model_rank))
        .route("/virtual-model-member-rank", get(virtual_model_member_rank))
        .route("/model-metrics", get(model_metrics))
        .route("/provider-metrics", get(provider_metrics))
        .route("/virtual-model-metrics", get(virtual_model_metrics))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SummaryResponse {
    total_requests: i64,
    success_rate: f64,
    total_tokens: i64,
    cache_hit_rate: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TrendPoint {
    /// 小时桶起点（毫秒时间戳）。
    bucket_start: i64,
    value: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelValue {
    /// 实际服务的供应商名称（供应商已删除时为空串）。
    provider_name: String,
    model_id: String,
    value: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChartsResponse {
    call_trend: Vec<TrendPoint>,
    call_by_model: Vec<ModelValue>,
    token_trend: Vec<TrendPoint>,
    token_by_model: Vec<ModelValue>,
}

/// 全量历史累计：累计请求数、成功率、总计 token、加权缓存命中率。
async fn summary(State(state): State<AppState>) -> impl IntoResponse {
    let sql = r#"
        SELECT COUNT(*) AS total_requests,
               COALESCE(SUM(CASE WHEN success THEN 1 ELSE 0 END), 0) AS success_count,
               COALESCE(SUM(total_tokens), 0) AS total_tokens,
               COALESCE(SUM(input_tokens), 0) AS input_tokens,
               COALESCE(SUM(input_cache_tokens), 0) AS cache_tokens
        FROM request
    "#;
    let row = match state
        .db
        .query_one_raw(Statement::from_string(DbBackend::Sqlite, sql))
        .await
    {
        Ok(Some(row)) => row,
        Ok(None) => {
            return response::db_error("统计查询无结果".to_string());
        }
        Err(e) => return response::db_error(e.to_string()),
    };

    let total_requests: i64 = row.try_get("", "total_requests").unwrap_or(0);
    let success_count: i64 = row.try_get("", "success_count").unwrap_or(0);
    let total_tokens: i64 = row.try_get("", "total_tokens").unwrap_or(0);
    let input_tokens: i64 = row.try_get("", "input_tokens").unwrap_or(0);
    let cache_tokens: i64 = row.try_get("", "cache_tokens").unwrap_or(0);

    let success_rate = if total_requests > 0 {
        // 保留 5 位小数，与缓存命中率的口径一致。
        let rate = success_count as f64 / total_requests as f64;
        (rate * 100_000.0).round() / 100_000.0
    } else {
        0.0
    };
    let cache_hit_rate = if input_tokens > 0 {
        // 保留 5 位小数，与 request 表的 input_cache_rate 口径一致。
        let rate = cache_tokens as f64 / input_tokens as f64;
        (rate * 100_000.0).round() / 100_000.0
    } else {
        0.0
    };

    (
        StatusCode::OK,
        Json(Response::success(SummaryResponse {
            total_requests,
            success_rate,
            total_tokens,
            cache_hit_rate,
        })),
    )
}

/// 图表查询参数（全部可选；缺省回退「过去 24 小时」）。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChartsQuery {
    /// 窗口起点（毫秒时间戳，含）。
    start_time: Option<i64>,
    /// 窗口终点（毫秒时间戳，不含）。
    end_time: Option<i64>,
    /// 按供应商过滤（可选）。
    provider_id: Option<i32>,
    /// 按虚拟模型过滤（可选）。
    virtual_model_id: Option<i32>,
    /// 按模型 ID 过滤（可选；供应商侧真实模型 ID）。
    model_id: Option<String>,
    /// 桶粒度（hour/day/month/year）。缺省按窗口长度回退推断。
    granularity: Option<String>,
    /// 客户端 UTC 偏移（分钟）。仅与显式 granularity 搭配使用。
    tz_offset_minutes: Option<i32>,
}

/// 图表数据：调用/ token 的趋势 + 按上游模型的分布。
///
/// 支持可选 startTime/endTime（缺省回退过去 24 小时）与 providerId 过滤；
/// 显式 granularity + tzOffsetMinutes 时按客户端本地自然边界分桶：
/// 小时/天桶对齐本地整点/午夜，月/年桶按自然月/年归并；两者缺省时
/// 按窗口长度回退（≤48h 小时桶、≤62 天天桶、其余 30 天块）。
async fn charts(State(state): State<AppState>, Query(query): Query<ChartsQuery>) -> impl IntoResponse {
    let explicit_granularity = match Granularity::parse(query.granularity.as_deref()) {
        Ok(g) => g,
        Err(msg) => return response::bad_request(msg),
    };
    let tz_offset_minutes = parse_tz_offset(query.tz_offset_minutes);
    let tz = chrono::FixedOffset::east_opt(tz_offset_minutes * 60)
        .unwrap_or_else(|| chrono::FixedOffset::east_opt(0).expect("0 偏移恒有效"));

    let (window_start, window_end, bucket_ms, granularity) = match explicit_granularity {
        Some(g) => {
            let bucket_ms = match g {
                Granularity::Hour => HOUR_MS,
                Granularity::Day => DAY_MS,
                Granularity::Month | Granularity::Year => DAY_MS,
            };
            let (start, end) = match (query.start_time, query.end_time) {
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
            let (start, end, bucket_ms) = match (query.start_time, query.end_time) {
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

    // WHERE 公共条件：时间窗口（半开）+ 可选供应商过滤。
    let mut where_sql = String::from("r.start_time >= ? AND r.start_time < ?");
    let mut params: Vec<sea_orm::Value> = vec![window_start.into(), window_end.into()];
    if let Some(provider_id) = query.provider_id {
        where_sql.push_str(" AND r.provider_id = ?");
        params.push(provider_id.into());
    }
    if let Some(virtual_model_id) = query.virtual_model_id {
        where_sql.push_str(" AND r.virtual_model_id = ?");
        params.push(virtual_model_id.into());
    }
    if let Some(model_id) = query.model_id {
        where_sql.push_str(" AND r.model_id = ?");
        params.push(model_id.into());
    }

    // 月/年粒度：SQL 按本地日桶聚合，Rust 侧再归并自然月/年。
    let offset_ms = i64::from(tz_offset_minutes) * 60_000;
    let bucket_expr = if matches!(granularity, Granularity::Month | Granularity::Year) {
        format!("(r.start_time + {offset_ms}) / {DAY_MS}")
    } else {
        format!("(r.start_time + {offset_ms}) / {bucket_ms}")
    };

    let trend_sql = |value_expr: &str| {
        format!(
            "SELECT {bucket_expr} AS bucket, {value_expr} AS value \
             FROM request r WHERE {where_sql} GROUP BY bucket"
        )
    };
    let model_sql = |value_expr: &str| {
        format!(
            "SELECT COALESCE(p.name, '') AS provider_name, r.model_id, {value_expr} AS value \
             FROM request r LEFT JOIN provider p ON p.id = r.provider_id \
             WHERE {where_sql} GROUP BY p.name, r.model_id"
        )
    };

    let db = &state.db;
    let (call_rows, token_rows, call_model_rows, token_model_rows) = match tokio::try_join!(
        db.query_all_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            trend_sql("COUNT(*)"),
            params.clone(),
        )),
        db.query_all_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            trend_sql("COALESCE(SUM(r.total_tokens), 0)"),
            params.clone(),
        )),
        db.query_all_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            model_sql("COUNT(*)"),
            params.clone(),
        )),
        db.query_all_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            model_sql("COALESCE(SUM(r.total_tokens), 0)"),
            params.clone(),
        )),
    ) {
        Ok(rows) => rows,
        Err(e) => return response::db_error(e.to_string()),
    };

    // 桶填充：从窗口起点所在桶到终点所在桶，按桶粒度对齐。
    let (call_trend, token_trend) = if matches!(granularity, Granularity::Month | Granularity::Year) {
        // 月/年：先按本地日索引收集，再归并自然月/年（含窗口内补零）。
        let collect = |rows: &[sea_orm::QueryResult]| {
            rows.iter()
                .filter_map(|row| {
                    let bucket: i64 = row.try_get("", "bucket").ok()?;
                    let value: i64 = row.try_get("", "value").ok()?;
                    Some((bucket, value))
                })
                .collect::<Vec<_>>()
        };
        let (starts, call_values, token_values) = merge_natural_periods(
            &collect(&call_rows),
            &collect(&token_rows),
            window_start,
            window_end,
            tz,
            matches!(granularity, Granularity::Month),
        );
        let call_trend = starts
            .iter()
            .zip(call_values)
            .map(|(&bucket_start, value)| TrendPoint { bucket_start, value })
            .collect::<Vec<_>>();
        let token_trend = starts
            .iter()
            .zip(token_values)
            .map(|(&bucket_start, value)| TrendPoint { bucket_start, value })
            .collect::<Vec<_>>();
        (call_trend, token_trend)
    } else {
        // 小时/天：直接按桶索引区间补零（桶对齐本地边界，tz 偏移已并入表达式）。
        let offset_ms = i64::from(tz_offset_minutes) * 60_000;
        let first_bucket = (window_start + offset_ms) / bucket_ms;
        let last_bucket = (window_end - 1 + offset_ms).max(window_start + offset_ms) / bucket_ms;
        let fill_trend = |map: &std::collections::HashMap<i64, i64>| {
            (first_bucket..=last_bucket)
                .map(|bucket| TrendPoint {
                    bucket_start: bucket * bucket_ms - offset_ms,
                    value: map.get(&bucket).copied().unwrap_or(0),
                })
                .collect::<Vec<_>>()
        };
        let mut call_counts = std::collections::HashMap::new();
        for row in &call_rows {
            let bucket: i64 = row.try_get("", "bucket").unwrap_or(0);
            let value: i64 = row.try_get("", "value").unwrap_or(0);
            call_counts.insert(bucket, value);
        }
        let mut token_sums = std::collections::HashMap::new();
        for row in &token_rows {
            let bucket: i64 = row.try_get("", "bucket").unwrap_or(0);
            let value: i64 = row.try_get("", "value").unwrap_or(0);
            token_sums.insert(bucket, value);
        }
        (fill_trend(&call_counts), fill_trend(&token_sums))
    };

    let to_model_values = |rows: Vec<sea_orm::QueryResult>| {
        rows.iter()
            .map(|row| ModelValue {
                provider_name: row.try_get("", "provider_name").unwrap_or_default(),
                model_id: row.try_get("", "model_id").unwrap_or_default(),
                value: row.try_get("", "value").unwrap_or(0),
            })
            .collect::<Vec<_>>()
    };

    (
        StatusCode::OK,
        Json(Response::success(ChartsResponse {
            call_trend,
            call_by_model: to_model_values(call_model_rows),
            token_trend,
            token_by_model: to_model_values(token_model_rows),
        })),
    )
}

/// 赛马排序指标。
///
/// SQLite 的 SUM(INTEGER) 结果为 INTEGER、SUM(REAL)/AVG 为 REAL：整数类
/// 指标（totalTokens/requestCount）用 i64 读取，其余用 f64，避免 sqlx
/// 严格类型转换失败。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RankSortKey {
    TotalTokens,
    RequestCount,
    Ttft,
    RequestTime,
    Tps,
    CacheHitRate,
}

/// 赛马查询公共参数（供应商/虚拟模型共用）。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RankQuery {
    /// 排序指标：totalTokens | requestCount | ttft | requestTime | tps | cacheHitRate。
    sort_by: Option<String>,
    /// 排序方向：asc | desc（缺省按指标默认方向）。
    sort_order: Option<String>,
    /// 窗口起点（毫秒时间戳，含）。
    start_time: Option<i64>,
    /// 窗口终点（毫秒时间戳，不含）。
    end_time: Option<i64>,
    /// 按供应商过滤（可选；provider_model_rank 使用）。
    provider_id: Option<i32>,
    /// 按虚拟模型过滤（可选；virtual_model_member_rank 使用）。
    virtual_model_id: Option<i32>,
}

/// 解析排序指标白名单；非法值返回 None（调用方转 400）。
fn parse_sort_key(sort_by: Option<&str>) -> Option<RankSortKey> {
    match sort_by {
        None | Some("totalTokens") => Some(RankSortKey::TotalTokens),
        Some("requestCount") => Some(RankSortKey::RequestCount),
        Some("ttft") => Some(RankSortKey::Ttft),
        Some("requestTime") => Some(RankSortKey::RequestTime),
        Some("tps") => Some(RankSortKey::Tps),
        Some("cacheHitRate") => Some(RankSortKey::CacheHitRate),
        _ => None,
    }
}

/// 排序方向：显式 asc/desc 优先；缺省时耗时类指标升序（越快越靠前），其余降序。
fn sort_direction(sort_order: Option<&str>, sort_key: RankSortKey) -> &'static str {
    match sort_order {
        Some("asc") => "ASC",
        Some("desc") => "DESC",
        _ => match sort_key {
            RankSortKey::Ttft | RankSortKey::RequestTime => "ASC",
            _ => "DESC",
        },
    }
}

/// 按排序指标与方向对聚合行排序（Rust 侧；NULL 聚合已由 SQL 归一为 0）。
fn sort_rank_rows<T>(rows: &mut [T], is_asc: bool, value_of: impl Fn(&T) -> f64) {
    rows.sort_by(|a, b| {
        let cmp = value_of(a)
            .partial_cmp(&value_of(b))
            .unwrap_or(std::cmp::Ordering::Equal);
        if is_asc { cmp } else { cmp.reverse() }
    });
}

/// 赛马聚合的 6 个指标列（value_expr 与字段读取共用，供应商/虚拟模型维度
/// 仅 SELECT 的名称列、JOIN 与 GROUP BY 不同）。
const RANK_METRIC_SQL: &str = r#"
       COUNT(*) AS request_count,
       COALESCE(SUM(r.total_tokens), 0) AS total_tokens,
       AVG(r.ttft) AS ttft,
       AVG(r.request_time) AS request_time,
       CASE
           WHEN SUM(CASE WHEN r.tps > 0 AND r.output_tokens > 0 THEN r.output_tokens / r.tps ELSE 0 END) > 0
           THEN COALESCE(SUM(r.output_tokens), 0) / SUM(CASE WHEN r.tps > 0 AND r.output_tokens > 0 THEN r.output_tokens / r.tps ELSE 0 END)
           ELSE 0
       END AS tps,
       CASE
           WHEN SUM(r.input_tokens) > 0
           THEN ROUND(1.0 * SUM(r.input_cache_tokens) / SUM(r.input_tokens), 5)
           ELSE 0
       END AS cache_hit_rate
"#;

/// 从查询参数解析排序指标与方向；参数缺失/非法返回错误响应。
/// T 为调用方成功响应的 data 类型（错误响应的 data 为空，仅用于类型对齐）。
fn parse_rank_query<T>(
    query: &RankQuery,
) -> Result<(RankSortKey, &'static str, i64, i64), response::ErrorResponse<T>> {
    let sort_key = parse_sort_key(query.sort_by.as_deref())
        .ok_or_else(|| response::bad_request("sortBy 参数非法"))?;
    let (Some(start), Some(end)) = (query.start_time, query.end_time) else {
        return Err(response::bad_request("缺少 startTime / endTime 参数"));
    };
    if end <= start {
        return Err(response::bad_request("endTime 必须大于 startTime"));
    }
    let order_dir = sort_direction(query.sort_order.as_deref(), sort_key);
    Ok((sort_key, order_dir, start, end))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderRankItem {
    /// 实际服务的供应商 ID（聚合维度；已删除供应商仍保留原始 id）。
    provider_id: i32,
    /// 实际服务的供应商名称（供应商已删除时为空串）。
    provider_name: String,
    /// 成功请求数。
    request_count: i64,
    /// 总计 token（成功请求的 total_tokens 合计）。
    total_tokens: i64,
    /// 流式请求（stream=1 且 ttft 非空）首 token 耗时均值（毫秒）。
    ttft: f64,
    /// 平均请求耗时（毫秒，成功请求 request_time 均值）。
    request_time: f64,
    /// TPS：Σ输出 token ÷ Σ网络耗时（耗时按 output_tokens/tps 反推，
    /// 仅计入 tps>0 且 output_tokens>0 的行）；分母为 0 时记 0。
    tps: f64,
    /// 缓存命中率：Σ输入缓存 token ÷ Σ输入 token（加权，无输入 token 时记 0）。
    cache_hit_rate: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderRankResponse {
    start_time: i64,
    end_time: i64,
    items: Vec<ProviderRankItem>,
}

/// 供应商维度赛马：按时间窗口聚合 request 表，一次查询返回全部供应商的
/// 6 个指标，按排序参数（缺省 totalTokens 降序）排序。
async fn provider_rank(
    State(state): State<AppState>,
    Query(query): Query<RankQuery>,
) -> Result<Json<Response<ProviderRankResponse>>, response::ErrorResponse<ProviderRankResponse>> {
    let (sort_key, order_dir, start, end) = match parse_rank_query(&query) {
        Ok(v) => v,
        Err(e) => return Err(e),
    };
    let db = &state.db;

    // 单查询聚合全部 6 个指标：仅成功请求 + start_time 半开窗口。
    // 按 r.provider_id 分组（id 才是真实聚合维度，name 仅展示）。
    let sql = format!(
        "SELECT r.provider_id AS provider_id, COALESCE(p.name, '') AS provider_name,{RANK_METRIC_SQL} \
         FROM request r LEFT JOIN provider p ON p.id = r.provider_id \
         WHERE r.success = 1 AND r.start_time >= ? AND r.start_time < ? \
         GROUP BY r.provider_id"
    );

    let rows = match db
        .query_all_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [start.into(), end.into()],
        ))
        .await
    {
        Ok(rows) => rows,
        Err(e) => return Err(response::db_error(e.to_string())),
    };

    let mut items = rows
        .iter()
        .map(|row| ProviderRankItem {
            provider_id: row.try_get::<i32>("", "provider_id").unwrap_or(0),
            provider_name: row.try_get("", "provider_name").unwrap_or_default(),
            request_count: row.try_get::<i64>("", "request_count").unwrap_or(0),
            total_tokens: row.try_get::<i64>("", "total_tokens").unwrap_or(0),
            ttft: row.try_get::<f64>("", "ttft").unwrap_or(0.0),
            request_time: row.try_get::<f64>("", "request_time").unwrap_or(0.0),
            tps: row.try_get::<f64>("", "tps").unwrap_or(0.0),
            cache_hit_rate: row.try_get::<f64>("", "cache_hit_rate").unwrap_or(0.0),
        })
        .collect::<Vec<_>>();

    let is_asc = order_dir == "ASC";
    let value_of = |item: &ProviderRankItem| -> f64 {
        match sort_key {
            RankSortKey::TotalTokens => item.total_tokens as f64,
            RankSortKey::RequestCount => item.request_count as f64,
            RankSortKey::Ttft => item.ttft,
            RankSortKey::RequestTime => item.request_time,
            RankSortKey::Tps => item.tps,
            RankSortKey::CacheHitRate => item.cache_hit_rate,
        }
    };
    sort_rank_rows(&mut items, is_asc, value_of);

    Ok(Json(Response::success(ProviderRankResponse {
        start_time: start,
        end_time: end,
        items,
    })))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VirtualModelRankItem {
    /// 虚拟模型 ID（聚合维度；已删除虚拟模型仍保留原始 id）。
    virtual_model_id: i32,
    /// 虚拟模型对外 ID（虚拟模型已删除时为空串）。
    virtual_model_display_id: String,
    /// 成功请求数。
    request_count: i64,
    /// 总计 token（成功请求的 total_tokens 合计）。
    total_tokens: i64,
    /// 流式请求（stream=1 且 ttft 非空）首 token 耗时均值（毫秒）。
    ttft: f64,
    /// 平均请求耗时（毫秒，成功请求 request_time 均值）。
    request_time: f64,
    /// TPS：Σ输出 token ÷ Σ网络耗时（耗时按 output_tokens/tps 反推，
    /// 仅计入 tps>0 且 output_tokens>0 的行）；分母为 0 时记 0。
    tps: f64,
    /// 缓存命中率：Σ输入缓存 token ÷ Σ输入 token（加权，无输入 token 时记 0）。
    cache_hit_rate: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VirtualModelRankResponse {
    start_time: i64,
    end_time: i64,
    items: Vec<VirtualModelRankItem>,
}

/// 虚拟模型维度赛马：规格与供应商赛马完全一致（6 指标 + 排序 + 时间窗口），
/// 仅聚合维度不同——按 request.virtual_model_id 分组，JOIN virtual_model 出
/// display_id；虚拟模型已删除时 LEFT JOIN 得 NULL，显示空串。
async fn virtual_model_rank(
    State(state): State<AppState>,
    Query(query): Query<RankQuery>,
) -> Result<Json<Response<VirtualModelRankResponse>>, response::ErrorResponse<VirtualModelRankResponse>> {
    let (sort_key, order_dir, start, end) = match parse_rank_query(&query) {
        Ok(v) => v,
        Err(e) => return Err(e),
    };
    let db = &state.db;

    // 按 id 分组（同一 display_id 的虚拟模型也各自成行），JOIN 出 display_id。
    let sql = format!(
        "SELECT r.virtual_model_id AS virtual_model_id, \
                COALESCE(vm.display_id, '') AS virtual_model_display_id,{RANK_METRIC_SQL} \
         FROM request r LEFT JOIN virtual_model vm ON vm.virtual_model_id = r.virtual_model_id \
         WHERE r.success = 1 AND r.start_time >= ? AND r.start_time < ? \
         GROUP BY r.virtual_model_id"
    );

    let rows = match db
        .query_all_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [start.into(), end.into()],
        ))
        .await
    {
        Ok(rows) => rows,
        Err(e) => return Err(response::db_error(e.to_string())),
    };

    let mut items = rows
        .iter()
        .map(|row| VirtualModelRankItem {
            virtual_model_id: row.try_get::<i32>("", "virtual_model_id").unwrap_or(0),
            virtual_model_display_id: row.try_get("", "virtual_model_display_id").unwrap_or_default(),
            request_count: row.try_get::<i64>("", "request_count").unwrap_or(0),
            total_tokens: row.try_get::<i64>("", "total_tokens").unwrap_or(0),
            ttft: row.try_get::<f64>("", "ttft").unwrap_or(0.0),
            request_time: row.try_get::<f64>("", "request_time").unwrap_or(0.0),
            tps: row.try_get::<f64>("", "tps").unwrap_or(0.0),
            cache_hit_rate: row.try_get::<f64>("", "cache_hit_rate").unwrap_or(0.0),
        })
        .collect::<Vec<_>>();

    let is_asc = order_dir == "ASC";
    let value_of = |item: &VirtualModelRankItem| -> f64 {
        match sort_key {
            RankSortKey::TotalTokens => item.total_tokens as f64,
            RankSortKey::RequestCount => item.request_count as f64,
            RankSortKey::Ttft => item.ttft,
            RankSortKey::RequestTime => item.request_time,
            RankSortKey::Tps => item.tps,
            RankSortKey::CacheHitRate => item.cache_hit_rate,
        }
    };
    sort_rank_rows(&mut items, is_asc, value_of);

    Ok(Json(Response::success(VirtualModelRankResponse {
        start_time: start,
        end_time: end,
        items,
    })))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderModelRankItem {
    /// 实际服务的供应商 ID。
    provider_id: i32,
    /// 实际服务的供应商名称（供应商已删除时为空串）。
    provider_name: String,
    /// 模型 ID（供应商侧真实 ID；provider_model 行已删时退化为 request 里的原始串）。
    model_id: String,
    /// 成功请求数。
    request_count: i64,
    /// 总计 token（成功请求的 total_tokens 合计）。
    total_tokens: i64,
    /// 流式请求（stream=1 且 ttft 非空）首 token 耗时均值（毫秒）。
    ttft: f64,
    /// 平均请求耗时（毫秒，成功请求 request_time 均值）。
    request_time: f64,
    /// TPS：Σ输出 token ÷ Σ网络耗时（耗时按 output_tokens/tps 反推，
    /// 仅计入 tps>0 且 output_tokens>0 的行）；分母为 0 时记 0。
    tps: f64,
    /// 缓存命中率：Σ输入缓存 token ÷ Σ输入 token（加权，无输入 token 时记 0）。
    cache_hit_rate: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderModelRankResponse {
    start_time: i64,
    end_time: i64,
    items: Vec<ProviderModelRankItem>,
}

/// 供应商模型平铺赛马：规格与供应商/虚拟模型赛马完全一致（6 指标 + 排序 +
/// 时间窗口），行的含义 = 供应商的每个模型。按 (provider_id, model_id) 分组
/// （按 id 而非名称，避免不同供应商的相同模型 ID 被合并），JOIN provider 出
/// 供应商名、LEFT JOIN provider_model 兜底模型名（模型行已删时退化为 request
/// 里的原始 model_id）。
async fn provider_model_rank(
    State(state): State<AppState>,
    Query(query): Query<RankQuery>,
) -> Result<Json<Response<ProviderModelRankResponse>>, response::ErrorResponse<ProviderModelRankResponse>> {
    let (sort_key, order_dir, start, end) = match parse_rank_query(&query) {
        Ok(v) => v,
        Err(e) => return Err(e),
    };
    let db = &state.db;

    // 可选按供应商过滤（二级页用）：有 providerId 时只聚合该供应商内部模型。
    let mut where_sql = String::from("r.success = 1 AND r.start_time >= ? AND r.start_time < ?");
    let mut params: Vec<sea_orm::Value> = vec![start.into(), end.into()];
    if let Some(provider_id) = query.provider_id {
        where_sql.push_str(" AND r.provider_id = ?");
        params.push(provider_id.into());
    }

    let sql = format!(
        "SELECT r.provider_id AS provider_id, COALESCE(p.name, '') AS provider_name, \
                COALESCE(pm.provider_model_id, r.model_id) AS model_id,{RANK_METRIC_SQL} \
         FROM request r \
         LEFT JOIN provider p ON p.id = r.provider_id \
         LEFT JOIN provider_model pm ON pm.provider_id = r.provider_id AND pm.provider_model_id = r.model_id \
         WHERE {where_sql} \
         GROUP BY r.provider_id, r.model_id"
    );

    let rows = match db
        .query_all_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            params,
        ))
        .await
    {
        Ok(rows) => rows,
        Err(e) => return Err(response::db_error(e.to_string())),
    };

    let mut items = rows
        .iter()
        .map(|row| ProviderModelRankItem {
            provider_id: row.try_get::<i32>("", "provider_id").unwrap_or(0),
            provider_name: row.try_get("", "provider_name").unwrap_or_default(),
            model_id: row.try_get("", "model_id").unwrap_or_default(),
            request_count: row.try_get::<i64>("", "request_count").unwrap_or(0),
            total_tokens: row.try_get::<i64>("", "total_tokens").unwrap_or(0),
            ttft: row.try_get::<f64>("", "ttft").unwrap_or(0.0),
            request_time: row.try_get::<f64>("", "request_time").unwrap_or(0.0),
            tps: row.try_get::<f64>("", "tps").unwrap_or(0.0),
            cache_hit_rate: row.try_get::<f64>("", "cache_hit_rate").unwrap_or(0.0),
        })
        .collect::<Vec<_>>();

    let is_asc = order_dir == "ASC";
    let value_of = |item: &ProviderModelRankItem| -> f64 {
        match sort_key {
            RankSortKey::TotalTokens => item.total_tokens as f64,
            RankSortKey::RequestCount => item.request_count as f64,
            RankSortKey::Ttft => item.ttft,
            RankSortKey::RequestTime => item.request_time,
            RankSortKey::Tps => item.tps,
            RankSortKey::CacheHitRate => item.cache_hit_rate,
        }
    };
    sort_rank_rows(&mut items, is_asc, value_of);

    Ok(Json(Response::success(ProviderModelRankResponse {
        start_time: start,
        end_time: end,
        items,
    })))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VirtualModelMemberRankItem {
    /// 成员所属供应商 ID。
    provider_id: i32,
    /// 成员所属供应商名称（供应商已删除时为空串）。
    provider_name: String,
    /// 成员模型 ID（供应商侧真实 ID）。
    model_id: String,
    /// 成员是否启用（virtual_model_item.enable；停用成员可正常展示但指标多为 0）。
    member_enable: bool,
    /// 成功请求数（该虚拟模型下实际服务过该成员的行数）。
    request_count: i64,
    /// 总计 token（成功请求的 total_tokens 合计）。
    total_tokens: i64,
    /// 流式请求（stream=1 且 ttft 非空）首 token 耗时均值（毫秒）。
    ttft: f64,
    /// 平均请求耗时（毫秒，成功请求 request_time 均值）。
    request_time: f64,
    /// TPS：Σ输出 token ÷ Σ网络耗时（耗时按 output_tokens/tps 反推，
    /// 仅计入 tps>0 且 output_tokens>0 的行）；分母为 0 时记 0。
    tps: f64,
    /// 缓存命中率：Σ输入缓存 token ÷ Σ输入 token（加权，无输入 token 时记 0）。
    cache_hit_rate: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VirtualModelMemberRankResponse {
    start_time: i64,
    end_time: i64,
    items: Vec<VirtualModelMemberRankItem>,
}

/// 虚拟模型成员模型排行：以成员配置表（virtual_model_item）为左表反查——
/// 展示该虚拟模型配置的全部成员（即使某成员在窗口内无流量，指标为 0），
/// 指标从 request 聚合（该虚拟模型实际服务该成员的行，仅 success=1）。
///
/// 关联键说明：request.model_id 存的是 provider_model.provider_model_id
/// （字符串），聚合子查询按 (provider_id, model_id) 分组后与
/// (pm.provider_id, pm.provider_model_id) 关联。
async fn virtual_model_member_rank(
    State(state): State<AppState>,
    Query(query): Query<RankQuery>,
) -> Result<Json<Response<VirtualModelMemberRankResponse>>, response::ErrorResponse<VirtualModelMemberRankResponse>> {
    let (sort_key, order_dir, start, end) = match parse_rank_query(&query) {
        Ok(v) => v,
        Err(e) => return Err(e),
    };
    let Some(virtual_model_id) = query.virtual_model_id else {
        return Err(response::bad_request("缺少 virtualModelId 参数"));
    };
    let db = &state.db;

    // 聚合子查询：该虚拟模型下实际服务的成员（按 provider_id + model_id 分组）。
    // 6 指标表达式与 RANK_METRIC_SQL 同口径，但需带上关联键列。
    let sql = "SELECT pm.provider_id AS provider_id, COALESCE(p.name, '') AS provider_name, \
                pm.provider_model_id AS model_id, \
                vmi.enable AS member_enable, \
                COALESCE(agg.request_count, 0) AS request_count, \
                COALESCE(agg.total_tokens, 0) AS total_tokens, \
                COALESCE(agg.ttft, 0) AS ttft, \
                COALESCE(agg.request_time, 0) AS request_time, \
                COALESCE(agg.tps, 0) AS tps, \
                COALESCE(agg.cache_hit_rate, 0) AS cache_hit_rate \
         FROM virtual_model_item vmi \
         JOIN provider_model pm ON pm.model_id = vmi.model_id \
         LEFT JOIN provider p ON p.id = pm.provider_id \
         LEFT JOIN ( \
             SELECT r.provider_id, r.model_id AS provider_model_id, \
                    COUNT(*) AS request_count, \
                    COALESCE(SUM(r.total_tokens), 0) AS total_tokens, \
                    AVG(r.ttft) AS ttft, \
                    AVG(r.request_time) AS request_time, \
                    CASE \
                        WHEN SUM(CASE WHEN r.tps > 0 AND r.output_tokens > 0 THEN r.output_tokens / r.tps ELSE 0 END) > 0 \
                        THEN COALESCE(SUM(r.output_tokens), 0) / SUM(CASE WHEN r.tps > 0 AND r.output_tokens > 0 THEN r.output_tokens / r.tps ELSE 0 END) \
                        ELSE 0 \
                    END AS tps, \
                    CASE \
                        WHEN SUM(r.input_tokens) > 0 \
                        THEN 1.0 * SUM(r.input_cache_tokens) / SUM(r.input_tokens) \
                        ELSE 0 \
                    END AS cache_hit_rate \
             FROM request r \
             WHERE r.success = 1 AND r.virtual_model_id = ? AND r.start_time >= ? AND r.start_time < ? \
             GROUP BY r.provider_id, r.model_id \
         ) agg ON agg.provider_id = pm.provider_id AND agg.provider_model_id = pm.provider_model_id \
         WHERE vmi.virtual_model_id = ?";

    let rows = match db
        .query_all_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [
                virtual_model_id.into(),
                start.into(),
                end.into(),
                virtual_model_id.into(),
            ],
        ))
        .await
    {
        Ok(rows) => rows,
        Err(e) => return Err(response::db_error(e.to_string())),
    };

    let mut items = rows
        .iter()
        .map(|row| VirtualModelMemberRankItem {
            provider_id: row.try_get::<i32>("", "provider_id").unwrap_or(0),
            provider_name: row.try_get("", "provider_name").unwrap_or_default(),
            model_id: row.try_get("", "model_id").unwrap_or_default(),
            member_enable: row.try_get::<bool>("", "member_enable").unwrap_or(true),
            request_count: row.try_get::<i64>("", "request_count").unwrap_or(0),
            total_tokens: row.try_get::<i64>("", "total_tokens").unwrap_or(0),
            ttft: row.try_get::<f64>("", "ttft").unwrap_or(0.0),
            request_time: row.try_get::<f64>("", "request_time").unwrap_or(0.0),
            tps: row.try_get::<f64>("", "tps").unwrap_or(0.0),
            cache_hit_rate: row.try_get::<f64>("", "cache_hit_rate").unwrap_or(0.0),
        })
        .collect::<Vec<_>>();

    let is_asc = order_dir == "ASC";
    let value_of = |item: &VirtualModelMemberRankItem| -> f64 {
        match sort_key {
            RankSortKey::TotalTokens => item.total_tokens as f64,
            RankSortKey::RequestCount => item.request_count as f64,
            RankSortKey::Ttft => item.ttft,
            RankSortKey::RequestTime => item.request_time,
            RankSortKey::Tps => item.tps,
            RankSortKey::CacheHitRate => item.cache_hit_rate,
        }
    };
    // 无流量成员（request_count=0）始终排最后，避免升序时 0 值抢前；
    // 有流量成员组内按指标升/降序。
    items.sort_by(|a, b| {
        let a_has_traffic = a.request_count > 0;
        let b_has_traffic = b.request_count > 0;
        match (a_has_traffic, b_has_traffic) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => {
                let cmp = value_of(a)
                    .partial_cmp(&value_of(b))
                    .unwrap_or(std::cmp::Ordering::Equal);
                if is_asc { cmp } else { cmp.reverse() }
            }
        }
    });

    Ok(Json(Response::success(VirtualModelMemberRankResponse {
        start_time: start,
        end_time: end,
        items,
    })))
}

/// 模型详情查询参数：providerId + modelId + 时间窗口。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelMetricsQuery {
    /// 供应商 ID（必填）。
    provider_id: Option<i32>,
    /// 模型 ID（必填；供应商侧真实模型 ID）。
    model_id: Option<String>,
    /// 窗口起点（毫秒时间戳，含）。
    start_time: Option<i64>,
    /// 窗口终点（毫秒时间戳，不含）。
    end_time: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelMetricsResponse {
    /// 供应商 ID。
    provider_id: i32,
    /// 供应商名称（供应商已删除时为空串）。
    provider_name: String,
    /// 模型 ID（供应商侧真实 ID）。
    model_id: String,
    /// 成功请求数。
    request_count: i64,
    /// 总计 token（成功请求的 total_tokens 合计）。
    total_tokens: i64,
    /// 流式请求（stream=1 且 ttft 非空）首 token 耗时均值（毫秒）。
    ttft: f64,
    /// 平均请求耗时（毫秒，成功请求 request_time 均值）。
    request_time: f64,
    /// TPS：Σ输出 token ÷ Σ网络耗时（耗时按 output_tokens/tps 反推，
    /// 仅计入 tps>0 且 output_tokens>0 的行）；分母为 0 时记 0。
    tps: f64,
    /// 缓存命中率：Σ输入缓存 token ÷ Σ输入 token（加权，无输入 token 时记 0）。
    cache_hit_rate: f64,
}

/// 单模型指标：按 (provider_id, model_id) 过滤聚合 6 指标，返回单行。
/// 供模型详情三级页的指标卡片使用。
async fn model_metrics(
    State(state): State<AppState>,
    Query(query): Query<ModelMetricsQuery>,
) -> impl IntoResponse {
    let (Some(provider_id), Some(model_id)) = (query.provider_id, query.model_id) else {
        return response::bad_request("缺少 providerId / modelId 参数");
    };
    let (Some(start), Some(end)) = (query.start_time, query.end_time) else {
        return response::bad_request("缺少 startTime / endTime 参数");
    };
    if end <= start {
        return response::bad_request("endTime 必须大于 startTime");
    }
    let db = &state.db;

    // 单行聚合 6 指标（无 GROUP BY），JOIN provider 出名称。
    let sql = format!(
        "SELECT COALESCE(p.name, '') AS provider_name,{RANK_METRIC_SQL} \
         FROM request r LEFT JOIN provider p ON p.id = r.provider_id \
         WHERE r.success = 1 AND r.provider_id = ? AND r.model_id = ? \
           AND r.start_time >= ? AND r.start_time < ?"
    );

    let row = match db
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [
                provider_id.into(),
                model_id.clone().into(),
                start.into(),
                end.into(),
            ],
        ))
        .await
    {
        Ok(Some(row)) => row,
        Ok(None) => return response::db_error("模型指标查询无结果".to_string()),
        Err(e) => return response::db_error(e.to_string()),
    };

    (
        StatusCode::OK,
        Json(Response::success(ModelMetricsResponse {
            provider_id,
            provider_name: row.try_get("", "provider_name").unwrap_or_default(),
            model_id,
            request_count: row.try_get::<i64>("", "request_count").unwrap_or(0),
            total_tokens: row.try_get::<i64>("", "total_tokens").unwrap_or(0),
            ttft: row.try_get::<f64>("", "ttft").unwrap_or(0.0),
            request_time: row.try_get::<f64>("", "request_time").unwrap_or(0.0),
            tps: row.try_get::<f64>("", "tps").unwrap_or(0.0),
            cache_hit_rate: row.try_get::<f64>("", "cache_hit_rate").unwrap_or(0.0),
        })),
    )
}

/// 供应商指标查询参数：providerId + 时间窗口。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderMetricsQuery {
    /// 供应商 ID（必填）。
    provider_id: Option<i32>,
    /// 窗口起点（毫秒时间戳，含）。
    start_time: Option<i64>,
    /// 窗口终点（毫秒时间戳，不含）。
    end_time: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderMetricsResponse {
    /// 供应商 ID。
    provider_id: i32,
    /// 供应商名称（供应商已删除时为空串）。
    provider_name: String,
    /// 成功请求数。
    request_count: i64,
    /// 总计 token（成功请求的 total_tokens 合计）。
    total_tokens: i64,
    /// 流式请求（stream=1 且 ttft 非空）首 token 耗时均值（毫秒）。
    ttft: f64,
    /// 平均请求耗时（毫秒，成功请求 request_time 均值）。
    request_time: f64,
    /// TPS：Σ输出 token ÷ Σ网络耗时（耗时按 output_tokens/tps 反推）。
    tps: f64,
    /// 缓存命中率：Σ输入缓存 token ÷ Σ输入 token（加权，无输入 token 时记 0）。
    cache_hit_rate: f64,
}

/// 供应商级 6 指标聚合：按 provider_id 过滤返回单行，供二级页顶部指标卡。
async fn provider_metrics(
    State(state): State<AppState>,
    Query(query): Query<ProviderMetricsQuery>,
) -> impl IntoResponse {
    let Some(provider_id) = query.provider_id else {
        return response::bad_request("缺少 providerId 参数");
    };
    let (Some(start), Some(end)) = (query.start_time, query.end_time) else {
        return response::bad_request("缺少 startTime / endTime 参数");
    };
    if end <= start {
        return response::bad_request("endTime 必须大于 startTime");
    }
    let db = &state.db;

    let sql = format!(
        "SELECT COALESCE(p.name, '') AS provider_name,{RANK_METRIC_SQL} \
         FROM request r LEFT JOIN provider p ON p.id = r.provider_id \
         WHERE r.success = 1 AND r.provider_id = ? \
           AND r.start_time >= ? AND r.start_time < ?"
    );

    let row = match db
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [provider_id.into(), start.into(), end.into()],
        ))
        .await
    {
        Ok(Some(row)) => row,
        Ok(None) => return response::db_error("供应商指标查询无结果".to_string()),
        Err(e) => return response::db_error(e.to_string()),
    };

    (
        StatusCode::OK,
        Json(Response::success(ProviderMetricsResponse {
            provider_id,
            provider_name: row.try_get("", "provider_name").unwrap_or_default(),
            request_count: row.try_get::<i64>("", "request_count").unwrap_or(0),
            total_tokens: row.try_get::<i64>("", "total_tokens").unwrap_or(0),
            ttft: row.try_get::<f64>("", "ttft").unwrap_or(0.0),
            request_time: row.try_get::<f64>("", "request_time").unwrap_or(0.0),
            tps: row.try_get::<f64>("", "tps").unwrap_or(0.0),
            cache_hit_rate: row.try_get::<f64>("", "cache_hit_rate").unwrap_or(0.0),
        })),
    )
}

/// 虚拟模型指标查询参数：virtualModelId + 时间窗口。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VirtualModelMetricsQuery {
    /// 虚拟模型 ID（必填）。
    virtual_model_id: Option<i32>,
    /// 窗口起点（毫秒时间戳，含）。
    start_time: Option<i64>,
    /// 窗口终点（毫秒时间戳，不含）。
    end_time: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VirtualModelMetricsResponse {
    /// 虚拟模型 ID。
    virtual_model_id: i32,
    /// 虚拟模型对外 ID（虚拟模型已删除时为空串）。
    virtual_model_display_id: String,
    /// 成功请求数。
    request_count: i64,
    /// 总计 token（成功请求的 total_tokens 合计）。
    total_tokens: i64,
    /// 流式请求（stream=1 且 ttft 非空）首 token 耗时均值（毫秒）。
    ttft: f64,
    /// 平均请求耗时（毫秒，成功请求 request_time 均值）。
    request_time: f64,
    /// TPS：Σ输出 token ÷ Σ网络耗时（耗时按 output_tokens/tps 反推）。
    tps: f64,
    /// 缓存命中率：Σ输入缓存 token ÷ Σ输入 token（加权，无输入 token 时记 0）。
    cache_hit_rate: f64,
}

/// 虚拟模型级 6 指标聚合：按 virtual_model_id 过滤返回单行，供二级页顶部指标卡。
async fn virtual_model_metrics(
    State(state): State<AppState>,
    Query(query): Query<VirtualModelMetricsQuery>,
) -> impl IntoResponse {
    let Some(virtual_model_id) = query.virtual_model_id else {
        return response::bad_request("缺少 virtualModelId 参数");
    };
    let (Some(start), Some(end)) = (query.start_time, query.end_time) else {
        return response::bad_request("缺少 startTime / endTime 参数");
    };
    if end <= start {
        return response::bad_request("endTime 必须大于 startTime");
    }
    let db = &state.db;

    let sql = format!(
        "SELECT COALESCE(vm.display_id, '') AS virtual_model_display_id,{RANK_METRIC_SQL} \
         FROM request r LEFT JOIN virtual_model vm ON vm.virtual_model_id = r.virtual_model_id \
         WHERE r.success = 1 AND r.virtual_model_id = ? \
           AND r.start_time >= ? AND r.start_time < ?"
    );

    let row = match db
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [virtual_model_id.into(), start.into(), end.into()],
        ))
        .await
    {
        Ok(Some(row)) => row,
        Ok(None) => return response::db_error("虚拟模型指标查询无结果".to_string()),
        Err(e) => return response::db_error(e.to_string()),
    };

    (
        StatusCode::OK,
        Json(Response::success(VirtualModelMetricsResponse {
            virtual_model_id,
            virtual_model_display_id: row.try_get("", "virtual_model_display_id").unwrap_or_default(),
            request_count: row.try_get::<i64>("", "request_count").unwrap_or(0),
            total_tokens: row.try_get::<i64>("", "total_tokens").unwrap_or(0),
            ttft: row.try_get::<f64>("", "ttft").unwrap_or(0.0),
            request_time: row.try_get::<f64>("", "request_time").unwrap_or(0.0),
            tps: row.try_get::<f64>("", "tps").unwrap_or(0.0),
            cache_hit_rate: row.try_get::<f64>("", "cache_hit_rate").unwrap_or(0.0),
        })),
    )
}

#[cfg(test)]
mod tests {
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
        assert_eq!(Granularity::parse(Some("hour")), Ok(Some(Granularity::Hour)));
        assert_eq!(Granularity::parse(Some("day")), Ok(Some(Granularity::Day)));
        assert_eq!(Granularity::parse(Some("month")), Ok(Some(Granularity::Month)));
        assert_eq!(Granularity::parse(Some("year")), Ok(Some(Granularity::Year)));
        assert!(Granularity::parse(Some("week")).is_err());
    }

    #[test]
    fn parse_tz_offset_clamps_out_of_range() {
        assert_eq!(parse_tz_offset(None), 0);
        assert_eq!(parse_tz_offset(Some(480)), 480);
        assert_eq!(parse_tz_offset(Some(0)), 0);
        assert_eq!(parse_tz_offset(Some(-330)), -330);
        assert_eq!(parse_tz_offset(Some(900)), 0); // 超出 ±14h
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
}

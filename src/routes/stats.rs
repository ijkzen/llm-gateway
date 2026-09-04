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

use crate::app_settings::AppSettings;
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
            Some(_) => Err(AppSettings::lang_sync().tr(
                "不支持的 granularity，可选 hour/day/month/year",
                "unsupported granularity; choose hour/day/month/year",
            )),
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

/// 解析后的图表窗口：时间区间 + 桶粒度 + 客户端时区偏移。
#[derive(Clone, Copy, Debug)]
struct ChartWindow {
    start: i64,
    end: i64,
    bucket_ms: i64,
    granularity: Granularity,
    tz_offset_minutes: i32,
}

impl ChartWindow {
    /// 桶起点表达式（SQL 侧）：把请求时间按客户端时区对齐到桶边界。
    fn bucket_expr(&self) -> String {
        let offset_ms = i64::from(self.tz_offset_minutes) * 60_000;
        if matches!(self.granularity, Granularity::Month | Granularity::Year) {
            format!("(r.start_time + {offset_ms}) / {DAY_MS}")
        } else {
            format!("(r.start_time + {offset_ms}) / {}", self.bucket_ms)
        }
    }

    /// 桶起点（毫秒时间戳）：与 bucket_expr 严格互逆（bucket_ms * 索引 - offset_ms）。
    fn bucket_start_ms(&self, bucket: i64) -> i64 {
        let offset_ms = i64::from(self.tz_offset_minutes) * 60_000;
        bucket * self.bucket_ms - offset_ms
    }
}

/// 解析图表窗口参数（与 charts 端点同一套缺省/回退规则）：
/// 显式 granularity 优先；缺省按窗口长度推断桶粒度（≤48h 小时、≤62d 天、其余 30 天块）。
/// 无 startTime/endTime 时回退过去 24 小时（小时桶）。
fn resolve_chart_window(
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

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/summary", get(summary))
        .route("/charts", get(charts))
        .route("/provider-rank", get(provider_rank))
        .route("/virtual-model-rank", get(virtual_model_rank))
        .route("/provider-model-rank", get(provider_model_rank))
        .route("/virtual-model-member-rank", get(virtual_model_member_rank))
        .route("/api-key-rank", get(api_key_rank))
        .route("/model-metrics", get(model_metrics))
        .route("/api-key-metrics", get(api_key_metrics))
        .route("/provider-metrics", get(provider_metrics))
        .route("/virtual-model-metrics", get(virtual_model_metrics))
        .route("/insight", get(insight))
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

/// 浮点值趋势点（比率/速率类指标，如失败率、缓存命中率、输出 token/秒）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FloatTrendPoint {
    /// 桶起点（毫秒时间戳）。
    bucket_start: i64,
    value: f64,
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

/// 每桶延迟分位点（毫秒；该桶无样本时字段为 0）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PercentilePoint {
    bucket_start: i64,
    p50: f64,
    p90: f64,
    p95: f64,
    p99: f64,
}

/// 失败原因分布条目（空/缺失原因归「无原因」，由前端文案呈现）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FailureReasonItem {
    reason: String,
    count: i64,
}

/// 按 API Key 聚合的调用量条目。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ApiKeyRankItem {
    api_key_name: String,
    value: i64,
}

/// 性能与可靠性分析（insight）：一次返回失败诊断 / 延迟分位 / Token 结构 / 吞吐四组数据。
///
/// 口径：失败相关基于全量请求（成功+失败都计数）；延迟与 Token 相关基于成功请求
/// （`success = 1`，与赛马/指标一致）；全部由 request 表现有字段聚合，无 schema 变更。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InsightResponse {
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

/// summary 可选时间窗口参数（均为可选；要么都缺省、要么都提供）。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SummaryQuery {
    /// 窗口起点（毫秒时间戳，含）。
    start_time: Option<i64>,
    /// 窗口终点（毫秒时间戳，不含）。
    end_time: Option<i64>,
}

/// 全量历史累计（可选时间窗口过滤）：累计请求数、成功率、总计 token、加权缓存命中率。
/// 不带 startTime/endTime 时保持全量聚合；两者同时提供时按 [start, end) 半开区间过滤。
async fn summary(
    State(state): State<AppState>,
    Query(query): Query<SummaryQuery>,
) -> impl IntoResponse {
    let base_sql = r#"
        SELECT COUNT(*) AS total_requests,
               COALESCE(SUM(CASE WHEN success THEN 1 ELSE 0 END), 0) AS success_count,
               COALESCE(SUM(total_tokens), 0) AS total_tokens,
               COALESCE(SUM(input_tokens), 0) AS input_tokens,
               COALESCE(SUM(input_cache_tokens), 0) AS cache_tokens
        FROM request
    "#;
    let (sql, params): (String, Vec<sea_orm::Value>) = match (query.start_time, query.end_time) {
        (None, None) => (base_sql.to_string(), Vec::new()),
        (Some(start), Some(end)) if end > start => (
            format!("{base_sql} WHERE start_time >= ? AND start_time < ?"),
            vec![start.into(), end.into()],
        ),
        _ => {
            return response::bad_request(AppSettings::lang_sync().tr(
                "startTime 与 endTime 必须同时提供且 endTime 晚于 startTime",
                "startTime and endTime must both be provided with endTime after startTime",
            ));
        }
    };
    let row = match state
        .db
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            params,
        ))
        .await
    {
        Ok(Some(row)) => row,
        Ok(None) => {
            return response::db_error(
                AppSettings::lang_sync().tr("统计查询无结果", "stats query returned no rows"),
            );
        }
        Err(e) => return response::db_error(e.to_string()),
    };

    let total_requests: i64 = row.try_get("", "total_requests").unwrap_or(0);
    let success_count: i64 = row.try_get("", "success_count").unwrap_or(0);
    let total_tokens: i64 = row.try_get("", "total_tokens").unwrap_or(0);
    let input_tokens: i64 = row.try_get("", "input_tokens").unwrap_or(0);
    let cache_tokens: i64 = row.try_get("", "cache_tokens").unwrap_or(0);

    let success_rate = weighted_ratio(success_count as f64, total_requests as f64);
    let cache_hit_rate = weighted_ratio(cache_tokens as f64, input_tokens as f64);

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
    /// 按调用方 API Key 名称过滤（可选；request.api_key_name 精确匹配）。
    api_key: Option<String>,
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
async fn charts(
    State(state): State<AppState>,
    Query(query): Query<ChartsQuery>,
) -> impl IntoResponse {
    let explicit_granularity = match Granularity::parse(query.granularity.as_deref()) {
        Ok(g) => g,
        Err(msg) => return response::bad_request(msg),
    };
    let tz_offset_minutes = parse_tz_offset(query.tz_offset_minutes);
    let tz = chrono::FixedOffset::east_opt(tz_offset_minutes * 60)
        .unwrap_or_else(|| chrono::FixedOffset::east_opt(0).expect("0 偏移恒有效"));
    let window = resolve_chart_window(
        query.start_time,
        query.end_time,
        explicit_granularity,
        tz_offset_minutes,
    );
    let (window_start, window_end, bucket_ms, granularity) = (
        window.start,
        window.end,
        window.bucket_ms,
        window.granularity,
    );

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
    if let Some(api_key) = query.api_key.as_deref() {
        where_sql.push_str(" AND r.api_key_name = ?");
        params.push(api_key.into());
    }

    // 月/年粒度：SQL 按本地日桶聚合，Rust 侧再归并自然月/年。
    let bucket_expr = window.bucket_expr();

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
    let (call_trend, token_trend) = if matches!(granularity, Granularity::Month | Granularity::Year)
    {
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
            .map(|(&bucket_start, value)| TrendPoint {
                bucket_start,
                value,
            })
            .collect::<Vec<_>>();
        let token_trend = starts
            .iter()
            .zip(token_values)
            .map(|(&bucket_start, value)| TrendPoint {
                bucket_start,
                value,
            })
            .collect::<Vec<_>>();
        (call_trend, token_trend)
    } else {
        // 小时/天：直接按桶索引区间补零（桶对齐本地边界，tz 偏移已并入表达式）。
        let first_bucket = (window_start + i64::from(tz_offset_minutes) * 60_000) / bucket_ms;
        let last_bucket = (window_end - 1 + i64::from(tz_offset_minutes) * 60_000)
            .max(window_start + i64::from(tz_offset_minutes) * 60_000)
            / bucket_ms;
        let fill_trend = |map: &std::collections::HashMap<i64, i64>| {
            (first_bucket..=last_bucket)
                .map(|bucket| TrendPoint {
                    bucket_start: window.bucket_start_ms(bucket),
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

/// 分位值（0~1）：升序样本的线性插值，与业界 P95 口径一致（N·p 位置插值）。
/// 空样本返回 0.0。
fn percentile(sorted_values: &[f64], p: f64) -> f64 {
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

/// 性能与可靠性分析：失败诊断 / 延迟分位 / Token 结构 / 吞吐 四组聚合。
///
/// 与 charts 共用同一套窗口/粒度/时区解析与过滤参数（providerId/virtualModelId/modelId）。
/// 月/年桶：失败率、缓存命中率、流式占比等比值在归并后的桶上重算；
/// 延迟分位在月/年桶上退化（分位需要逐值，跨月合并语义含糊），返回空数组。
async fn insight(
    State(state): State<AppState>,
    Query(query): Query<ChartsQuery>,
) -> impl IntoResponse {
    let explicit_granularity = match Granularity::parse(query.granularity.as_deref()) {
        Ok(g) => g,
        Err(msg) => return response::bad_request(msg),
    };
    let tz_offset_minutes = parse_tz_offset(query.tz_offset_minutes);
    let tz = chrono::FixedOffset::east_opt(tz_offset_minutes * 60)
        .unwrap_or_else(|| chrono::FixedOffset::east_opt(0).expect("0 偏移恒有效"));
    let window = resolve_chart_window(
        query.start_time,
        query.end_time,
        explicit_granularity,
        tz_offset_minutes,
    );

    // WHERE 公共条件：时间窗口（半开）+ 可选过滤（与 charts 同口径）。
    let mut where_sql = String::from("r.start_time >= ? AND r.start_time < ?");
    let mut params: Vec<sea_orm::Value> = vec![window.start.into(), window.end.into()];
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
    if let Some(api_key) = query.api_key.as_deref() {
        where_sql.push_str(" AND r.api_key_name = ?");
        params.push(api_key.into());
    }

    let db = &state.db;
    let month_mode = matches!(window.granularity, Granularity::Month | Granularity::Year);
    let bucket_expr = window.bucket_expr();

    // 每桶分组聚合（全量或成功请求），返回 (bucket, value) 对；月/年桶先按本地日聚合。
    let group_rows = |value_expr: &str, success_only: bool| {
        let success_cond = if success_only {
            " AND r.success = 1"
        } else {
            ""
        };
        format!(
            "SELECT {bucket_expr} AS bucket, {value_expr} AS value \
             FROM request r WHERE {where_sql}{success_cond} GROUP BY bucket"
        )
    };

    // 全量：每桶调用数 / 失败数 / 流式数 / total_tokens（吞吐用）。
    let (call_rows, fail_rows, stream_rows, tpm_rows, api_key_rows) = match tokio::try_join!(
        db.query_all_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            group_rows("COUNT(*)", false),
            params.clone(),
        )),
        db.query_all_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            group_rows("COALESCE(SUM(CASE WHEN r.success = 0 THEN 1 ELSE 0 END), 0)", false),
            params.clone(),
        )),
        db.query_all_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            group_rows("COALESCE(SUM(CASE WHEN r.stream THEN 1 ELSE 0 END), 0)", false),
            params.clone(),
        )),
        db.query_all_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            group_rows("COALESCE(SUM(r.total_tokens), 0)", false),
            params.clone(),
        )),
        db.query_all_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT r.api_key_name, COUNT(*) AS value FROM request r WHERE {where_sql} GROUP BY r.api_key_name"
            ),
            params.clone(),
        )),
    ) {
        Ok(rows) => rows,
        Err(e) => return response::db_error(e.to_string()),
    };

    // 成功请求：输入 / 输出 token、缓存 token、输出耗时（秒）。
    let (input_rows, output_rows, cache_rows, out_time_rows) = match tokio::try_join!(
        db.query_all_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            group_rows("COALESCE(SUM(r.input_tokens), 0)", true),
            params.clone(),
        )),
        db.query_all_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            group_rows("COALESCE(SUM(r.output_tokens), 0)", true),
            params.clone(),
        )),
        db.query_all_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            group_rows("COALESCE(SUM(r.input_cache_tokens), 0)", true),
            params.clone(),
        )),
        db.query_all_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            group_rows(
                "COALESCE(SUM(CASE WHEN r.output_tokens_time > 0 THEN r.output_tokens / (r.output_tokens_time / 1000.0) ELSE 0 END), 0)",
                true,
            ),
            params.clone(),
        )),
    ) {
        Ok(rows) => rows,
        Err(e) => return response::db_error(e.to_string()),
    };

    // 失败原因分布：全窗口按 fail_reason 分组（仅失败请求；NULL/空串归空串，
    // 前端显示「无原因」）。
    let mut reason_map: std::collections::BTreeMap<String, i64> = std::collections::BTreeMap::new();
    if let Ok(rows) = db
        .query_all_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT COALESCE(NULLIF(r.fail_reason, ''), '') AS reason, COUNT(*) AS count \
                 FROM request r WHERE {where_sql} AND r.success = 0 GROUP BY reason"
            ),
            params.clone(),
        ))
        .await
    {
        for row in &rows {
            let reason: String = row.try_get("", "reason").unwrap_or_default();
            let count: i64 = row.try_get("", "count").unwrap_or(0);
            *reason_map.entry(reason).or_insert(0) += count;
        }
    } else {
        return response::db_error("失败原因统计查询失败");
    }

    // 延迟分位：每桶逐值拉回（仅成功请求；ttft 只计流式有首 token 的行）。
    let (ttft_rows, latency_rows) = match tokio::try_join!(
        db.query_all_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT {bucket_expr} AS bucket, r.ttft AS value FROM request r \
                 WHERE {where_sql} AND r.success = 1 AND r.ttft IS NOT NULL"
            ),
            params.clone(),
        )),
        db.query_all_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT {bucket_expr} AS bucket, r.request_time AS value FROM request r \
                 WHERE {where_sql} AND r.success = 1"
            ),
            params.clone(),
        )),
    ) {
        Ok(rows) => rows,
        Err(e) => return response::db_error(e.to_string()),
    };

    // ---- 归并/填充 ----

    // 整数值趋势填充：小时/天按桶索引补零；月/年按本地日归并自然月/年。
    let fill_int_series = |map: &std::collections::HashMap<i64, f64>| -> Vec<TrendPoint> {
        if month_mode {
            let day_indexes = map
                .iter()
                .map(|(&bucket, &value)| (bucket, value.round() as i64))
                .collect::<Vec<_>>();
            let (starts, values, _) = merge_natural_periods(
                &day_indexes,
                &[],
                window.start,
                window.end,
                tz,
                matches!(window.granularity, Granularity::Month),
            );
            starts
                .into_iter()
                .zip(values)
                .map(|(bucket_start, value)| TrendPoint {
                    bucket_start,
                    value,
                })
                .collect()
        } else {
            let first_bucket =
                (window.start + i64::from(tz_offset_minutes) * 60_000) / window.bucket_ms;
            let last_bucket = (window.end - 1 + i64::from(tz_offset_minutes) * 60_000)
                .max(window.start + i64::from(tz_offset_minutes) * 60_000)
                / window.bucket_ms;
            (first_bucket..=last_bucket)
                .map(|bucket| TrendPoint {
                    bucket_start: window.bucket_start_ms(bucket),
                    value: map.get(&bucket).copied().unwrap_or(0.0).round() as i64,
                })
                .collect()
        }
    };

    // 浮点值趋势填充：语义同 fill_int_series，但保留小数（比率/速率类指标）。
    let fill_float_series = |map: &std::collections::HashMap<i64, f64>| -> Vec<FloatTrendPoint> {
        if month_mode {
            let day_indexes = map
                .iter()
                .map(|(&bucket, &value)| (bucket, value.round() as i64))
                .collect::<Vec<_>>();
            let (starts, values, _) = merge_natural_periods(
                &day_indexes,
                &[],
                window.start,
                window.end,
                tz,
                matches!(window.granularity, Granularity::Month),
            );
            starts
                .into_iter()
                .zip(values)
                .map(|(bucket_start, value)| FloatTrendPoint {
                    bucket_start,
                    value: value as f64,
                })
                .collect()
        } else {
            let first_bucket =
                (window.start + i64::from(tz_offset_minutes) * 60_000) / window.bucket_ms;
            let last_bucket = (window.end - 1 + i64::from(tz_offset_minutes) * 60_000)
                .max(window.start + i64::from(tz_offset_minutes) * 60_000)
                / window.bucket_ms;
            (first_bucket..=last_bucket)
                .map(|bucket| FloatTrendPoint {
                    bucket_start: window.bucket_start_ms(bucket),
                    value: map.get(&bucket).copied().unwrap_or(0.0),
                })
                .collect()
        }
    };

    // 按桶起点（bucket_start）索引 map，供月/年归并后按桶起点重算比率。
    // 小时/天：桶索引反推桶起点；月/年：SQL 桶是本地日索引，需归并到自然月/年起点
    //（与 merge_natural_periods 的 to_bucket_start 一致），否则与 failure_trend 的
    // 桶起点（每月/年 1 日 0 点）对不上，比率会全部错配为 0。
    let index_by_start = |map: &std::collections::HashMap<i64, f64>| {
        let mut by_start: std::collections::HashMap<i64, f64> = std::collections::HashMap::new();
        for (&bucket, &value) in map {
            let start = if month_mode {
                let day_ms = bucket * DAY_MS - i64::from(tz_offset_minutes) * 60_000;
                let date = chrono::DateTime::from_timestamp_millis(day_ms)
                    .map(|t| t.with_timezone(&tz))
                    .and_then(|local| {
                        let (y, m) = if matches!(window.granularity, Granularity::Month) {
                            (local.year(), local.month())
                        } else {
                            (local.year(), 0)
                        };
                        chrono::NaiveDate::from_ymd_opt(y, if m > 0 { m } else { 1 }, 1)
                            .and_then(|d| d.and_hms_opt(0, 0, 0))
                            .and_then(|dt| dt.and_local_timezone(tz).single())
                            .map(|dt| dt.timestamp_millis())
                    });
                match date {
                    Some(ms) => ms,
                    None => continue,
                }
            } else {
                window.bucket_start_ms(bucket)
            };
            *by_start.entry(start).or_insert(0.0) += value;
        }
        by_start
    };

    let to_map = |rows: &[sea_orm::QueryResult]| -> std::collections::HashMap<i64, f64> {
        let mut map = std::collections::HashMap::new();
        for row in rows {
            let bucket: i64 = row.try_get("", "bucket").unwrap_or(0);
            // SQLite 的 SUM(INTEGER) 返回 INTEGER、SUM(REAL)/除法为 REAL：先试 f64 再试 i64。
            let value: f64 = row
                .try_get("", "value")
                .ok()
                .or_else(|| row.try_get::<i64>("", "value").ok().map(|v| v as f64))
                .unwrap_or(0.0);
            // SQL GROUP BY bucket 保证桶唯一，直接覆盖即可。
            map.insert(bucket, value);
        }
        map
    };

    let calls_map = to_map(&call_rows);
    let fails_map = to_map(&fail_rows);
    let streams_map = to_map(&stream_rows);
    let tpm_map = to_map(&tpm_rows);
    let inputs_map = to_map(&input_rows);
    let outputs_map = to_map(&output_rows);
    let caches_map = to_map(&cache_rows);
    let out_time_rates_map = to_map(&out_time_rows);

    let failure_trend = fill_int_series(&fails_map);
    let call_trend = fill_int_series(&calls_map);
    let input_token_trend = fill_int_series(&inputs_map);
    let output_token_trend = fill_int_series(&outputs_map);
    let output_per_sec_trend = fill_float_series(&out_time_rates_map);

    // 比率重算：月/年桶按归并后桶起点，小时/天按桶索引。
    let calls_by_start = index_by_start(&calls_map);
    let fails_by_start = index_by_start(&fails_map);
    let streams_by_start = index_by_start(&streams_map);
    let inputs_by_start = index_by_start(&inputs_map);
    let caches_by_start = index_by_start(&caches_map);
    // 比率按归并后的桶起点重算：以 failure_trend 的桶起点为对齐锚（所有 int/float
    // 序列都用同一组零填充桶，保证三组比率对齐同一时间轴；若未来某序列改条件填充，
    // 需同步改这里的锚定）。
    let ratio_by_start = |total_by_start: &std::collections::HashMap<i64, f64>,
                          part_by_start: &std::collections::HashMap<i64, f64>|
     -> Vec<FloatTrendPoint> {
        let aligned_starts: Vec<i64> = failure_trend.iter().map(|p| p.bucket_start).collect();
        aligned_starts
            .into_iter()
            .map(|start| {
                let total = total_by_start.get(&start).copied().unwrap_or(0.0);
                let part = part_by_start.get(&start).copied().unwrap_or(0.0);
                FloatTrendPoint {
                    bucket_start: start,
                    value: if total > 0.0 { part / total } else { 0.0 },
                }
            })
            .collect()
    };
    let failure_rate_trend = ratio_by_start(&calls_by_start, &fails_by_start);
    let stream_ratio_final = ratio_by_start(&calls_by_start, &streams_by_start);
    // 缓存命中率趋势单独保留 5 位小数，与 summary / SQL 侧 cache_hit_rate_sql
    // 口径一致（failure_rate / stream_ratio 保持原精度）。
    let cache_rate_final = ratio_by_start(&inputs_by_start, &caches_by_start)
        .into_iter()
        .map(|point| FloatTrendPoint {
            bucket_start: point.bucket_start,
            value: round_5(point.value),
        })
        .collect::<Vec<_>>();

    // RPM/TPM：仅小时桶有意义。RPM=每小时请求数（÷窗口小时数）；TPM=每小时 token 总量 ÷60 得每分钟。
    let (rpm_trend, tpm_trend) = if matches!(window.granularity, Granularity::Hour) {
        let hours = ((window.end - window.start) as f64 / HOUR_MS as f64).max(1.0);
        (
            fill_int_series(&calls_map)
                .into_iter()
                .map(|point| TrendPoint {
                    bucket_start: point.bucket_start,
                    value: ((point.value as f64 / hours).round()) as i64,
                })
                .collect::<Vec<_>>(),
            // 桶内 token 总量 ÷ 60 分钟 = 每分钟 token（保留小数）。
            fill_int_series(&tpm_map)
                .into_iter()
                .map(|point| FloatTrendPoint {
                    bucket_start: point.bucket_start,
                    value: point.value as f64 / 60.0,
                })
                .collect::<Vec<_>>(),
        )
    } else {
        (Vec::new(), Vec::new())
    };

    // API Key 排行：按调用数降序（Top N + 其他由前端处理）。
    let mut api_key_rank = api_key_rows
        .iter()
        .map(|row| ApiKeyRankItem {
            api_key_name: row.try_get("", "api_key_name").unwrap_or_default(),
            value: row.try_get::<i64>("", "value").unwrap_or(0),
        })
        .collect::<Vec<_>>();
    api_key_rank.sort_by_key(|item| std::cmp::Reverse(item.value));

    // 延迟分位：小时/天桶逐值分组算分位；月/年返回空数组。
    let group_percentiles = |rows: &[sea_orm::QueryResult]| -> Vec<PercentilePoint> {
        if month_mode {
            return Vec::new();
        }
        let mut buckets: std::collections::BTreeMap<i64, Vec<f64>> =
            std::collections::BTreeMap::new();
        for row in rows {
            let bucket: i64 = row.try_get("", "bucket").unwrap_or(0);
            // ttft/request_time 是 INTEGER 列：先试 f64 再试 i64。
            let value: f64 = row
                .try_get("", "value")
                .ok()
                .or_else(|| row.try_get::<i64>("", "value").ok().map(|v| v as f64))
                .unwrap_or(0.0);
            buckets.entry(bucket).or_default().push(value);
        }
        // 按桶补零对齐（无样本桶输出 0）。
        let first_bucket =
            (window.start + i64::from(tz_offset_minutes) * 60_000) / window.bucket_ms;
        let last_bucket = (window.end - 1 + i64::from(tz_offset_minutes) * 60_000)
            .max(window.start + i64::from(tz_offset_minutes) * 60_000)
            / window.bucket_ms;
        (first_bucket..=last_bucket)
            .map(|bucket| {
                let mut values = buckets.get(&bucket).cloned().unwrap_or_default();
                values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                PercentilePoint {
                    bucket_start: window.bucket_start_ms(bucket),
                    p50: percentile(&values, 0.5),
                    p90: percentile(&values, 0.9),
                    p95: percentile(&values, 0.95),
                    p99: percentile(&values, 0.99),
                }
            })
            .collect()
    };
    let ttft_percentiles = group_percentiles(&ttft_rows);
    let latency_percentiles = group_percentiles(&latency_rows);

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
            cache_hit_rate_trend: cache_rate_final,
            output_tokens_per_sec_trend: output_per_sec_trend,
            api_key_rank,
            rpm_trend,
            tpm_trend,
            stream_ratio_trend: stream_ratio_final,
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
    /// 按模型过滤（可选；api_key_rank 三级页使用，须与 provider_id 同传）。
    model_id: Option<String>,
    /// 按调用方 API Key 名称过滤（可选；API Key 数据面板「该 key 用到的 X」排行）。
    api_key: Option<String>,
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

/// 加权 TPS：Σ输出 token ÷ Σ网络耗时（耗时按 output_tokens/tps 反推，
/// 仅计入 tps>0 且 output_tokens>0 的行）；分母为 0 时记 0。
fn tps_sql(alias: &str) -> String {
    format!(
        "CASE \
           WHEN SUM(CASE WHEN {alias}.tps > 0 AND {alias}.output_tokens > 0 THEN {alias}.output_tokens / {alias}.tps ELSE 0 END) > 0 \
           THEN COALESCE(SUM({alias}.output_tokens), 0) / SUM(CASE WHEN {alias}.tps > 0 AND {alias}.output_tokens > 0 THEN {alias}.output_tokens / {alias}.tps ELSE 0 END) \
           ELSE 0 \
         END AS tps"
    )
}

/// 加权缓存命中率：缓存命中 token / 输入 token，统一保留 5 位小数。
/// 排行榜 SQL 与虚拟模型成员子查询共用，避免两处口径漂移。
fn cache_hit_rate_sql(alias: &str) -> String {
    format!(
        "CASE \
           WHEN SUM({alias}.input_tokens) > 0 \
           THEN ROUND(1.0 * SUM({alias}.input_cache_tokens) / SUM({alias}.input_tokens), 5) \
           ELSE 0 \
         END AS cache_hit_rate"
    )
}

/// 赛马聚合的 6 个指标列（value_expr 与字段读取共用，供应商/虚拟模型维度
/// 仅 SELECT 的名称列、JOIN 与 GROUP BY 不同）；tps / cache_hit_rate 与
/// 虚拟模型成员子查询共用 tps_sql / cache_hit_rate_sql，避免口径漂移。
fn rank_metric_sql() -> String {
    format!(
        r#"
       COUNT(*) AS request_count,
       COALESCE(SUM(r.total_tokens), 0) AS total_tokens,
       AVG(r.ttft) AS ttft,
       AVG(r.request_time) AS request_time,
       {tps},
       {cache}
"#,
        tps = tps_sql("r"),
        cache = cache_hit_rate_sql("r"),
    )
}

/// 保留 5 位小数（0.123456 → 0.12346），与 SQL 侧 ROUND(…, 5) 一致。
fn round_5(value: f64) -> f64 {
    (value * 100_000.0).round() / 100_000.0
}

/// 加权比率：part / total，统一保留 5 位小数；total 为 0 时记 0。
/// 与 SQL 侧 cache_hit_rate_sql 的口径（ROUND(…, 5)）保持一致，
/// 供 summary 等 Rust 层聚合复用，避免与 SQL 侧口径漂移。
fn weighted_ratio(part: f64, total: f64) -> f64 {
    if total <= 0.0 {
        return 0.0;
    }
    round_5(part / total)
}

/// 从查询参数解析排序指标与方向；参数缺失/非法返回错误响应。
/// T 为调用方成功响应的 data 类型（错误响应的 data 为空，仅用于类型对齐）。
fn parse_rank_query<T>(
    query: &RankQuery,
) -> Result<(RankSortKey, &'static str, i64, i64), response::ErrorResponse<T>> {
    let sort_key = parse_sort_key(query.sort_by.as_deref()).ok_or_else(|| {
        response::bad_request(
            AppSettings::lang_sync().tr("sortBy 参数非法", "invalid sortBy parameter"),
        )
    })?;
    let (Some(start), Some(end)) = (query.start_time, query.end_time) else {
        return Err(response::bad_request(AppSettings::lang_sync().tr(
            "缺少 startTime / endTime 参数",
            "missing startTime / endTime parameters",
        )));
    };
    if end <= start {
        return Err(response::bad_request(AppSettings::lang_sync().tr(
            "endTime 必须大于 startTime",
            "endTime must be greater than startTime",
        )));
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
    let rank_sql = rank_metric_sql();
    let mut where_sql = String::from("r.success = 1 AND r.start_time >= ? AND r.start_time < ?");
    let mut params: Vec<sea_orm::Value> = vec![start.into(), end.into()];
    if let Some(api_key) = query.api_key.as_deref() {
        where_sql.push_str(" AND r.api_key_name = ?");
        params.push(api_key.into());
    }
    let sql = format!(
        "SELECT r.provider_id AS provider_id, COALESCE(p.name, '') AS provider_name,{rank_sql} \
         FROM request r LEFT JOIN provider p ON p.id = r.provider_id \
         WHERE {where_sql} \
         GROUP BY r.provider_id"
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
) -> Result<
    Json<Response<VirtualModelRankResponse>>,
    response::ErrorResponse<VirtualModelRankResponse>,
> {
    let (sort_key, order_dir, start, end) = match parse_rank_query(&query) {
        Ok(v) => v,
        Err(e) => return Err(e),
    };
    let db = &state.db;

    // 按 id 分组（同一 display_id 的虚拟模型也各自成行），JOIN 出 display_id。
    let rank_sql = rank_metric_sql();
    let mut where_sql = String::from("r.success = 1 AND r.start_time >= ? AND r.start_time < ?");
    let mut params: Vec<sea_orm::Value> = vec![start.into(), end.into()];
    if let Some(api_key) = query.api_key.as_deref() {
        where_sql.push_str(" AND r.api_key_name = ?");
        params.push(api_key.into());
    }
    let sql = format!(
        "SELECT r.virtual_model_id AS virtual_model_id, \
                COALESCE(vm.display_id, '') AS virtual_model_display_id,{rank_sql} \
         FROM request r LEFT JOIN virtual_model vm ON vm.virtual_model_id = r.virtual_model_id \
         WHERE {where_sql} \
         GROUP BY r.virtual_model_id"
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
        .map(|row| VirtualModelRankItem {
            virtual_model_id: row.try_get::<i32>("", "virtual_model_id").unwrap_or(0),
            virtual_model_display_id: row
                .try_get("", "virtual_model_display_id")
                .unwrap_or_default(),
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
) -> Result<
    Json<Response<ProviderModelRankResponse>>,
    response::ErrorResponse<ProviderModelRankResponse>,
> {
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
    if let Some(api_key) = query.api_key.as_deref() {
        where_sql.push_str(" AND r.api_key_name = ?");
        params.push(api_key.into());
    }

    let rank_sql = rank_metric_sql();
    let sql = format!(
        "SELECT r.provider_id AS provider_id, COALESCE(p.name, '') AS provider_name, \
                COALESCE(pm.provider_model_id, r.model_id) AS model_id,{rank_sql} \
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
) -> Result<
    Json<Response<VirtualModelMemberRankResponse>>,
    response::ErrorResponse<VirtualModelMemberRankResponse>,
> {
    let (sort_key, order_dir, start, end) = match parse_rank_query(&query) {
        Ok(v) => v,
        Err(e) => return Err(e),
    };
    let Some(virtual_model_id) = query.virtual_model_id else {
        return Err(response::bad_request(AppSettings::lang_sync().tr(
            "缺少 virtualModelId 参数",
            "missing virtualModelId parameter",
        )));
    };
    let db = &state.db;

    // 聚合子查询：该虚拟模型下实际服务的成员（按 provider_id + model_id 分组）。
    // 6 指标表达式与 rank_metric_sql 同口径（经 tps_sql / cache_hit_rate_sql 复用），
    // 但需带上关联键列。
    let sql = format!(
        "SELECT pm.provider_id AS provider_id, COALESCE(p.name, '') AS provider_name, \
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
                    {tps_sql}, \
                    {cache_sql} \
             FROM request r \
             WHERE r.success = 1 AND r.virtual_model_id = ? AND r.start_time >= ? AND r.start_time < ? \
             GROUP BY r.provider_id, r.model_id \
         ) agg ON agg.provider_id = pm.provider_id AND agg.provider_model_id = pm.provider_model_id \
         WHERE vmi.virtual_model_id = ?",
        tps_sql = tps_sql("r"),
        cache_sql = cache_hit_rate_sql("r"),
    );

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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ApiKeyRaceRankItem {
    /// 调用方 API Key 名称（request.api_key_name；Key 已删除的历史行仍按原名聚合）。
    api_key_name: String,
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
struct ApiKeyRaceRankResponse {
    start_time: i64,
    end_time: i64,
    items: Vec<ApiKeyRaceRankItem>,
}

/// API Key 维度赛马：按 request.api_key_name 分组聚合 6 指标，规格与
/// 供应商赛马一致（排序 + 时间窗口）。可选过滤：
/// - provider_id：二级页（供应商详情）——只看该供应商的调用；
/// - virtual_model_id：二级页（虚拟模型详情）——只看该虚拟模型的调用；
/// - provider_id + model_id：三级页（模型详情）——只看该供应商下某模型的调用。
async fn api_key_rank(
    State(state): State<AppState>,
    Query(query): Query<RankQuery>,
) -> Result<Json<Response<ApiKeyRaceRankResponse>>, response::ErrorResponse<ApiKeyRaceRankResponse>>
{
    let (sort_key, order_dir, start, end) = match parse_rank_query(&query) {
        Ok(v) => v,
        Err(e) => return Err(e),
    };
    // 过滤组合契约：三种互斥形态（providerId / virtualModelId / providerId+modelId），
    // 组合之外（providerId+virtualModelId 同传、modelId 无 providerId）返回 400，
    // 避免静默叠加两个维度造成语义混乱。
    if query.provider_id.is_some() && query.virtual_model_id.is_some() {
        return Err(response::bad_request(AppSettings::lang_sync().tr(
            "providerId 与 virtualModelId 不能同时指定",
            "providerId and virtualModelId cannot be combined",
        )));
    }
    if query.model_id.is_some() && query.provider_id.is_none() {
        return Err(response::bad_request(AppSettings::lang_sync().tr(
            "modelId 须与 providerId 同时指定",
            "modelId requires providerId",
        )));
    }
    let db = &state.db;

    // 过滤条件拼接：provider_id / virtual_model_id / provider_id + model_id 三种组合。
    let mut where_sql = String::from("r.success = 1 AND r.start_time >= ? AND r.start_time < ?");
    let mut params: Vec<sea_orm::Value> = vec![start.into(), end.into()];
    if let Some(provider_id) = query.provider_id {
        where_sql.push_str(" AND r.provider_id = ?");
        params.push(provider_id.into());
    }
    if let Some(virtual_model_id) = query.virtual_model_id {
        where_sql.push_str(" AND r.virtual_model_id = ?");
        params.push(virtual_model_id.into());
    }
    if let Some(model_id) = query.model_id.as_deref() {
        where_sql.push_str(" AND r.model_id = ?");
        params.push(model_id.into());
    }

    let rank_sql = rank_metric_sql();
    let sql = format!(
        "SELECT r.api_key_name AS api_key_name,{rank_sql} \
         FROM request r \
         WHERE {where_sql} \
         GROUP BY r.api_key_name"
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
        .map(|row| ApiKeyRaceRankItem {
            api_key_name: row.try_get("", "api_key_name").unwrap_or_default(),
            request_count: row.try_get::<i64>("", "request_count").unwrap_or(0),
            total_tokens: row.try_get::<i64>("", "total_tokens").unwrap_or(0),
            ttft: row.try_get::<f64>("", "ttft").unwrap_or(0.0),
            request_time: row.try_get::<f64>("", "request_time").unwrap_or(0.0),
            tps: row.try_get::<f64>("", "tps").unwrap_or(0.0),
            cache_hit_rate: row.try_get::<f64>("", "cache_hit_rate").unwrap_or(0.0),
        })
        .collect::<Vec<_>>();

    let is_asc = order_dir == "ASC";
    let value_of = |item: &ApiKeyRaceRankItem| -> f64 {
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

    Ok(Json(Response::success(ApiKeyRaceRankResponse {
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
        return response::bad_request(AppSettings::lang_sync().tr(
            "缺少 providerId / modelId 参数",
            "missing providerId / modelId parameters",
        ));
    };
    let (Some(start), Some(end)) = (query.start_time, query.end_time) else {
        return response::bad_request(AppSettings::lang_sync().tr(
            "缺少 startTime / endTime 参数",
            "missing startTime / endTime parameters",
        ));
    };
    if end <= start {
        return response::bad_request(AppSettings::lang_sync().tr(
            "endTime 必须大于 startTime",
            "endTime must be greater than startTime",
        ));
    }
    let db = &state.db;

    // 单行聚合 6 指标（无 GROUP BY），JOIN provider 出名称。
    let rank_sql = rank_metric_sql();
    let sql = format!(
        "SELECT COALESCE(p.name, '') AS provider_name,{rank_sql} \
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
        Ok(None) => {
            return response::db_error(
                AppSettings::lang_sync()
                    .tr("模型指标查询无结果", "model metrics query returned no rows"),
            );
        }
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

/// API Key 指标查询参数：apiKey（调用方名称）+ 时间窗口。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiKeyMetricsQuery {
    /// 调用方 API Key 名称（request.api_key_name，必填）。
    api_key: Option<String>,
    /// 窗口起点（毫秒时间戳，含）。
    start_time: Option<i64>,
    /// 窗口终点（毫秒时间戳，不含）。
    end_time: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ApiKeyMetricsResponse {
    /// 调用方 API Key 名称。
    api_key_name: String,
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

/// API Key 指标：按调用方 API Key 名称过滤聚合 6 指标，返回单行。
/// 供 API Key 数据面板顶部指标卡使用；窗口内无该 key 请求时返回全 0（不报错）。
async fn api_key_metrics(
    State(state): State<AppState>,
    Query(query): Query<ApiKeyMetricsQuery>,
) -> impl IntoResponse {
    let Some(api_key) = query.api_key.as_deref().filter(|s| !s.is_empty()) else {
        return response::bad_request(
            AppSettings::lang_sync().tr("缺少 apiKey 参数", "missing apiKey parameter"),
        );
    };
    let (Some(start), Some(end)) = (query.start_time, query.end_time) else {
        return response::bad_request(AppSettings::lang_sync().tr(
            "缺少 startTime / endTime 参数",
            "missing startTime / endTime parameters",
        ));
    };
    if end <= start {
        return response::bad_request(AppSettings::lang_sync().tr(
            "endTime 必须大于 startTime",
            "endTime must be greater than startTime",
        ));
    }
    let db = &state.db;

    // 单行聚合 6 指标（无 GROUP BY）：仅该 key 的成功请求。
    let rank_sql = rank_metric_sql();
    let sql = format!(
        "SELECT {rank_sql} \
         FROM request r \
         WHERE r.success = 1 AND r.api_key_name = ? \
           AND r.start_time >= ? AND r.start_time < ?"
    );

    // SQLite COUNT/SUM 无行时返回单行全 0/0.0；无请求窗口用聚合行归一，避免 db_error。
    let (request_count, total_tokens, ttft, request_time, tps, cache_hit_rate) = match db
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [api_key.into(), start.into(), end.into()],
        ))
        .await
    {
        Ok(Some(row)) => (
            row.try_get::<i64>("", "request_count").unwrap_or(0),
            row.try_get::<i64>("", "total_tokens").unwrap_or(0),
            row.try_get::<f64>("", "ttft").unwrap_or(0.0),
            row.try_get::<f64>("", "request_time").unwrap_or(0.0),
            row.try_get::<f64>("", "tps").unwrap_or(0.0),
            row.try_get::<f64>("", "cache_hit_rate").unwrap_or(0.0),
        ),
        Ok(None) => (0, 0, 0.0, 0.0, 0.0, 0.0),
        Err(e) => return response::db_error(e.to_string()),
    };

    (
        StatusCode::OK,
        Json(Response::success(ApiKeyMetricsResponse {
            api_key_name: api_key.to_string(),
            request_count,
            total_tokens,
            ttft,
            request_time,
            tps,
            cache_hit_rate,
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
        return response::bad_request(
            AppSettings::lang_sync().tr("缺少 providerId 参数", "missing providerId parameter"),
        );
    };
    let (Some(start), Some(end)) = (query.start_time, query.end_time) else {
        return response::bad_request(AppSettings::lang_sync().tr(
            "缺少 startTime / endTime 参数",
            "missing startTime / endTime parameters",
        ));
    };
    if end <= start {
        return response::bad_request(AppSettings::lang_sync().tr(
            "endTime 必须大于 startTime",
            "endTime must be greater than startTime",
        ));
    }
    let db = &state.db;

    let rank_sql = rank_metric_sql();
    let sql = format!(
        "SELECT COALESCE(p.name, '') AS provider_name,{rank_sql} \
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
        Ok(None) => {
            return response::db_error(AppSettings::lang_sync().tr(
                "供应商指标查询无结果",
                "provider metrics query returned no rows",
            ));
        }
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
        return response::bad_request(AppSettings::lang_sync().tr(
            "缺少 virtualModelId 参数",
            "missing virtualModelId parameter",
        ));
    };
    let (Some(start), Some(end)) = (query.start_time, query.end_time) else {
        return response::bad_request(AppSettings::lang_sync().tr(
            "缺少 startTime / endTime 参数",
            "missing startTime / endTime parameters",
        ));
    };
    if end <= start {
        return response::bad_request(AppSettings::lang_sync().tr(
            "endTime 必须大于 startTime",
            "endTime must be greater than startTime",
        ));
    }
    let db = &state.db;

    let rank_sql = rank_metric_sql();
    let sql = format!(
        "SELECT COALESCE(vm.display_id, '') AS virtual_model_display_id,{rank_sql} \
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
        Ok(None) => {
            return response::db_error(AppSettings::lang_sync().tr(
                "虚拟模型指标查询无结果",
                "virtual model metrics query returned no rows",
            ));
        }
        Err(e) => return response::db_error(e.to_string()),
    };

    (
        StatusCode::OK,
        Json(Response::success(VirtualModelMetricsResponse {
            virtual_model_id,
            virtual_model_display_id: row
                .try_get("", "virtual_model_display_id")
                .unwrap_or_default(),
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

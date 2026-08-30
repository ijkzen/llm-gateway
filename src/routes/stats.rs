use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
};
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::response::{self, Response};
use crate::state::AppState;

const HOUR_MS: i64 = 3_600_000;
const TREND_BUCKETS: i64 = 24;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/summary", get(summary))
        .route("/charts", get(charts))
        .route("/provider-rank", get(provider_rank))
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
        success_count as f64 / total_requests as f64
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

/// 过去 24 小时图表数据：调用/ token 的小时趋势 + 按上游模型的分布。
async fn charts(State(state): State<AppState>) -> impl IntoResponse {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let current_bucket = now_ms / HOUR_MS;
    let first_bucket = current_bucket - (TREND_BUCKETS - 1);
    let window_start = first_bucket * HOUR_MS;

    let trend_sql = |value_expr: &str| {
        format!(
            "SELECT (start_time / {HOUR_MS}) AS bucket, {value_expr} AS value \
             FROM request WHERE start_time >= {window_start} GROUP BY bucket"
        )
    };
    let model_sql = |value_expr: &str| {
        format!(
            "SELECT COALESCE(p.name, '') AS provider_name, r.model_id, {value_expr} AS value \
             FROM request r LEFT JOIN provider p ON p.id = r.provider_id \
             WHERE r.start_time >= {window_start} GROUP BY p.name, r.model_id"
        )
    };

    let db = &state.db;
    let (call_rows, token_rows, call_model_rows, token_model_rows) = match tokio::try_join!(
        db.query_all_raw(Statement::from_string(
            DbBackend::Sqlite,
            trend_sql("COUNT(*)"),
        )),
        db.query_all_raw(Statement::from_string(
            DbBackend::Sqlite,
            trend_sql("COALESCE(SUM(total_tokens), 0)"),
        )),
        db.query_all_raw(Statement::from_string(
            DbBackend::Sqlite,
            model_sql("COUNT(*)"),
        )),
        db.query_all_raw(Statement::from_string(
            DbBackend::Sqlite,
            model_sql("COALESCE(SUM(total_tokens), 0)"),
        )),
    ) {
        Ok(rows) => rows,
        Err(e) => return response::db_error(e.to_string()),
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

    let fill_trend = |map: &std::collections::HashMap<i64, i64>| {
        (first_bucket..=current_bucket)
            .map(|bucket| TrendPoint {
                bucket_start: bucket * HOUR_MS,
                value: map.get(&bucket).copied().unwrap_or(0),
            })
            .collect::<Vec<_>>()
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
            call_trend: fill_trend(&call_counts),
            call_by_model: to_model_values(call_model_rows),
            token_trend: fill_trend(&token_sums),
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderRankQuery {
    /// 排序指标：totalTokens | requestCount | ttft | requestTime | tps | cacheHitRate。
    sort_by: Option<String>,
    /// 排序方向：asc | desc（缺省按指标默认方向）。
    sort_order: Option<String>,
    /// 窗口起点（毫秒时间戳，含）。
    start_time: Option<i64>,
    /// 窗口终点（毫秒时间戳，不含）。
    end_time: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderRankItem {
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
    Query(query): Query<ProviderRankQuery>,
) -> impl IntoResponse {
    let sort_key = match query.sort_by.as_deref() {
        None | Some("totalTokens") => RankSortKey::TotalTokens,
        Some("requestCount") => RankSortKey::RequestCount,
        Some("ttft") => RankSortKey::Ttft,
        Some("requestTime") => RankSortKey::RequestTime,
        Some("tps") => RankSortKey::Tps,
        Some("cacheHitRate") => RankSortKey::CacheHitRate,
        _ => return response::bad_request("sortBy 参数非法"),
    };
    let (Some(start), Some(end)) = (query.start_time, query.end_time) else {
        return response::bad_request("缺少 startTime / endTime 参数");
    };
    if end <= start {
        return response::bad_request("endTime 必须大于 startTime");
    }

    // 排序方向：显式 asc/desc 优先；缺省时耗时类指标升序（越快越靠前），其余降序。
    let order_dir = match query.sort_order.as_deref() {
        Some("asc") => "ASC",
        Some("desc") => "DESC",
        _ => match sort_key {
            RankSortKey::Ttft | RankSortKey::RequestTime => "ASC",
            _ => "DESC",
        },
    };

    // 单查询聚合全部 6 个指标：仅成功请求 + start_time 半开窗口。
    let db = &state.db;
    let sql = r#"
        SELECT COALESCE(p.name, '') AS provider_name,
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
                   THEN 1.0 * SUM(r.input_cache_tokens) / SUM(r.input_tokens)
                   ELSE 0
               END AS cache_hit_rate
        FROM request r LEFT JOIN provider p ON p.id = r.provider_id
        WHERE r.success = 1 AND r.start_time >= ? AND r.start_time < ?
        GROUP BY p.name
    "#;

    let rows = match db
        .query_all_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [start.into(), end.into()],
        ))
        .await
    {
        Ok(rows) => rows,
        Err(e) => return response::db_error(e.to_string()),
    };

    let mut items = rows
        .iter()
        .map(|row| ProviderRankItem {
            provider_name: row.try_get("", "provider_name").unwrap_or_default(),
            request_count: row.try_get::<i64>("", "request_count").unwrap_or(0),
            total_tokens: row.try_get::<i64>("", "total_tokens").unwrap_or(0),
            ttft: row.try_get::<f64>("", "ttft").unwrap_or(0.0),
            request_time: row.try_get::<f64>("", "request_time").unwrap_or(0.0),
            tps: row.try_get::<f64>("", "tps").unwrap_or(0.0),
            cache_hit_rate: row.try_get::<f64>("", "cache_hit_rate").unwrap_or(0.0),
        })
        .collect::<Vec<_>>();

    // 排序在 Rust 侧做：NULL 聚合（如 ttft/cache_hit_rate 无可统计行）已由
    // SQL 归一为 0，直接按值比较即可。耗时类指标升序（0 值排最后），其余降序。
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
    items.sort_by(|a, b| {
        let cmp = value_of(a)
            .partial_cmp(&value_of(b))
            .unwrap_or(std::cmp::Ordering::Equal);
        if is_asc { cmp } else { cmp.reverse() }
    });

    (
        StatusCode::OK,
        Json(Response::success(ProviderRankResponse {
            start_time: start,
            end_time: end,
            items,
        })),
    )
}

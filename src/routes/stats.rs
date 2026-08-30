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

/// 赛马排行返回的供应商数量上限。
const RANK_LIMIT: u32 = 10;

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

/// 赛马指标：token 总额 / tps / ttft。
///
/// SQLite 的 SUM(INTEGER) 结果为 INTEGER、SUM(REAL)/AVG 为 REAL：token 用
/// i64 读取、tps/ttft 用 f64 读取，避免 sqlx 严格类型转换失败。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RankMetric {
    Token,
    Tps,
    Ttft,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderRankQuery {
    /// 指标：token | tps | ttft。
    metric: Option<String>,
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
    request_count: i64,
    value: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderRankResponse {
    metric: String,
    start_time: i64,
    end_time: i64,
    items: Vec<ProviderRankItem>,
}

/// 供应商维度赛马排行：按时间窗口聚合 request 表，返回 Top N 供应商。
///
/// - token：各供应商 `total_tokens` 合计（仅成功请求），降序。
/// - tps：Σ输出 token ÷ Σ网络耗时（耗时按 `output_tokens / tps` 反推，
///   仅计入 tps>0 且 output_tokens>0 的行），降序；分母为 0 时值记 0。
/// - ttft：流式请求（stream=1 且 ttft 非空）首 token 耗时均值，升序（最快在前）。
async fn provider_rank(
    State(state): State<AppState>,
    Query(query): Query<ProviderRankQuery>,
) -> impl IntoResponse {
    let metric = match query.metric.as_deref() {
        Some("token") => RankMetric::Token,
        Some("tps") => RankMetric::Tps,
        Some("ttft") => RankMetric::Ttft,
        _ => return response::bad_request("metric 参数非法（token | tps | ttft）"),
    };
    let (Some(start), Some(end)) = (query.start_time, query.end_time) else {
        return response::bad_request("缺少 startTime / endTime 参数");
    };
    if end <= start {
        return response::bad_request("endTime 必须大于 startTime");
    }

    let db = &state.db;
    let (value_expr, order_dir): (&str, &str) = match metric {
        RankMetric::Token => (
            "COALESCE(SUM(r.total_tokens), 0)",
            "DESC",
        ),
        RankMetric::Tps => {
            // 加权均值在 SQL 内算好，ORDER BY 直接按最终值排序。
            let secs = "SUM(CASE WHEN r.tps > 0 AND r.output_tokens > 0 THEN r.output_tokens / r.tps ELSE 0 END)";
            (
                &format!(
                    "CASE WHEN {secs} > 0 THEN COALESCE(SUM(r.output_tokens), 0) / {secs} ELSE 0 END"
                ),
                "DESC",
            )
        }
        RankMetric::Ttft => ("AVG(r.ttft)", "ASC"),
    };

    // 按窗口过滤 + 仅成功请求；ttft 只统计流式且有值的行。
    let extra_where = match metric {
        RankMetric::Ttft => " AND r.stream = 1 AND r.ttft IS NOT NULL",
        _ => "",
    };
    let sql = format!(
        "SELECT COALESCE(p.name, '') AS provider_name, COUNT(*) AS request_count, {value_expr} AS value \
         FROM request r LEFT JOIN provider p ON p.id = r.provider_id \
         WHERE r.success = 1 AND r.start_time >= ? AND r.start_time < ?{extra_where} \
         GROUP BY p.name ORDER BY value {order_dir} LIMIT {RANK_LIMIT}"
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
        Err(e) => return response::db_error(e.to_string()),
    };

    let items = rows
        .iter()
        .map(|row| ProviderRankItem {
            provider_name: row.try_get("", "provider_name").unwrap_or_default(),
            request_count: row.try_get("", "request_count").unwrap_or(0),
            value: match metric {
                // SUM(INTEGER) 结果为 INTEGER，先按 i64 读再转 f64。
                RankMetric::Token => row.try_get::<i64>("", "value").unwrap_or(0) as f64,
                _ => row.try_get::<f64>("", "value").unwrap_or(0.0),
            },
        })
        .collect::<Vec<_>>();

    (
        StatusCode::OK,
        Json(Response::success(ProviderRankResponse {
            metric: metric_name(metric).to_string(),
            start_time: start,
            end_time: end,
            items,
        })),
    )
}

fn metric_name(metric: RankMetric) -> &'static str {
    match metric {
        RankMetric::Token => "token",
        RankMetric::Tps => "tps",
        RankMetric::Ttft => "ttft",
    }
}

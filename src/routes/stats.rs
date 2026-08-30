use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::get};
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::Serialize;

use crate::response::{self, Response};
use crate::state::AppState;

const HOUR_MS: i64 = 3_600_000;
const TREND_BUCKETS: i64 = 24;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/summary", get(summary))
        .route("/charts", get(charts))
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

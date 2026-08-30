//! 请求日志查询：`GET /api/request-logs`。
//!
//! 服务端分页 + 过滤（时间段 / 虚拟模型 / API Key）+ 排序，返回全字段行
//! （含 JOIN 出的虚拟模型 displayId），供前端表格与行详情弹窗使用。

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

pub fn routes() -> Router<AppState> {
    Router::new().route("/", get(list_request_logs))
}

const DEFAULT_PAGE_SIZE: u32 = 20;
const MAX_PAGE_SIZE: u32 = 100;

/// 排序字段白名单（防止 SQL 注入）；key 为前端传入值，value 为真实列名。
const SORTABLE_COLUMNS: &[(&str, &str)] = &[
    ("startTime", "start_time"),
    ("requestTime", "request_time"),
    ("totalTokens", "total_tokens"),
    ("success", "success"),
    ("apiKeyName", "api_key_name"),
    ("virtualModelId", "virtual_model_id"),
    ("modelId", "model_id"),
    ("ttft", "ttft"),
    ("tps", "tps"),
];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListQuery {
    page: Option<u32>,
    page_size: Option<u32>,
    vm_id: Option<i32>,
    api_key: Option<String>,
    start_time: Option<i64>,
    end_time: Option<i64>,
    sort_by: Option<String>,
    sort_order: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RequestLogRow {
    request_id: String,
    virtual_model_id: i32,
    virtual_model_display_id: Option<String>,
    provider_id: i32,
    model_id: String,
    stream: bool,
    ttft: Option<i64>,
    input_tokens: Option<i64>,
    input_cache_tokens: i64,
    input_cache_rate: f64,
    output_tokens: Option<i64>,
    output_tokens_time: Option<i64>,
    tps: f64,
    start_time: i64,
    end_time: i64,
    request_time: i64,
    success: bool,
    fail_reason: Option<String>,
    total_tokens: Option<i64>,
    api_key_name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PageResponse {
    items: Vec<RequestLogRow>,
    total: i64,
    page: u32,
    page_size: u32,
}

async fn list_request_logs(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> impl IntoResponse {
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(DEFAULT_PAGE_SIZE).clamp(1, MAX_PAGE_SIZE);
    let offset = ((page - 1) * page_size) as i64;

    // 拼接 WHERE 条件与绑定参数。
    let mut where_sql = String::from("WHERE 1=1");
    let mut params: Vec<sea_orm::Value> = Vec::new();
    if let Some(vm_id) = query.vm_id {
        where_sql.push_str(" AND r.virtual_model_id = ?");
        params.push(vm_id.into());
    }
    if let Some(api_key) = query.api_key.filter(|k| !k.trim().is_empty()) {
        where_sql.push_str(" AND r.api_key_name = ?");
        params.push(api_key.trim().to_string().into());
    }
    if let Some(start) = query.start_time {
        where_sql.push_str(" AND r.start_time >= ?");
        params.push(start.into());
    }
    if let Some(end) = query.end_time {
        where_sql.push_str(" AND r.start_time <= ?");
        params.push(end.into());
    }

    // 排序：白名单映射 + 方向校验，默认 start_time DESC。
    let order_col = SORTABLE_COLUMNS
        .iter()
        .find(|(key, _)| Some(*key) == query.sort_by.as_deref())
        .map(|(_, col)| *col)
        .unwrap_or("start_time");
    let order_dir = match query.sort_order.as_deref() {
        Some("asc") => "ASC",
        _ => "DESC",
    };

    let count_sql = format!("SELECT COUNT(*) AS total FROM request r {where_sql}");
    let count_result = state
        .db
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            count_sql,
            params.clone(),
        ))
        .await;

    let total = match count_result {
        Ok(Some(row)) => row.try_get::<i64>("", "total").unwrap_or(0),
        Ok(None) => 0,
        Err(e) => return response::db_error(e.to_string()),
    };

    // 列表查询：LEFT JOIN virtual_model 补 display_id。
    let list_sql = format!(
        "SELECT r.*, vm.display_id AS virtual_model_display_id \
         FROM request r \
         LEFT JOIN virtual_model vm ON vm.virtual_model_id = r.virtual_model_id \
         {where_sql} \
         ORDER BY r.{order_col} {order_dir} \
         LIMIT ? OFFSET ?"
    );
    let mut list_params = params.clone();
    list_params.push((page_size as i64).into());
    list_params.push(offset.into());

    let rows = match state
        .db
        .query_all_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            list_sql,
            list_params,
        ))
        .await
    {
        Ok(rows) => rows,
        Err(e) => return response::db_error(e.to_string()),
    };

    let items = rows.iter().filter_map(|row| row_to_entry(row).ok()).collect();
    (
        StatusCode::OK,
        Json(Response::success(PageResponse {
            items,
            total,
            page,
            page_size,
        })),
    )
}

fn row_to_entry(row: &sea_orm::QueryResult) -> Result<RequestLogRow, sea_orm::TryGetError> {
    Ok(RequestLogRow {
        request_id: row.try_get("", "request_id")?,
        virtual_model_id: row.try_get("", "virtual_model_id")?,
        virtual_model_display_id: row.try_get("", "virtual_model_display_id").ok().flatten(),
        provider_id: row.try_get("", "provider_id")?,
        model_id: row.try_get("", "model_id")?,
        stream: row.try_get("", "stream")?,
        ttft: row.try_get("", "ttft")?,
        input_tokens: row.try_get("", "input_tokens")?,
        input_cache_tokens: row.try_get("", "input_cache_tokens")?,
        input_cache_rate: row.try_get("", "input_cache_rate")?,
        output_tokens: row.try_get("", "output_tokens")?,
        output_tokens_time: row.try_get("", "output_tokens_time")?,
        tps: row.try_get("", "tps")?,
        start_time: row.try_get("", "start_time")?,
        end_time: row.try_get("", "end_time")?,
        request_time: row.try_get("", "request_time")?,
        success: row.try_get("", "success")?,
        fail_reason: row.try_get("", "fail_reason")?,
        total_tokens: row.try_get("", "total_tokens")?,
        api_key_name: row.try_get("", "api_key_name")?,
    })
}

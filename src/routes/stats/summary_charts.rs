use super::*;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SummaryResponse {
    total_requests: i64,
    success_rate: f64,
    total_tokens: i64,
    cache_hit_rate: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrendPoint {
    /// 小时桶起点（毫秒时间戳）。
    pub(crate) bucket_start: i64,
    pub(crate) value: i64,
}

/// 浮点值趋势点（比率/速率类指标，如失败率、缓存命中率、输出 token/秒）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FloatTrendPoint {
    /// 桶起点（毫秒时间戳）。
    pub(crate) bucket_start: i64,
    pub(crate) value: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelValue {
    /// 实际服务的供应商名称（供应商已删除时为空串）。
    provider_name: String,
    model_id: String,
    value: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChartsResponse {
    call_trend: Vec<TrendPoint>,
    call_by_model: Vec<ModelValue>,
    token_trend: Vec<TrendPoint>,
    token_by_model: Vec<ModelValue>,
}

/// summary 可选时间窗口参数（均为可选；要么都缺省、要么都提供）。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SummaryQuery {
    /// 窗口起点（毫秒时间戳，含）。
    start_time: Option<i64>,
    /// 窗口终点（毫秒时间戳，不含）。
    end_time: Option<i64>,
}

/// 全量历史累计（可选时间窗口过滤）：累计请求数、成功率、总计 token、加权缓存命中率。
/// 不带 startTime/endTime 时保持全量聚合；两者同时提供时按 [start, end) 半开区间过滤。
pub(crate) async fn summary(
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
pub(crate) struct ChartsQuery {
    /// 窗口起点（毫秒时间戳，含）。
    pub(crate) start_time: Option<i64>,
    /// 窗口终点（毫秒时间戳，不含）。
    pub(crate) end_time: Option<i64>,
    /// 按供应商过滤（可选）。
    pub(crate) provider_id: Option<i32>,
    /// 按虚拟模型过滤（可选）。
    pub(crate) virtual_model_id: Option<i32>,
    /// 按模型 ID 过滤（可选；供应商侧真实模型 ID）。
    pub(crate) model_id: Option<String>,
    /// 按调用方 API Key 名称过滤（可选；request.api_key_name 精确匹配）。
    pub(crate) api_key: Option<String>,
    /// 桶粒度（hour/day/month/year）。缺省按窗口长度回退推断。
    pub(crate) granularity: Option<String>,
}

/// 图表数据：调用/ token 的趋势 + 按上游模型的分布。
///
/// 支持可选 startTime/endTime（缺省回退过去 24 小时）与 providerId 过滤；
/// 显式 granularity 时按设置表时区的自然边界分桶：
/// 小时/天桶对齐本地整点/午夜，月/年桶按自然月/年归并；granularity 缺省时
/// 按窗口长度回退（≤48h 小时桶、≤62 天天桶、其余 30 天块）。
pub(crate) async fn charts(
    State(state): State<AppState>,
    Query(query): Query<ChartsQuery>,
) -> impl IntoResponse {
    let explicit_granularity = match Granularity::parse(query.granularity.as_deref()) {
        Ok(g) => g,
        Err(msg) => return response::bad_request(msg),
    };
    let tz_offset_minutes = stats_tz_offset_minutes(query.start_time);
    let tz = chrono::FixedOffset::east_opt(tz_offset_minutes * 60)
        .unwrap_or_else(|| chrono::FixedOffset::east_opt(0).expect("0 偏移恒有效"));
    let window = resolve_chart_window(
        query.start_time,
        query.end_time,
        explicit_granularity,
        tz_offset_minutes,
    );
    let (window_start, window_end, granularity) = (window.start, window.end, window.granularity);

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
        let buckets = window.bucket_range();
        let fill_trend = |map: &std::collections::HashMap<i64, i64>| {
            buckets
                .clone()
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

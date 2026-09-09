use super::*;

/// 赛马排序指标。
///
/// SQLite 的 SUM(INTEGER) 结果为 INTEGER、SUM(REAL)/AVG 为 REAL：整数类
/// 指标（totalTokens/requestCount）用 i64 读取，其余用 f64，避免 sqlx
/// 严格类型转换失败。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RankSortKey {
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
pub(crate) struct RankQuery {
    /// 排序指标：totalTokens | requestCount | ttft | requestTime | tps | cacheHitRate。
    pub(crate) sort_by: Option<String>,
    /// 排序方向：asc | desc（缺省按指标默认方向）。
    pub(crate) sort_order: Option<String>,
    /// 窗口起点（毫秒时间戳，含）。
    pub(crate) start_time: Option<i64>,
    /// 窗口终点（毫秒时间戳，不含）。
    pub(crate) end_time: Option<i64>,
    /// 按供应商过滤（可选；provider_model_rank 使用）。
    pub(crate) provider_id: Option<i32>,
    /// 按虚拟模型过滤（可选；virtual_model_member_rank 使用）。
    pub(crate) virtual_model_id: Option<i32>,
    /// 按模型过滤（可选；api_key_rank 三级页使用，须与 provider_id 同传）。
    pub(crate) model_id: Option<String>,
    /// 按调用方 API Key 名称过滤（可选；API Key 数据面板「该 key 用到的 X」排行）。
    pub(crate) api_key: Option<String>,
}

/// 解析排序指标白名单；非法值返回 None（调用方转 400）。
pub(crate) fn parse_sort_key(sort_by: Option<&str>) -> Option<RankSortKey> {
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
pub(crate) fn sort_direction(sort_order: Option<&str>, sort_key: RankSortKey) -> &'static str {
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
pub(crate) fn sort_rank_rows<T>(rows: &mut [T], is_asc: bool, value_of: impl Fn(&T) -> f64) {
    rows.sort_by(|a, b| {
        let cmp = value_of(a)
            .partial_cmp(&value_of(b))
            .unwrap_or(std::cmp::Ordering::Equal);
        if is_asc { cmp } else { cmp.reverse() }
    });
}

// ── 赛马 6 指标行：公共解码 / 取值 / 过滤（五个 rank 端点共用） ──────────

/// 聚合行中的 6 个指标（列名与 `rank_metric_sql` 别名一一对应）。嵌入各
/// rank item 后经 `#[serde(flatten)]` 输出，JSON 形状与旧内联字段一致。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RankRowMetrics {
    /// 成功请求数。
    pub(crate) request_count: i64,
    /// 总计 token（成功请求的 total_tokens 合计）。
    pub(crate) total_tokens: i64,
    /// 流式请求（stream=1 且 ttft 非空）首 token 耗时均值（毫秒）。
    pub(crate) ttft: f64,
    /// 平均请求耗时（毫秒，成功请求 request_time 均值）。
    pub(crate) request_time: f64,
    /// TPS：Σ输出 token ÷ Σ网络耗时（耗时按 output_tokens/tps 反推，
    /// 仅计入 tps>0 且 output_tokens>0 的行）；分母为 0 时记 0。
    pub(crate) tps: f64,
    /// 缓存命中率：Σ输入缓存 token ÷ Σ输入 token（加权，无输入 token 时记 0）。
    pub(crate) cache_hit_rate: f64,
}

/// 排序键 → 指标数值（六个端点同一个取值口径）。
pub(crate) fn rank_metric_value(key: RankSortKey, metrics: &RankRowMetrics) -> f64 {
    match key {
        RankSortKey::TotalTokens => metrics.total_tokens as f64,
        RankSortKey::RequestCount => metrics.request_count as f64,
        RankSortKey::Ttft => metrics.ttft,
        RankSortKey::RequestTime => metrics.request_time,
        RankSortKey::Tps => metrics.tps,
        RankSortKey::CacheHitRate => metrics.cache_hit_rate,
    }
}

/// 行读取小助手（聚合维度列）。
/// 聚合行读取：整型维度列（缺省 0）。
pub(crate) fn row_i32(row: &sea_orm::QueryResult, column: &str) -> i32 {
    row.try_get::<i32>("", column).unwrap_or(0)
}

/// 聚合行读取：文本维度列（缺省空串）。
pub(crate) fn row_string(row: &sea_orm::QueryResult, column: &str) -> String {
    row.try_get("", column).unwrap_or_default()
}

use crate::stats_snapshot as snap;

/// 快照覆盖计划（rank 无粒度：day 级闭桶分解，缺桶自动兑底）。
pub(crate) async fn rank_coverage(
    db: &sea_orm::DatabaseConnection,
    start: i64,
    end: i64,
) -> Result<snap::Coverage, String> {
    let offset = super::stats_tz_offset_minutes(Some(start));
    let now = chrono::Utc::now().timestamp_millis();
    match snap::coverage(
        db,
        snap::Level::Day,
        offset,
        start,
        end,
        now,
        snap::MARGIN_MS,
    )
    .await
    {
        Ok(cov) => snap::trim_zero_prefix(db, cov)
            .await
            .map_err(|e| e.to_string()),
        Err(e) => Err(e.to_string()),
    }
}

/// 不可落快照的过滤形态：整窗兑底（保持正确性）。
pub(crate) fn demote(cov: &mut snap::Coverage, start: i64, end: i64, supported: bool) {
    if !supported {
        cov.snapshots.clear();
        cov.live = vec![(start, end)];
    }
}

/// 主体 → 展示名解析（LEFT JOIN 语义：缺失给空串）。
pub(crate) async fn resolve_names(
    db: &sea_orm::DatabaseConnection,
    table: &str,
    id_col: &str,
    name_col: &str,
    keys: &[String],
) -> Result<std::collections::HashMap<String, String>, String> {
    let mut map = std::collections::HashMap::new();
    if keys.is_empty() {
        return Ok(map);
    }
    let in_list = keys
        .iter()
        .map(|k| format!("'{k}'"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT CAST({id_col} AS TEXT) AS k, COALESCE({name_col}, '') AS n FROM {table} \
         WHERE CAST({id_col} AS TEXT) IN ({in_list})"
    );
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(DbBackend::Sqlite, sql, []))
        .await
        .map_err(|e| e.to_string())?;
    for row in rows {
        let k: String = row.try_get("", "k").unwrap_or_default();
        let n: String = row.try_get("", "n").unwrap_or_default();
        map.insert(k, n);
    }
    Ok(map)
}

/// 按固定顺序（providerId → virtualModelId → modelId → apiKey）追加可选过滤：
/// 占位符与参数一一对应，五个端点共用同一拼接实现。
pub(crate) fn push_rank_filters(
    where_sql: &mut String,
    params: &mut Vec<sea_orm::Value>,
    query: &RankQuery,
) {
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
    if let Some(api_key) = query.api_key.as_deref() {
        where_sql.push_str(" AND r.api_key_name = ?");
        params.push(api_key.into());
    }
}

/// 从查询参数解析排序指标与方向；参数缺失/非法返回错误响应。
/// T 为调用方成功响应的 data 类型（错误响应的 data 为空，仅用于类型对齐）。
pub(crate) fn parse_rank_query<T>(
    query: &RankQuery,
) -> Result<(RankSortKey, &'static str, i64, i64), response::ErrorResponse<T>> {
    let sort_key = parse_sort_key(query.sort_by.as_deref()).ok_or_else(|| {
        response::bad_request(
            AppSettings::lang_sync().tr("sortBy 参数非法", "invalid sortBy parameter"),
        )
    })?;
    let (start, end) = required_time_range(query.start_time, query.end_time)
        .map_err(|msg| response::bad_request::<T>(msg))?;
    let order_dir = sort_direction(query.sort_order.as_deref(), sort_key);
    Ok((sort_key, order_dir, start, end))
}

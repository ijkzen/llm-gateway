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

impl RankRowMetrics {
    /// 从聚合行解码（SQLite 数值列：先试 f64 再试 i64 的列在此全部按声明类型读）。
    fn from_row(row: &sea_orm::QueryResult) -> Self {
        Self {
            request_count: row.try_get::<i64>("", "request_count").unwrap_or(0),
            total_tokens: row.try_get::<i64>("", "total_tokens").unwrap_or(0),
            ttft: row.try_get::<f64>("", "ttft").unwrap_or(0.0),
            request_time: row.try_get::<f64>("", "request_time").unwrap_or(0.0),
            tps: row.try_get::<f64>("", "tps").unwrap_or(0.0),
            cache_hit_rate: row.try_get::<f64>("", "cache_hit_rate").unwrap_or(0.0),
        }
    }
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

/// 聚合行读取：可空整型维度列（如 provider_model/api_key 主键）。
pub(crate) fn row_opt_i32(row: &sea_orm::QueryResult, column: &str) -> Option<i32> {
    row.try_get("", column).ok()
}

/// 执行只读聚合 SQL（rank 端点共用）：DB 错误转统一响应文案。
pub(crate) async fn query_group_rank(
    db: &sea_orm::DatabaseConnection,
    sql: &str,
    params: Vec<sea_orm::Value>,
) -> Result<Vec<sea_orm::QueryResult>, String> {
    db.query_all_raw(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        sql,
        params,
    ))
    .await
    .map_err(|e| e.to_string())
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

/// 加权 TPS：Σ输出 token ÷ Σ网络耗时（耗时按 output_tokens/tps 反推，
/// 仅计入 tps>0 且 output_tokens>0 的行）；分母为 0 时记 0。
pub(crate) fn tps_sql(alias: &str) -> String {
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
pub(crate) fn cache_hit_rate_sql(alias: &str) -> String {
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
pub(crate) fn rank_metric_sql() -> String {
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderRankItem {
    /// 实际服务的供应商 ID（聚合维度；已删除供应商仍保留原始 id）。
    provider_id: i32,
    /// 实际服务的供应商名称（供应商已删除时为空串）。
    provider_name: String,
    #[serde(flatten)]
    metrics: RankRowMetrics,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderRankResponse {
    start_time: i64,
    end_time: i64,
    items: Vec<ProviderRankItem>,
}

/// 供应商维度赛马：按时间窗口聚合 request 表，一次查询返回全部供应商的
/// 6 个指标，按排序参数（缺省 totalTokens 降序）排序。
pub(crate) async fn provider_rank(
    State(state): State<AppState>,
    Query(query): Query<RankQuery>,
) -> Result<Json<Response<ProviderRankResponse>>, response::ErrorResponse<ProviderRankResponse>> {
    let (sort_key, order_dir, start, end) = parse_rank_query(&query)?;
    let db = &state.db;

    // 按 r.provider_id 分组（id 才是真实聚合维度，name 仅展示）。
    let mut where_sql = String::from("r.success = 1 AND r.start_time >= ? AND r.start_time < ?");
    let mut params: Vec<sea_orm::Value> = vec![start.into(), end.into()];
    push_rank_filters(&mut where_sql, &mut params, &query);
    let rank_sql = rank_metric_sql();
    let sql = format!(
        "SELECT r.provider_id AS provider_id, COALESCE(p.name, '') AS provider_name,{rank_sql} \
         FROM request r LEFT JOIN provider p ON p.id = r.provider_id \
         WHERE {where_sql} \
         GROUP BY r.provider_id"
    );

    let rows = query_group_rank(db, &sql, params)
        .await
        .map_err(response::db_error)?;

    let mut items = rows
        .iter()
        .map(|row| ProviderRankItem {
            provider_id: row_i32(row, "provider_id"),
            provider_name: row_string(row, "provider_name"),
            metrics: RankRowMetrics::from_row(row),
        })
        .collect::<Vec<_>>();

    sort_rank_rows(&mut items, order_dir == "ASC", |item| {
        rank_metric_value(sort_key, &item.metrics)
    });

    Ok(Json(Response::success(ProviderRankResponse {
        start_time: start,
        end_time: end,
        items,
    })))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VirtualModelRankItem {
    /// 虚拟模型 ID（聚合维度；已删除虚拟模型仍保留原始 id）。
    virtual_model_id: i32,
    /// 虚拟模型对外 ID（虚拟模型已删除时为空串）。
    virtual_model_display_id: String,
    #[serde(flatten)]
    metrics: RankRowMetrics,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VirtualModelRankResponse {
    start_time: i64,
    end_time: i64,
    items: Vec<VirtualModelRankItem>,
}

/// 虚拟模型维度赛马：规格与供应商赛马完全一致（6 指标 + 排序 + 时间窗口），
/// 仅聚合维度不同——按 request.virtual_model_id 分组，JOIN virtual_model 出
/// display_id；虚拟模型已删除时 LEFT JOIN 得 NULL，显示空串。
pub(crate) async fn virtual_model_rank(
    State(state): State<AppState>,
    Query(query): Query<RankQuery>,
) -> Result<
    Json<Response<VirtualModelRankResponse>>,
    response::ErrorResponse<VirtualModelRankResponse>,
> {
    let (sort_key, order_dir, start, end) = parse_rank_query(&query)?;
    let db = &state.db;

    // 按 id 分组（同一 display_id 的虚拟模型也各自成行），JOIN 出 display_id。
    let mut where_sql = String::from("r.success = 1 AND r.start_time >= ? AND r.start_time < ?");
    let mut params: Vec<sea_orm::Value> = vec![start.into(), end.into()];
    push_rank_filters(&mut where_sql, &mut params, &query);
    let rank_sql = rank_metric_sql();
    let sql = format!(
        "SELECT r.virtual_model_id AS virtual_model_id, \
                COALESCE(vm.display_id, '') AS virtual_model_display_id,{rank_sql} \
         FROM request r LEFT JOIN virtual_model vm ON vm.virtual_model_id = r.virtual_model_id \
         WHERE {where_sql} \
         GROUP BY r.virtual_model_id"
    );

    let rows = query_group_rank(db, &sql, params)
        .await
        .map_err(response::db_error)?;

    let mut items = rows
        .iter()
        .map(|row| VirtualModelRankItem {
            virtual_model_id: row_i32(row, "virtual_model_id"),
            virtual_model_display_id: row_string(row, "virtual_model_display_id"),
            metrics: RankRowMetrics::from_row(row),
        })
        .collect::<Vec<_>>();

    sort_rank_rows(&mut items, order_dir == "ASC", |item| {
        rank_metric_value(sort_key, &item.metrics)
    });

    Ok(Json(Response::success(VirtualModelRankResponse {
        start_time: start,
        end_time: end,
        items,
    })))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderModelRankItem {
    /// 实际服务的供应商 ID。
    provider_id: i32,
    /// 实际服务的供应商名称（供应商已删除时为空串）。
    provider_name: String,
    /// 模型 ID（供应商侧真实 ID；provider_model 行已删时退化为 request 里的原始串）。
    model_id: String,
    /// provider_model 自增主键（行已删时为 NULL，前端据此禁用跳转）。
    model_pk: Option<i32>,
    #[serde(flatten)]
    metrics: RankRowMetrics,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderModelRankResponse {
    start_time: i64,
    end_time: i64,
    items: Vec<ProviderModelRankItem>,
}

/// 供应商模型平铺赛马：规格与供应商/虚拟模型赛马完全一致（6 指标 + 排序 +
/// 时间窗口），行的含义 = 供应商的每个模型。按 (provider_id, model_id) 分组
/// （按 id 而非名称，避免不同供应商的相同模型 ID 被合并），JOIN provider 出
/// 供应商名、LEFT JOIN provider_model 兜底模型名（模型行已删时退化为 request
/// 里的原始 model_id）。
pub(crate) async fn provider_model_rank(
    State(state): State<AppState>,
    Query(query): Query<RankQuery>,
) -> Result<
    Json<Response<ProviderModelRankResponse>>,
    response::ErrorResponse<ProviderModelRankResponse>,
> {
    let (sort_key, order_dir, start, end) = parse_rank_query(&query)?;
    let db = &state.db;

    // 可选按供应商过滤（二级页用）：有 providerId 时只聚合该供应商内部模型。
    let mut where_sql = String::from("r.success = 1 AND r.start_time >= ? AND r.start_time < ?");
    let mut params: Vec<sea_orm::Value> = vec![start.into(), end.into()];
    push_rank_filters(&mut where_sql, &mut params, &query);

    let rank_sql = rank_metric_sql();
    let sql = format!(
        "SELECT r.provider_id AS provider_id, COALESCE(p.name, '') AS provider_name, \
                COALESCE(pm.provider_model_id, r.model_id) AS model_id, \
                pm.model_id AS model_pk,{rank_sql} \
         FROM request r \
         LEFT JOIN provider p ON p.id = r.provider_id \
         LEFT JOIN provider_model pm ON pm.provider_id = r.provider_id AND pm.provider_model_id = r.model_id \
         WHERE {where_sql} \
         GROUP BY r.provider_id, r.model_id"
    );

    let rows = query_group_rank(db, &sql, params)
        .await
        .map_err(response::db_error)?;

    let mut items = rows
        .iter()
        .map(|row| ProviderModelRankItem {
            provider_id: row_i32(row, "provider_id"),
            provider_name: row_string(row, "provider_name"),
            model_id: row_string(row, "model_id"),
            model_pk: row_opt_i32(row, "model_pk"),
            metrics: RankRowMetrics::from_row(row),
        })
        .collect::<Vec<_>>();

    sort_rank_rows(&mut items, order_dir == "ASC", |item| {
        rank_metric_value(sort_key, &item.metrics)
    });

    Ok(Json(Response::success(ProviderModelRankResponse {
        start_time: start,
        end_time: end,
        items,
    })))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VirtualModelMemberRankItem {
    /// 成员所属供应商 ID。
    provider_id: i32,
    /// 成员所属供应商名称（供应商已删除时为空串）。
    provider_name: String,
    /// 成员模型 ID（供应商侧真实 ID）。
    model_id: String,
    /// provider_model 自增主键（成员恒指向现存模型，恒非空）。
    model_pk: Option<i32>,
    /// 成员是否启用（virtual_model_item.enable；停用成员可正常展示但指标多为 0）。
    member_enable: bool,
    #[serde(flatten)]
    metrics: RankRowMetrics,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VirtualModelMemberRankResponse {
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
pub(crate) async fn virtual_model_member_rank(
    State(state): State<AppState>,
    Query(query): Query<RankQuery>,
) -> Result<
    Json<Response<VirtualModelMemberRankResponse>>,
    response::ErrorResponse<VirtualModelMemberRankResponse>,
> {
    let (sort_key, order_dir, start, end) = parse_rank_query(&query)?;
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
                pm.model_id AS model_pk, \
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
            provider_id: row_i32(row, "provider_id"),
            provider_name: row_string(row, "provider_name"),
            model_id: row_string(row, "model_id"),
            model_pk: row_opt_i32(row, "model_pk"),
            member_enable: row.try_get::<bool>("", "member_enable").unwrap_or(true),
            metrics: RankRowMetrics::from_row(row),
        })
        .collect::<Vec<_>>();

    // 无流量成员（request_count=0）始终排最后，避免升序时 0 值抢前；
    // 有流量成员组内按指标升/降序（partial_cmp 片段与 sort_rank_rows 同源，
    // 因 0 流量优先规则无法直接复用该 helper）。
    items.sort_by(|a, b| {
        let a_has_traffic = a.metrics.request_count > 0;
        let b_has_traffic = b.metrics.request_count > 0;
        match (a_has_traffic, b_has_traffic) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => {
                let cmp = rank_metric_value(sort_key, &a.metrics)
                    .partial_cmp(&rank_metric_value(sort_key, &b.metrics))
                    .unwrap_or(std::cmp::Ordering::Equal);
                if order_dir == "ASC" {
                    cmp
                } else {
                    cmp.reverse()
                }
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
pub(crate) struct ApiKeyRaceRankItem {
    /// 调用方 API Key 名称（request.api_key_name；Key 已删除的历史行仍按原名聚合）。
    api_key_name: String,
    /// 现存 API Key 的数字主键（JOIN api_key 按 name 补出；Key 已删除时为 null，不可跳转数据面板）。
    api_key_id: Option<i32>,
    #[serde(flatten)]
    metrics: RankRowMetrics,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiKeyRaceRankResponse {
    start_time: i64,
    end_time: i64,
    items: Vec<ApiKeyRaceRankItem>,
}

/// API Key 维度赛马：按 request.api_key_name 分组聚合 6 指标，规格与
/// 供应商赛马一致（排序 + 时间窗口）。可选过滤：
/// - provider_id：二级页（供应商详情）——只看该供应商的调用；
/// - virtual_model_id：二级页（虚拟模型详情）——只看该虚拟模型的调用；
/// - provider_id + model_id：三级页（模型详情）——只看该供应商下某模型的调用。
pub(crate) async fn api_key_rank(
    State(state): State<AppState>,
    Query(query): Query<RankQuery>,
) -> Result<Json<Response<ApiKeyRaceRankResponse>>, response::ErrorResponse<ApiKeyRaceRankResponse>>
{
    let (sort_key, order_dir, start, end) = parse_rank_query(&query)?;
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
    push_rank_filters(&mut where_sql, &mut params, &query);

    let rank_sql = rank_metric_sql();
    let sql = format!(
        "SELECT r.api_key_name AS api_key_name, k.id AS api_key_id,{rank_sql} \
         FROM request r \
         LEFT JOIN api_key k ON k.name = r.api_key_name \
         WHERE {where_sql} \
         GROUP BY r.api_key_name"
    );

    let rows = query_group_rank(db, &sql, params)
        .await
        .map_err(response::db_error)?;

    let mut items = rows
        .iter()
        .map(|row| ApiKeyRaceRankItem {
            api_key_name: row_string(row, "api_key_name"),
            api_key_id: row_opt_i32(row, "api_key_id"),
            metrics: RankRowMetrics::from_row(row),
        })
        .collect::<Vec<_>>();

    sort_rank_rows(&mut items, order_dir == "ASC", |item| {
        rank_metric_value(sort_key, &item.metrics)
    });

    Ok(Json(Response::success(ApiKeyRaceRankResponse {
        start_time: start,
        end_time: end,
        items,
    })))
}

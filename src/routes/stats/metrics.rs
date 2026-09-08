use super::*;

/// 模型详情查询参数：providerId + modelId + 时间窗口。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelMetricsQuery {
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
pub(crate) struct ModelMetricsResponse {
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
pub(crate) async fn model_metrics(
    State(state): State<AppState>,
    Query(query): Query<ModelMetricsQuery>,
) -> impl IntoResponse {
    let (Some(provider_id), Some(model_id)) = (query.provider_id, query.model_id) else {
        return response::bad_request(AppSettings::lang_sync().tr(
            "缺少 providerId / modelId 参数",
            "missing providerId / modelId parameters",
        ));
    };
    let (start, end) = match required_time_range(query.start_time, query.end_time) {
        Ok(range) => range,
        Err(msg) => return response::bad_request(msg),
    };
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
pub(crate) struct ApiKeyMetricsQuery {
    /// 调用方 API Key 名称（request.api_key_name，必填）。
    api_key: Option<String>,
    /// 窗口起点（毫秒时间戳，含）。
    start_time: Option<i64>,
    /// 窗口终点（毫秒时间戳，不含）。
    end_time: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiKeyMetricsResponse {
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
pub(crate) async fn api_key_metrics(
    State(state): State<AppState>,
    Query(query): Query<ApiKeyMetricsQuery>,
) -> impl IntoResponse {
    let Some(api_key) = query.api_key.as_deref().filter(|s| !s.is_empty()) else {
        return response::bad_request(
            AppSettings::lang_sync().tr("缺少 apiKey 参数", "missing apiKey parameter"),
        );
    };
    let (start, end) = match required_time_range(query.start_time, query.end_time) {
        Ok(range) => range,
        Err(msg) => return response::bad_request(msg),
    };
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
pub(crate) struct ProviderMetricsQuery {
    /// 供应商 ID（必填）。
    provider_id: Option<i32>,
    /// 窗口起点（毫秒时间戳，含）。
    start_time: Option<i64>,
    /// 窗口终点（毫秒时间戳，不含）。
    end_time: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderMetricsResponse {
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
pub(crate) async fn provider_metrics(
    State(state): State<AppState>,
    Query(query): Query<ProviderMetricsQuery>,
) -> impl IntoResponse {
    let Some(provider_id) = query.provider_id else {
        return response::bad_request(
            AppSettings::lang_sync().tr("缺少 providerId 参数", "missing providerId parameter"),
        );
    };
    let (start, end) = match required_time_range(query.start_time, query.end_time) {
        Ok(range) => range,
        Err(msg) => return response::bad_request(msg),
    };
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
pub(crate) struct VirtualModelMetricsQuery {
    /// 虚拟模型 ID（必填）。
    virtual_model_id: Option<i32>,
    /// 窗口起点（毫秒时间戳，含）。
    start_time: Option<i64>,
    /// 窗口终点（毫秒时间戳，不含）。
    end_time: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VirtualModelMetricsResponse {
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
pub(crate) async fn virtual_model_metrics(
    State(state): State<AppState>,
    Query(query): Query<VirtualModelMetricsQuery>,
) -> impl IntoResponse {
    let Some(virtual_model_id) = query.virtual_model_id else {
        return response::bad_request(AppSettings::lang_sync().tr(
            "缺少 virtualModelId 参数",
            "missing virtualModelId parameter",
        ));
    };
    let (start, end) = match required_time_range(query.start_time, query.end_time) {
        Ok(range) => range,
        Err(msg) => return response::bad_request(msg),
    };
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

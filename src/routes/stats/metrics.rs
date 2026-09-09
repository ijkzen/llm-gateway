use super::*;

use crate::stats_snapshot as snap;

/// 指标端点共用：闭桶读快照（精确主体键）+ 兑底段实时，返回 6 指标原语和。
/// subject_sql 追加 WHERE 片段与参数（如 `AND r.provider_id = ?`）。
#[allow(clippy::too_many_arguments)]
async fn metrics_prims(
    state: &AppState,
    start: i64,
    end: i64,
    snap_type: &str,
    snap_exact: Option<&str>,
    snap_required: bool,
    extra_where: &str,
    extra_params: Vec<sea_orm::Value>,
) -> Result<super::rank_snap::Prims, String> {
    let mut cov = super::rank::rank_coverage(&state.db, start, end).await?;
    // 主体键解析失败（provider_model/api_key 已删）时快照贡献必须为空：
    // 若仍按全量主体行取数会把其它主体加进来，高估数字 —— 直接整窗兑底。
    if snap_required && snap_exact.is_none() {
        cov.snapshots.clear();
        cov.live = vec![(start, end)];
    }
    let prim_list = rank_snap::prim_select_list();
    let grouped = |s: i64, e: i64| {
        let sql = format!(
            "SELECT 'x' AS key, {prim_list} FROM request r \
             WHERE r.start_time >= {s} AND r.start_time < {e} AND r.success = 1{extra_where}"
        );
        (sql, extra_params.clone())
    };
    // 快照侧按主体键（如 "1"）记账、兑底侧按哨兵键 "x" 记账：单主体端点
    // 全量加总（等价于同一主体的快照 + 兑底两部分）。
    let map =
        super::rank_snap::merged_prims(&state.db, &cov, snap_type, snap_exact, grouped).await?;
    let mut total = super::rank_snap::Prims::default();
    for prims in map.values() {
        for i in 0..super::rank_snap::PRIM_COUNT {
            total.0[i] += prims.0[i];
        }
    }
    Ok(total)
}

/// 主体展示名解析（LEFT JOIN 语义：缺失给空串）。
async fn subject_name(
    db: &sea_orm::DatabaseConnection,
    table: &str,
    id_col: &str,
    name_col: &str,
    id: i32,
) -> String {
    super::rank::resolve_names(db, table, id_col, name_col, &[id.to_string()])
        .await
        .unwrap_or_default()
        .get(&id.to_string())
        .cloned()
        .unwrap_or_default()
}

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

    // 快照主体键：pm 主键（模型行）；无法解析（pm 已删）→ 快照贡献为空，
    // 兑底段按 provider+model 原文过滤，与旧语义一致。
    let pm_id: Option<i64> = db
        .query_one_raw(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "SELECT model_id AS v FROM provider_model \
                 WHERE provider_id = {provider_id} AND provider_model_id = '{model_id}'"
            ),
        ))
        .await
        .ok()
        .flatten()
        .and_then(|row| row.try_get("", "v").ok());
    let prims = match metrics_prims(
        &state,
        start,
        end,
        snap::ENTITY_MODEL,
        pm_id.map(|v| v.to_string()).as_deref(),
        true,
        " AND r.provider_id = ? AND r.model_id = ?",
        vec![provider_id.into(), model_id.clone().into()],
    )
    .await
    {
        Ok(prims) => prims,
        Err(e) => return response::db_error(e),
    };
    let (request_count, total_tokens, ttft, request_time, tps, cache_hit_rate) = prims.derive();
    let provider_name = subject_name(db, "provider", "id", "name", provider_id).await;

    (
        StatusCode::OK,
        Json(Response::success(ModelMetricsResponse {
            provider_id,
            provider_name,
            model_id,
            request_count,
            total_tokens,
            ttft,
            request_time,
            tps,
            cache_hit_rate,
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

    // 快照主体键：Key 主键（已删 Key 无法解析 → 快照贡献为空，兑底按原名兜底）。
    let key_id: Option<i64> = db
        .query_one_raw(Statement::from_string(
            DbBackend::Sqlite,
            format!("SELECT id AS v FROM api_key WHERE name = '{api_key}'"),
        ))
        .await
        .ok()
        .flatten()
        .and_then(|row| row.try_get("", "v").ok());
    let prims = match metrics_prims(
        &state,
        start,
        end,
        snap::ENTITY_API_KEY,
        key_id.map(|v| v.to_string()).as_deref(),
        true,
        " AND r.api_key_name = ?",
        vec![api_key.into()],
    )
    .await
    {
        Ok(prims) => prims,
        Err(e) => return response::db_error(e),
    };
    let (request_count, total_tokens, ttft, request_time, tps, cache_hit_rate) = prims.derive();

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

    let prims = match metrics_prims(
        &state,
        start,
        end,
        snap::ENTITY_PROVIDER,
        Some(&provider_id.to_string()),
        false,
        " AND r.provider_id = ?",
        vec![provider_id.into()],
    )
    .await
    {
        Ok(prims) => prims,
        Err(e) => return response::db_error(e),
    };
    let (request_count, total_tokens, ttft, request_time, tps, cache_hit_rate) = prims.derive();
    let provider_name = subject_name(db, "provider", "id", "name", provider_id).await;

    (
        StatusCode::OK,
        Json(Response::success(ProviderMetricsResponse {
            provider_id,
            provider_name,
            request_count,
            total_tokens,
            ttft,
            request_time,
            tps,
            cache_hit_rate,
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

    let prims = match metrics_prims(
        &state,
        start,
        end,
        snap::ENTITY_VIRTUAL_MODEL,
        Some(&virtual_model_id.to_string()),
        false,
        " AND r.virtual_model_id = ?",
        vec![virtual_model_id.into()],
    )
    .await
    {
        Ok(prims) => prims,
        Err(e) => return response::db_error(e),
    };
    let (request_count, total_tokens, ttft, request_time, tps, cache_hit_rate) = prims.derive();
    let virtual_model_display_id = subject_name(
        db,
        "virtual_model",
        "virtual_model_id",
        "display_id",
        virtual_model_id,
    )
    .await;

    (
        StatusCode::OK,
        Json(Response::success(VirtualModelMetricsResponse {
            virtual_model_id,
            virtual_model_display_id,
            request_count,
            total_tokens,
            ttft,
            request_time,
            tps,
            cache_hit_rate,
        })),
    )
}

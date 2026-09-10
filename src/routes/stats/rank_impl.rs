use super::*;

use super::rank::{
    RankQuery, RankRowMetrics, demote, parse_rank_query, push_rank_filters, rank_coverage,
    rank_metric_value, resolve_names, row_i32, row_string, sort_rank_rows,
};
use super::rank_snap;
use crate::stats_snapshot as snap;

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

    // 覆盖计划：闭桶（provider 行）读快照，其余兑底；跨维过滤形态整窗兑底。
    let mut cov = rank_coverage(db, start, end)
        .await
        .map_err(response::db_error)?;
    let supported =
        query.virtual_model_id.is_none() && query.model_id.is_none() && query.api_key.is_none();
    demote(&mut cov, start, end, supported);

    let (snap_type, snap_exact) = if supported {
        (
            snap::ENTITY_PROVIDER,
            query.provider_id.map(|p| p.to_string()),
        )
    } else {
        (snap::ENTITY_PROVIDER, None)
    };

    // 兑底分组：key = provider id 文本（与快照 entity 同域）；过滤统一追加
    //（跨维形态已 demote 为整窗兑底，过滤仍须与旧语义一致）。
    let prim_list = rank_snap::prim_select_list();
    let grouped = |s: i64, e: i64| {
        let mut sql = format!(
            "SELECT CAST(r.provider_id AS TEXT) AS key, {prim_list} FROM request r \
             WHERE r.start_time >= {s} AND r.start_time < {e} AND r.success = 1"
        );
        let mut params: Vec<sea_orm::Value> = Vec::new();
        push_rank_filters(&mut sql, &mut params, &query);
        sql.push_str(" GROUP BY r.provider_id");
        (sql, params)
    };
    let prims =
        super::rank_snap::merged_prims(db, &cov, snap_type, snap_exact.as_deref(), None, grouped)
            .await
            .map_err(response::db_error)?;

    // 展示名解析 + 组装 + 排序（平局序按 id 升序，近似旧 SQL 分组序）。
    let mut keys: Vec<String> = prims.keys().cloned().collect();
    keys.sort_by_key(|k| k.parse::<i64>().unwrap_or(i64::MAX));
    let names = resolve_names(db, "provider", "id", "name", &keys)
        .await
        .map_err(response::db_error)?;
    let mut items: Vec<ProviderRankItem> = Vec::with_capacity(keys.len());
    for key in keys {
        let prims = &prims[&key];
        let (request_count, total_tokens, ttft, request_time, tps, cache_hit_rate) = prims.derive();
        if request_count == 0 {
            continue; // 只有失败请求的供应商在 success=1 口径下不成行
        }
        items.push(ProviderRankItem {
            provider_id: key.parse().unwrap_or(0),
            provider_name: names.get(&key).cloned().unwrap_or_default(),
            metrics: RankRowMetrics {
                request_count,
                total_tokens,
                ttft,
                request_time,
                tps,
                cache_hit_rate,
            },
        });
    }

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

    // 覆盖计划：闭桶（virtual_model 行）读快照，其余兑底；带过滤时整窗兑底。
    let mut cov = rank_coverage(db, start, end)
        .await
        .map_err(response::db_error)?;
    let supported = query.provider_id.is_none()
        && query.virtual_model_id.is_none()
        && query.model_id.is_none()
        && query.api_key.is_none();
    demote(&mut cov, start, end, supported);

    let prim_list = rank_snap::prim_select_list();
    let grouped = |s: i64, e: i64| {
        let mut sql = format!(
            "SELECT CAST(r.virtual_model_id AS TEXT) AS key, {prim_list} FROM request r \
             WHERE r.start_time >= {s} AND r.start_time < {e} AND r.success = 1"
        );
        let mut params: Vec<sea_orm::Value> = Vec::new();
        push_rank_filters(&mut sql, &mut params, &query);
        sql.push_str(" GROUP BY r.virtual_model_id");
        (sql, params)
    };
    let prims =
        super::rank_snap::merged_prims(db, &cov, snap::ENTITY_VIRTUAL_MODEL, None, None, grouped)
            .await
            .map_err(response::db_error)?;

    let mut keys: Vec<String> = prims.keys().cloned().collect();
    keys.sort_by_key(|k| k.parse::<i64>().unwrap_or(i64::MAX));
    let display = resolve_names(db, "virtual_model", "virtual_model_id", "display_id", &keys)
        .await
        .map_err(response::db_error)?;
    let mut items: Vec<VirtualModelRankItem> = Vec::with_capacity(keys.len());
    for key in keys {
        let (request_count, total_tokens, ttft, request_time, tps, cache_hit_rate) =
            prims[&key].derive();
        if request_count == 0 {
            continue;
        }
        items.push(VirtualModelRankItem {
            virtual_model_id: key.parse().unwrap_or(0),
            virtual_model_display_id: display.get(&key).cloned().unwrap_or_default(),
            metrics: RankRowMetrics {
                request_count,
                total_tokens,
                ttft,
                request_time,
                tps,
                cache_hit_rate,
            },
        });
    }

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

    // 覆盖计划：闭桶（model 行）读快照；支持 ∅/providerId(/modelId 精确) 形态，
    // 跨维（vm/apiKey）整窗兑底。
    let mut cov = rank_coverage(db, start, end)
        .await
        .map_err(response::db_error)?;
    let supported = query.virtual_model_id.is_none() && query.api_key.is_none();
    demote(&mut cov, start, end, supported);

    // 过滤形态与快照主体集合：∅ 全量；providerId+modelId 精确取该 pm 键（键
    // 解析失败整窗兑底）；单侧（仅 providerId 或仅 modelId）取该侧下全部 pm
    // 键集合 —— 快照只读集合内主体行，避免全量 model 行混入其它供应商/模型。
    let exact_shape = matches!(
        (query.provider_id, query.model_id.as_deref()),
        (Some(_), Some(_))
    );
    let exact = match (query.provider_id, query.model_id.as_deref()) {
        (Some(p), Some(m)) => snap::resolve_pm_key(db, p, m).await,
        _ => None,
    };
    if exact_shape {
        snap::demote_if_unresolved(&mut cov, start, end, snap::ENTITY_MODEL, exact.as_deref());
    }
    let entities: Option<Vec<String>> = if exact_shape {
        None
    } else {
        match (query.provider_id, query.model_id.as_deref()) {
            (Some(_), None) | (None, Some(_)) => {
                let keys = snap::resolve_pm_keys_for_filter(
                    db,
                    query.provider_id,
                    query.model_id.as_deref(),
                )
                .await;
                Some(keys)
            }
            _ => None,
        }
    };
    let provider_filter = query.provider_id;

    let prim_list = rank_snap::prim_select_list();
    // 兑底分组：key = pm 主键文本（与快照 model 行同域）；模型/供应商已删
    // （pm 映射不到）的行退化为 raw 键（providerId + 原始 model 串），与旧
    // 实现「COALESCE(pm.provider_model_id, r.model_id)」的孤儿行语义一致。
    let raw_prefix = "raw:";
    let grouped = |s: i64, e: i64| {
        let mut sql = format!(
            "SELECT CASE WHEN pm.model_id IS NULL \
                    THEN '{raw_prefix}' || CAST(r.provider_id AS TEXT) || '|' || r.model_id \
                    ELSE CAST(pm.model_id AS TEXT) END AS key, {prim_list} \
             FROM request r \
             LEFT JOIN provider_model pm ON pm.provider_id = r.provider_id \
                                          AND pm.provider_model_id = r.model_id \
             WHERE r.start_time >= {s} AND r.start_time < {e} AND r.success = 1"
        );
        let mut params: Vec<sea_orm::Value> = Vec::new();
        push_rank_filters(&mut sql, &mut params, &query);
        sql.push_str(" GROUP BY key");
        (sql, params)
    };
    let prims = super::rank_snap::merged_prims(
        db,
        &cov,
        snap::ENTITY_MODEL,
        exact.as_deref(),
        entities.as_deref(),
        grouped,
    )
    .await
    .map_err(response::db_error)?;

    // 展示解析：pm 键 → (供应商 id/名, 模型串, pk)；raw 键按孤儿行解析。
    let keys: Vec<String> = prims.keys().cloned().collect();
    let mut meta: std::collections::HashMap<String, (i32, String, String, Option<i32>)> =
        std::collections::HashMap::new();
    let pm_keys: Vec<&String> = keys.iter().filter(|k| !k.starts_with(raw_prefix)).collect();
    if !pm_keys.is_empty() {
        let in_list = pm_keys
            .iter()
            .map(|k| format!("'{k}'"))
            .collect::<Vec<_>>()
            .join(", ");
        let mut sql = format!(
            "SELECT CAST(pm.model_id AS TEXT) AS k, pm.provider_id AS pid, \
                    COALESCE(p.name, '') AS pname, pm.provider_model_id AS mid \
             FROM provider_model pm LEFT JOIN provider p ON p.id = pm.provider_id \
             WHERE CAST(pm.model_id AS TEXT) IN ({in_list})"
        );
        if let Some(p) = provider_filter {
            sql.push_str(&format!(" AND pm.provider_id = {p}"));
        }
        let rows = db
            .query_all_raw(Statement::from_string(DbBackend::Sqlite, sql))
            .await
            .map_err(|e| response::db_error(e.to_string()))?;
        for row in rows {
            let k: String = row.try_get("", "k").unwrap_or_default();
            let pid: i32 = row.try_get("", "pid").unwrap_or(0);
            let pname: String = row.try_get("", "pname").unwrap_or_default();
            let mid: String = row.try_get("", "mid").unwrap_or_default();
            meta.insert(k.clone(), (pid, pname, mid, k.parse().ok()));
        }
    }
    // raw 键：providerId|model 拆回，供应商名 LEFT JOIN（已删为空串）。
    let raw_ids: Vec<i32> = keys
        .iter()
        .filter_map(|k| {
            k.strip_prefix(raw_prefix)
                .and_then(|rest| rest.split('|').next())
                .and_then(|pid| pid.parse().ok())
        })
        .collect();
    let provider_names = resolve_names(
        db,
        "provider",
        "id",
        "name",
        &raw_ids.iter().map(|v| v.to_string()).collect::<Vec<_>>(),
    )
    .await
    .map_err(response::db_error)?;
    for key in &keys {
        let parsed = key
            .strip_prefix(raw_prefix)
            .and_then(|r| r.split_once('|'))
            .and_then(|(pid, mid)| pid.parse::<i32>().ok().map(|pid| (pid, mid)));
        if let Some((pid, mid)) = parsed {
            meta.insert(
                key.clone(),
                (
                    pid,
                    provider_names
                        .get(&pid.to_string())
                        .cloned()
                        .unwrap_or_default(),
                    mid.to_string(),
                    None,
                ),
            );
        }
    }

    let mut items: Vec<ProviderModelRankItem> = Vec::new();
    for (key, prims) in &prims {
        let (request_count, total_tokens, ttft, request_time, tps, cache_hit_rate) = prims.derive();
        if request_count == 0 {
            continue;
        }
        let Some((provider_id, provider_name, model_id, model_pk)) = meta.get(key) else {
            continue;
        };
        items.push(ProviderModelRankItem {
            provider_id: *provider_id,
            provider_name: provider_name.clone(),
            model_id: model_id.clone(),
            model_pk: *model_pk,
            metrics: RankRowMetrics {
                request_count,
                total_tokens,
                ttft,
                request_time,
                tps,
                cache_hit_rate,
            },
        });
    }
    // 稳定排序平局序：按 (供应商, 模型串) 预排（近似 SQL GROUP BY 的 B-tree 序）。
    items.sort_by(|a, b| (a.provider_id, &a.model_id).cmp(&(b.provider_id, &b.model_id)));

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

    // 覆盖计划：闭桶（vm_member 行）读快照；兑底段按「该虚拟模型实际服务的
    // 成员」聚合（映射不到 pm 的孤儿行丢弃——成员恒指向现存模型）。
    let cov = rank_coverage(db, start, end)
        .await
        .map_err(response::db_error)?;

    let prim_list = rank_snap::prim_select_list();
    let member_key = format!("{virtual_model_id},");
    let grouped = |s: i64, e: i64| {
        (
            format!(
                "SELECT CASE WHEN pm.model_id IS NULL THEN NULL \
                        ELSE CAST(r.virtual_model_id AS TEXT) || ',' || CAST(pm.model_id AS TEXT) END \
                        AS key, {prim_list} \
                 FROM request r \
                 LEFT JOIN provider_model pm ON pm.provider_id = r.provider_id \
                                              AND pm.provider_model_id = r.model_id \
                 WHERE r.start_time >= {s} AND r.start_time < {e} AND r.success = 1 \
                   AND r.virtual_model_id = ? AND pm.model_id IS NOT NULL \
                 GROUP BY key",
            ),
            vec![virtual_model_id.into()],
        )
    };
    // 快照侧先取全部 vm_member 行，再按 vm 前缀过滤（与兑底键同域 "vm,pm"）。
    let mut traffic =
        super::rank_snap::merged_prims(db, &cov, snap::ENTITY_VM_MEMBER, None, None, grouped)
            .await
            .map_err(response::db_error)?;
    traffic.retain(|key, _| key.starts_with(&member_key));

    // 成员配置（左表）：展示全部成员，无流量成员指标为 0。
    let rows = match db
        .query_all_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT pm.provider_id AS provider_id, COALESCE(p.name, '') AS provider_name, \
                    pm.provider_model_id AS model_id, \
                    pm.model_id AS model_pk, \
                    vmi.enable AS member_enable \
             FROM virtual_model_item vmi \
             JOIN provider_model pm ON pm.model_id = vmi.model_id \
             LEFT JOIN provider p ON p.id = pm.provider_id \
             WHERE vmi.virtual_model_id = ? \
             ORDER BY pm.model_id",
            [virtual_model_id.into()],
        ))
        .await
    {
        Ok(rows) => rows,
        Err(e) => return Err(response::db_error(e.to_string())),
    };

    let mut items = Vec::new();
    for row in &rows {
        let member_pk: Option<i32> = row.try_get("", "model_pk").ok();
        let key = member_pk
            .map(|pk| format!("{virtual_model_id},{pk}"))
            .unwrap_or_default();
        let prims = traffic.remove(&key).unwrap_or_default();
        let (request_count, total_tokens, ttft, request_time, tps, cache_hit_rate) = prims.derive();
        items.push(VirtualModelMemberRankItem {
            provider_id: row_i32(row, "provider_id"),
            provider_name: row_string(row, "provider_name"),
            model_id: row_string(row, "model_id"),
            model_pk: member_pk,
            member_enable: row.try_get::<bool>("", "member_enable").unwrap_or(true),
            metrics: RankRowMetrics {
                request_count,
                total_tokens,
                ttft,
                request_time,
                tps,
                cache_hit_rate,
            },
        });
    }

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

    // 覆盖计划：闭桶读快照、其余兑底。快照主体：
    // - ∅ → api_key 行；providerId / providerId+modelId → api_key_model 行按
    //   pm 归属过滤（key×模型 交叉行）；virtualModelId → 无对应行形态，整窗兑底。
    let mut cov = rank_coverage(db, start, end)
        .await
        .map_err(response::db_error)?;
    let use_key_model = query.provider_id.is_some();
    let supported = query.virtual_model_id.is_none();
    demote(&mut cov, start, end, supported);

    // 快照允许的 pm 集合（provider 过滤 / 精确 pm）；∅ 形态不用过滤。
    let allowed_pm: std::collections::HashSet<String> = if use_key_model {
        let (sql, params): (String, Vec<sea_orm::Value>) =
            match (query.provider_id, query.model_id.as_deref()) {
                (Some(p), Some(m)) => (
                    "SELECT CAST(model_id AS TEXT) AS v FROM provider_model \
                     WHERE provider_id = ? AND provider_model_id = ?"
                        .to_string(),
                    vec![p.into(), m.into()],
                ),
                (Some(p), None) => (
                    "SELECT CAST(model_id AS TEXT) AS v FROM provider_model WHERE provider_id = ?"
                        .to_string(),
                    vec![p.into()],
                ),
                _ => (String::new(), Vec::new()),
            };
        if sql.is_empty() {
            std::collections::HashSet::new()
        } else {
            let rows = db
                .query_all_raw(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    sql,
                    params,
                ))
                .await
                .map_err(|e| response::db_error(e.to_string()))?;
            rows.iter()
                .filter_map(|row| row.try_get::<String>("", "v").ok())
                .collect()
        }
    } else {
        std::collections::HashSet::new()
    };

    // 快照侧：entity 键 → Prims；再按「id → 名称」归并到名称域（已删 Key 的
    // 闭桶贡献按生成语义丢弃；兑底段孤儿名称行保留）。
    let mut by_id: std::collections::HashMap<String, super::rank_snap::Prims> =
        std::collections::HashMap::new();
    let mut by_level: std::collections::BTreeMap<snap::Level, Vec<snap::Frame>> =
        std::collections::BTreeMap::new();
    for frame in &cov.snapshots {
        by_level.entry(frame.level).or_default().push(*frame);
    }
    let snap_type = if use_key_model {
        snap::ENTITY_API_KEY_MODEL
    } else {
        snap::ENTITY_API_KEY
    };
    let prim_names: Vec<&str> = snap::success_prims().map(|(m, _)| m).collect();
    for (level, frames) in &by_level {
        let rows = snap::snapshot_rows(db, *level, frames, snap_type, None, None, &prim_names)
            .await
            .map_err(|e| response::db_error(e.to_string()))?;
        for (_, _, entity, metric, value) in rows {
            // api_key_model 形态：只保留 pm 在允许集合内的行，并按 key 部分聚合。
            let key = if use_key_model {
                let Some((ak, pm)) = entity.split_once(',') else {
                    continue;
                };
                if !allowed_pm.contains(pm) {
                    continue;
                }
                ak.to_string()
            } else {
                entity.clone()
            };
            if let Some(i) = prim_names.iter().position(|m| *m == metric) {
                by_id.entry(key).or_default().0[i] += value;
            }
        }
    }

    // 兑底侧：按 api_key_name 分组（旧口径），孤儿（Key 已删）行保留原名。
    let prim_list = rank_snap::prim_select_list();
    let grouped = |s: i64, e: i64| {
        let mut sql = format!(
            "SELECT r.api_key_name AS key, {prim_list} FROM request r \
             WHERE r.start_time >= {s} AND r.start_time < {e} AND r.success = 1"
        );
        let mut params: Vec<sea_orm::Value> = Vec::new();
        if let Some(p) = query.provider_id {
            sql.push_str(" AND r.provider_id = ?");
            params.push(p.into());
        }
        if let Some(vm) = query.virtual_model_id {
            sql.push_str(" AND r.virtual_model_id = ?");
            params.push(vm.into());
        }
        if let Some(m) = query.model_id.as_deref() {
            sql.push_str(" AND r.model_id = ?");
            params.push(m.into());
        }
        sql.push_str(" GROUP BY r.api_key_name");
        (sql, params)
    };
    super::rank_snap::fold_live(db, &cov, grouped, &mut by_id)
        .await
        .map_err(response::db_error)?;

    // 名称域归并：快照 id 键解析名称（已删 Key 的贡献按生成语义丢弃）、
    // 兑底名称键原样保留（subject 归并助手，与 insight 同一实现）。
    let by_name = snap::api_key_reconcile_names(db, by_id)
        .await
        .map_err(response::db_error)?;

    let mut items: Vec<ApiKeyRaceRankItem> = Vec::new();
    for (name, prims) in by_name {
        let (request_count, total_tokens, ttft, request_time, tps, cache_hit_rate) = prims.derive();
        if request_count == 0 {
            continue;
        }
        // 参数绑定（10-04）：名称来自本仓 api_key.name（仅 trim 校验、无字符集
        // 限制），内联拼接会被含单引号的名称破坏。
        let api_key_id: Option<i32> = db
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id AS v FROM api_key WHERE name = ?",
                [name.clone().into()],
            ))
            .await
            .map_err(|e| response::db_error(e.to_string()))?
            .and_then(|row| row.try_get::<i32>("", "v").ok());
        items.push(ApiKeyRaceRankItem {
            api_key_name: name,
            api_key_id,
            metrics: RankRowMetrics {
                request_count,
                total_tokens,
                ttft,
                request_time,
                tps,
                cache_hit_rate,
            },
        });
    }

    sort_rank_rows(&mut items, order_dir == "ASC", |item| {
        rank_metric_value(sort_key, &item.metrics)
    });

    Ok(Json(Response::success(ApiKeyRaceRankResponse {
        start_time: start,
        end_time: end,
        items,
    })))
}

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

use crate::stats_snapshot as snap;

/// 汇总兑底段原始聚合（谓词取自 registry 指标表，与生成端同一文本；
/// SUMMARY_METRICS 键序即列序）。
async fn summary_live(
    db: &sea_orm::DatabaseConnection,
    segs: &[(i64, i64)],
) -> anyhow::Result<(i64, i64, i64, i64, i64)> {
    let select = snap::select_list(&SUMMARY_METRICS);
    let mut totals = (0i64, 0i64, 0i64, 0i64, 0i64);
    for (s, e) in segs {
        let row = db
            .query_one_raw(Statement::from_string(
                DbBackend::Sqlite,
                format!(
                    "SELECT {select} FROM request r \
                     WHERE r.start_time >= {s} AND r.start_time < {e}"
                ),
            ))
            .await?;
        if let Some(row) = row {
            totals.0 += row.try_get::<i64>("", snap::metrics::CALLS).unwrap_or(0);
            totals.1 += row
                .try_get::<i64>("", snap::metrics::SUCCESS_CALLS)
                .unwrap_or(0);
            totals.2 += row
                .try_get::<i64>("", snap::metrics::TOKENS_ALL)
                .unwrap_or(0);
            totals.3 += row
                .try_get::<i64>("", snap::metrics::INPUT_TOKENS_ALL)
                .unwrap_or(0);
            totals.4 += row
                .try_get::<i64>("", snap::metrics::CACHE_TOKENS_ALL)
                .unwrap_or(0);
        }
    }
    Ok(totals)
}

const SUMMARY_METRICS: [&str; 5] = [
    snap::metrics::CALLS,
    snap::metrics::SUCCESS_CALLS,
    snap::metrics::TOKENS_ALL,
    snap::metrics::INPUT_TOKENS_ALL,
    snap::metrics::CACHE_TOKENS_ALL,
];

/// charts 快照读的指标名单（10-06：原为手写字面量，指标改名会静默漏行）。
/// 键序即快照读列序，与下文取值下标一一对应。
const CHARTS_SNAP_METRICS: [&str; 2] = [snap::metrics::CALLS, snap::metrics::TOKENS_ALL];

/// 全量历史累计（可选时间窗口过滤）：累计请求数、成功率、总计 token、加权缓存命中率。
/// 闭桶（day 帧）读快照，其余兑底；数字与实时口径一致（快照只是加速层）。
pub(crate) async fn summary(
    State(state): State<AppState>,
    Query(query): Query<SummaryQuery>,
) -> impl IntoResponse {
    // 无参数 = 全量历史（含起点之前与当前未闭/未来行）：兑底段上不封顶。
    let all_time = query.start_time.is_none() && query.end_time.is_none();
    let (start, end) = match (query.start_time, query.end_time) {
        (Some(start), Some(end)) if end > start => (start, end),
        (None, None) => (0, chrono::Utc::now().timestamp_millis()),
        _ => {
            return response::bad_request(AppSettings::lang_sync().tr(
                "startTime 与 endTime 必须同时提供且 endTime 晚于 startTime",
                "startTime and endTime must both be provided with endTime after startTime",
            ));
        }
    };
    let db = &state.db;
    let offset = stats_tz_offset_minutes(Some(start));
    let now = chrono::Utc::now().timestamp_millis();
    let mut coverage = match snap::coverage(
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
        Ok(cov) => match snap::trim_zero_prefix(db, cov).await {
            Ok(cov) => cov,
            Err(e) => return response::db_error(e.to_string()),
        },
        Err(e) => return response::db_error(e.to_string()),
    };
    if all_time {
        // 全量语义上不封顶：now 之后的实时行也计入（旧 SQL 无上界）。
        let last_end = coverage.live.last().map(|(_, e)| *e).unwrap_or(start);
        if last_end < i64::MAX {
            coverage.live.push((last_end.max(end), i64::MAX));
        }
    }

    // 覆盖帧可能混含 day 帧（整闭日）与 hour 帧（今天 00:00 起已闭小时）：
    // 按帧层级分组取 whole 行加总，任何层级都不可丢（兑底段只覆盖未闭部分）。
    let mut by_level: std::collections::BTreeMap<snap::Level, Vec<snap::Frame>> =
        std::collections::BTreeMap::new();
    for frame in &coverage.snapshots {
        by_level.entry(frame.level).or_default().push(*frame);
    }
    let mut sums = [0f64; 5];
    for (level, frames) in &by_level {
        let rows = match snap::snapshot_rows(
            db,
            *level,
            frames,
            snap::ENTITY_WHOLE,
            Some(""),
            None,
            &SUMMARY_METRICS,
        )
        .await
        {
            Ok(rows) => rows,
            Err(e) => return response::db_error(e.to_string()),
        };
        for (_, _, _, m, v) in rows {
            if let Some(i) = SUMMARY_METRICS.iter().position(|x| *x == m) {
                sums[i] += v;
            }
        }
    }
    let (l1, l2, l3, l4, l5) = match summary_live(db, &coverage.live).await {
        Ok(v) => v,
        Err(e) => return response::db_error(e.to_string()),
    };

    let total_requests = sums[0] as i64 + l1;
    let success_count = sums[1] as i64 + l2;
    let total_tokens = sums[2] as i64 + l3;
    let input_tokens = sums[3] as i64 + l4;
    let cache_tokens = sums[4] as i64 + l5;
    (
        StatusCode::OK,
        Json(Response::success(SummaryResponse {
            total_requests,
            success_rate: weighted_ratio(success_count as f64, total_requests as f64),
            total_tokens,
            cache_hit_rate: weighted_ratio(cache_tokens as f64, input_tokens as f64),
        })),
    )
}

/// 过滤子句与参数（live SQL 共用；charts/insight 同级模块复用）。
pub(crate) fn filter_parts(query: &ChartsQuery) -> (String, Vec<sea_orm::Value>) {
    let mut sql = String::new();
    let mut params: Vec<sea_orm::Value> = Vec::new();
    if let Some(provider_id) = query.provider_id {
        sql.push_str(" AND r.provider_id = ?");
        params.push(provider_id.into());
    }
    if let Some(virtual_model_id) = query.virtual_model_id {
        sql.push_str(" AND r.virtual_model_id = ?");
        params.push(virtual_model_id.into());
    }
    if let Some(model_id) = query.model_id.as_deref() {
        sql.push_str(" AND r.model_id = ?");
        params.push(model_id.into());
    }
    if let Some(api_key) = query.api_key.as_deref() {
        sql.push_str(" AND r.api_key_name = ?");
        params.push(api_key.into());
    }
    (sql, params)
}

/// 趋势主体解析（快照侧）：页面实际使用的单维过滤形态 → (entity_type, 键)。
/// None = 该形态不落快照（整窗兑底，保证正确）。charts/insight 同级模块复用。
/// 键解析失败（pm/Key 已删）同样返回 None——快照主体行按全量取数会把其它
/// 主体加进来（subject 模块不变量），整窗兑底是唯一正确口径。
pub(crate) async fn trend_entity(
    db: &sea_orm::DatabaseConnection,
    query: &ChartsQuery,
) -> Option<(&'static str, Option<String>)> {
    match (
        query.provider_id,
        query.virtual_model_id,
        query.model_id.as_deref(),
        query.api_key.as_deref(),
    ) {
        (None, None, None, None) => Some((snap::ENTITY_WHOLE, None)),
        (Some(p), None, None, None) => Some((snap::ENTITY_PROVIDER, Some(p.to_string()))),
        (Some(p), None, Some(m), None) => {
            let key = snap::resolve_pm_key(db, p, m).await?;
            Some((snap::ENTITY_MODEL, Some(key)))
        }
        (None, Some(vm), None, None) => Some((snap::ENTITY_VIRTUAL_MODEL, Some(vm.to_string()))),
        (None, None, None, Some(key)) => {
            let id = snap::resolve_api_key_id(db, key).await?;
            Some((snap::ENTITY_API_KEY, Some(id)))
        }
        _ => None,
    }
}

/// 分布主体解析：过滤形态 → 分布行模式（与生成端一致）。
/// 返回 (entity_type, 精确 entity 值, provider 过滤, vm 过滤, api_key 过滤)。
/// 精确形态（pm/Key）键解析失败（已删）→ None：整窗兑底（subject 不变量）。
async fn distribution_pattern(
    db: &sea_orm::DatabaseConnection,
    query: &ChartsQuery,
) -> Option<(
    &'static str,
    Option<String>,
    Option<i32>,
    Option<i32>,
    Option<i32>,
)> {
    match (
        query.provider_id,
        query.virtual_model_id,
        query.model_id.as_deref(),
        query.api_key.as_deref(),
    ) {
        (None, None, None, None) => Some((snap::ENTITY_MODEL, None, None, None, None)),
        (Some(p), None, None, None) => Some((snap::ENTITY_MODEL, None, Some(p), None, None)),
        (Some(p), None, Some(m), None) => {
            let key = snap::resolve_pm_key(db, p, m).await?;
            Some((snap::ENTITY_MODEL, Some(key), None, None, None))
        }
        (None, Some(vm), None, None) => Some((snap::ENTITY_VM_MEMBER, None, None, Some(vm), None)),
        (None, None, None, Some(key)) => {
            let id = snap::resolve_api_key_id(db, key)
                .await?
                .parse::<i32>()
                .ok()?;
            Some((snap::ENTITY_API_KEY_MODEL, None, None, None, Some(id)))
        }
        _ => None,
    }
}

/// 图表数据：调用/ token 的趋势 + 按上游模型的分布。
/// 支持可选 startTime/endTime（缺省回退过去 24 小时）与各维度过滤；
/// 显式 hour/day/month/year 桶且过滤形态可落快照时：闭桶读快照 + 兑底合并；
/// 其余（30 天块、不支持形态、缺快照）整窗兑底，数字与实时口径一致。
pub(crate) async fn charts(
    State(state): State<AppState>,
    Query(query): Query<ChartsQuery>,
) -> impl IntoResponse {
    let explicit_granularity = match Granularity::parse(query.granularity.as_deref()) {
        Ok(g) => g,
        Err(msg) => return response::bad_request(msg),
    };
    let tz_offset_minutes = stats_tz_offset_minutes(query.start_time);
    let window = resolve_chart_window(
        query.start_time,
        query.end_time,
        explicit_granularity,
        tz_offset_minutes,
    );
    let db = &state.db;
    let now = chrono::Utc::now().timestamp_millis();
    let month_mode = matches!(window.granularity, Granularity::Month | Granularity::Year);

    // 快照粒度：显式 hour/day/month/year 且与桶对齐的查询才走快照。
    let snap_level = match (window.granularity, window.bucket_ms) {
        (Granularity::Hour, b) if b == HOUR_MS => Some(snap::Level::Hour),
        (Granularity::Day, b) if b == DAY_MS => Some(snap::Level::Day),
        (Granularity::Month, _) => Some(snap::Level::Month),
        (Granularity::Year, _) => Some(snap::Level::Year),
        _ => None,
    };

    let supported = trend_entity(db, &query).await.is_some()
        && distribution_pattern(db, &query).await.is_some();
    let coverage = match (snap_level, supported) {
        (Some(level), true) => {
            match snap::coverage(
                db,
                level,
                tz_offset_minutes,
                window.start,
                window.end,
                now,
                snap::MARGIN_MS,
            )
            .await
            {
                Ok(cov) => match snap::trim_zero_prefix(db, cov).await {
                    Ok(cov) => cov,
                    Err(e) => return response::db_error(e.to_string()),
                },
                Err(e) => return response::db_error(e.to_string()),
            }
        }
        _ => snap::Coverage {
            snapshots: vec![],
            live: vec![(window.start, window.end)],
        },
    };

    let result = charts_merge(db, &query, &window, tz_offset_minutes, month_mode, coverage).await;
    match result {
        Ok(charts) => (StatusCode::OK, Json(Response::success(charts))),
        Err(e) => response::db_error(e),
    }
}

/// 图表合并主流程：闭桶贡献（快照行按桶记账）+ 兑底贡献（每段分桶 SQL）
/// 加总后按粒度补零输出，并计算按模型分布。
#[allow(clippy::too_many_arguments)]
async fn charts_merge(
    db: &sea_orm::DatabaseConnection,
    query: &ChartsQuery,
    window: &ChartWindow,
    tz_offset_minutes: i32,
    month_mode: bool,
    coverage: snap::Coverage,
) -> Result<ChartsResponse, String> {
    let off_ms = i64::from(tz_offset_minutes) * 60_000;
    let bucket_ms = window.bucket_ms;
    let (filter_sql, filter_params) = filter_parts(query);

    // 趋势桶记账：hour/day → 桶索引；month/year → (本地年, 月|0)。
    let cal_level = if matches!(window.granularity, Granularity::Year) {
        snap::Level::Year
    } else {
        snap::Level::Month
    };
    let (mut call_idx, mut token_idx) = (
        std::collections::BTreeMap::<i64, f64>::new(),
        std::collections::BTreeMap::<i64, f64>::new(),
    );
    let (mut call_period, mut token_period) = (
        std::collections::BTreeMap::<(i32, u32), f64>::new(),
        std::collections::BTreeMap::<(i32, u32), f64>::new(),
    );
    let key_of_ts = |ts: i64| snap::period_key_of_ts(ts, off_ms, cal_level);

    // 快照贡献：帧起点归属到查询桶。
    let mut by_level: std::collections::BTreeMap<snap::Level, Vec<snap::Frame>> =
        std::collections::BTreeMap::new();
    for frame in &coverage.snapshots {
        by_level.entry(frame.level).or_default().push(*frame);
    }
    let (entity_type, exact_entity) = match trend_entity(db, query).await {
        Some(v) => v,
        None => (snap::ENTITY_WHOLE, None),
    };
    let add_period =
        |map: &mut std::collections::BTreeMap<(i32, u32), f64>, key: (i32, u32), v: f64| {
            *map.entry(key).or_insert(0.0) += v;
        };
    let add_idx = |map: &mut std::collections::BTreeMap<i64, f64>, idx: i64, v: f64| {
        *map.entry(idx).or_insert(0.0) += v;
    };
    for (level, frames) in &by_level {
        let rows = snap::snapshot_rows(
            db,
            *level,
            frames,
            entity_type,
            exact_entity.as_deref(),
            None,
            &CHARTS_SNAP_METRICS,
        )
        .await
        .map_err(|e| e.to_string())?;
        for (start, _, _, metric, value) in rows {
            if month_mode {
                if let Some(key) = key_of_ts(start) {
                    if metric == "calls" {
                        add_period(&mut call_period, key, value);
                    } else {
                        add_period(&mut token_period, key, value);
                    }
                }
            } else {
                let idx = (start + off_ms).div_euclid(bucket_ms);
                if metric == "calls" {
                    add_idx(&mut call_idx, idx, value);
                } else {
                    add_idx(&mut token_idx, idx, value);
                }
            }
        }
    }

    // 兑底贡献：每段一条分桶 SQL（与旧实现同一 bucket 表达式/口径）。
    let bucket_expr = window.bucket_expr();
    let trend_sql = |value_expr: &str, seg: (i64, i64)| {
        format!(
            "SELECT {bucket_expr} AS bucket, {value_expr} AS value FROM request r \
             WHERE r.start_time >= {} AND r.start_time < {}{filter_sql} GROUP BY bucket",
            seg.0, seg.1
        )
    };
    let run_live = async |value_expr: &str,
                          idx_map: &mut std::collections::BTreeMap<i64, f64>,
                          period_map: &mut std::collections::BTreeMap<(i32, u32), f64>|
           -> Result<(), String> {
        for &(s, e) in &coverage.live {
            let sql = trend_sql(value_expr, (s, e));
            // 段边界已是字面量，只绑定过滤占位符（顺序与 filter_sql 一致）。
            let rows = db
                .query_all_raw(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    sql,
                    filter_params.clone(),
                ))
                .await
                .map_err(|e| e.to_string())?;
            for row in rows {
                let bucket: i64 = row.try_get("", "bucket").unwrap_or(0);
                let value: f64 = row
                    .try_get("", "value")
                    .ok()
                    .or_else(|| row.try_get::<i64>("", "value").ok().map(|v| v as f64))
                    .unwrap_or(0.0);
                if month_mode {
                    // bucket = 本地日索引（bucket_expr 月/年路径），日索引折历法周期键。
                    if let Some(key) = snap::period_key_of_day_index(bucket, cal_level) {
                        *period_map.entry(key).or_insert(0.0) += value;
                    }
                } else {
                    *idx_map.entry(bucket).or_insert(0.0) += value;
                }
            }
        }
        Ok(())
    };
    run_live(
        snap::expr_of(snap::metrics::CALLS),
        &mut call_idx,
        &mut call_period,
    )
    .await?;
    run_live(
        snap::expr_of(snap::metrics::TOKENS_ALL),
        &mut token_idx,
        &mut token_period,
    )
    .await?;

    // 趋势按粒度补零输出（与旧实现同形状：bucket_range 全补 / 自然月年全补）。
    let (call_trend, token_trend) = if month_mode {
        let fill_periods = |map: &std::collections::BTreeMap<(i32, u32), f64>| -> Vec<TrendPoint> {
            snap::natural_periods(cal_level, off_ms, window.start, window.end)
                .into_iter()
                .map(|(y, m, start)| TrendPoint {
                    bucket_start: start,
                    value: map
                        .get(&(y, if cal_level == snap::Level::Year { 0 } else { m }))
                        .copied()
                        .unwrap_or(0.0)
                        .round() as i64,
                })
                .collect()
        };
        (fill_periods(&call_period), fill_periods(&token_period))
    } else {
        let buckets = window.bucket_range();
        let fill_idx = |map: &std::collections::BTreeMap<i64, f64>| -> Vec<TrendPoint> {
            buckets
                .clone()
                .map(|bucket| TrendPoint {
                    bucket_start: window.bucket_start_ms(bucket),
                    value: map.get(&bucket).copied().unwrap_or(0.0).round() as i64,
                })
                .collect()
        };
        (fill_idx(&call_idx), fill_idx(&token_idx))
    };

    let (call_by_model, token_by_model) =
        model_distribution(db, query, &coverage, tz_offset_minutes).await?;

    Ok(ChartsResponse {
        call_trend,
        call_by_model,
        token_trend,
        token_by_model,
    })
}

/// 分布：快照闭桶段按主体行（model / vm_member / api_key_model）加总 +
/// 兑底段按 (p.name, r.model_id) 分组加总；显示键统一为 (供应商名, 模型 ID)。
async fn model_distribution(
    db: &sea_orm::DatabaseConnection,
    query: &ChartsQuery,
    coverage: &snap::Coverage,
    _tz_offset_minutes: i32,
) -> Result<(Vec<ModelValue>, Vec<ModelValue>), String> {
    let (entity_type, exact_entity, provider_filter, vm_filter, key_filter) =
        match distribution_pattern(db, query).await {
            Some(v) => v,
            None => (snap::ENTITY_MODEL, None, None, None, None),
        };
    let mut by_entity: std::collections::BTreeMap<String, (f64, f64)> =
        std::collections::BTreeMap::new();
    let mut by_level: std::collections::BTreeMap<snap::Level, Vec<snap::Frame>> =
        std::collections::BTreeMap::new();
    for frame in &coverage.snapshots {
        by_level.entry(frame.level).or_default().push(*frame);
    }
    for (level, frames) in &by_level {
        let rows = snap::snapshot_rows(
            db,
            *level,
            frames,
            entity_type,
            exact_entity.as_deref(),
            None,
            &CHARTS_SNAP_METRICS,
        )
        .await
        .map_err(|e| e.to_string())?;
        for (_, _, entity, metric, value) in rows {
            let entry = by_entity.entry(entity).or_insert((0.0, 0.0));
            if metric == "calls" {
                entry.0 += value;
            } else {
                entry.1 += value;
            }
        }
    }

    // 主体文本 → 展示 (供应商名, 模型 ID)；复合主体取逗号后段（pm id）。
    let pm_key_of = |entity: &str| -> Option<String> {
        if entity_type == snap::ENTITY_MODEL {
            Some(entity.to_string())
        } else {
            entity.rsplit_once(',').map(|(_, pm)| pm.to_string())
        }
    };
    let pm_keys: Vec<String> = by_entity.keys().filter_map(|e| pm_key_of(e)).collect();
    let mut display = std::collections::HashMap::<String, (String, String)>::new();
    if !pm_keys.is_empty() {
        let in_list = pm_keys
            .iter()
            .map(|k| format!("'{k}'"))
            .collect::<Vec<_>>()
            .join(", ");
        let mut sql = format!(
            "SELECT CAST(pm.model_id AS TEXT) AS k, COALESCE(p.name, '') AS n, \
                    pm.provider_model_id AS mid \
             FROM provider_model pm LEFT JOIN provider p ON p.id = pm.provider_id \
             WHERE CAST(pm.model_id AS TEXT) IN ({in_list})"
        );
        if let Some(p) = provider_filter {
            sql.push_str(&format!(" AND pm.provider_id = {p}"));
        }
        let rows = db
            .query_all_raw(Statement::from_string(DbBackend::Sqlite, sql))
            .await
            .map_err(|e| e.to_string())?;
        for row in rows {
            let k: String = row.try_get("", "k").unwrap_or_default();
            let n: String = row.try_get("", "n").unwrap_or_default();
            let mid: String = row.try_get("", "mid").unwrap_or_default();
            display.insert(k, (n, mid));
        }
    }

    let mut result: std::collections::BTreeMap<(String, String), (f64, f64)> =
        std::collections::BTreeMap::new();
    for (entity, (calls, tokens)) in by_entity {
        if let Some(vm) = vm_filter {
            let ok = entity
                .split_once(',')
                .map(|(v, _)| v == vm.to_string())
                .unwrap_or(false);
            if !ok {
                continue;
            }
        }
        if let Some(key_id) = key_filter {
            let ok = entity
                .split_once(',')
                .map(|(k, _)| k == key_id.to_string())
                .unwrap_or(false);
            if !ok {
                continue;
            }
        }
        let Some(pm_key) = pm_key_of(&entity) else {
            continue;
        };
        let Some((name, model_id)) = display.get(&pm_key) else {
            continue; // pm 已删：与生成端「映射不到不产行」同语义
        };
        let entry = result
            .entry((name.clone(), model_id.clone()))
            .or_insert((0.0, 0.0));
        entry.0 += calls;
        entry.1 += tokens;
    }

    // 兑底贡献：与旧 SQL 同分组（p.name, r.model_id）。
    let (filter_sql, filter_params) = filter_parts(query);
    let model_sql = |value_expr: &str, seg: (i64, i64)| {
        format!(
            "SELECT COALESCE(p.name, '') AS provider_name, r.model_id, {value_expr} AS value \
             FROM request r LEFT JOIN provider p ON p.id = r.provider_id \
             WHERE r.start_time >= {} AND r.start_time < {}{filter_sql} \
             GROUP BY p.name, r.model_id",
            seg.0, seg.1
        )
    };
    let mut live_calls: std::collections::BTreeMap<(String, String), f64> = Default::default();
    let mut live_tokens: std::collections::BTreeMap<(String, String), f64> = Default::default();
    let run_live = async |value_expr: &str,
                          map: &mut std::collections::BTreeMap<(String, String), f64>|
           -> Result<(), String> {
        for &(s, e) in &coverage.live {
            let sql = model_sql(value_expr, (s, e));
            let rows = db
                .query_all_raw(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    sql,
                    filter_params.clone(),
                ))
                .await
                .map_err(|e| e.to_string())?;
            for row in rows {
                let name: String = row.try_get("", "provider_name").unwrap_or_default();
                let model: String = row.try_get("", "model_id").unwrap_or_default();
                let value: f64 = row
                    .try_get("", "value")
                    .ok()
                    .or_else(|| row.try_get::<i64>("", "value").ok().map(|v| v as f64))
                    .unwrap_or(0.0);
                *map.entry((name, model)).or_insert(0.0) += value;
            }
        }
        Ok(())
    };
    run_live(snap::expr_of(snap::metrics::CALLS), &mut live_calls).await?;
    run_live(snap::expr_of(snap::metrics::TOKENS_ALL), &mut live_tokens).await?;

    let mut call_by_model: Vec<ModelValue> = Vec::new();
    let mut token_by_model: Vec<ModelValue> = Vec::new();
    let mut keys: Vec<(String, String)> = result
        .keys()
        .cloned()
        .chain(live_calls.keys().cloned())
        .chain(live_tokens.keys().cloned())
        .collect();
    keys.sort();
    keys.dedup();
    for (name, model) in keys {
        let (c, t) = result
            .get(&(name.clone(), model.clone()))
            .copied()
            .unwrap_or((0.0, 0.0));
        let lc = live_calls
            .get(&(name.clone(), model.clone()))
            .copied()
            .unwrap_or(0.0);
        let lt = live_tokens
            .get(&(name.clone(), model.clone()))
            .copied()
            .unwrap_or(0.0);
        let call_total = (c + lc).round() as i64;
        let token_total = (t + lt).round() as i64;
        if call_total == 0 && token_total == 0 {
            continue;
        }
        call_by_model.push(ModelValue {
            provider_name: name.clone(),
            model_id: model.clone(),
            value: call_total,
        });
        token_by_model.push(ModelValue {
            provider_name: name,
            model_id: model,
            value: token_total,
        });
    }
    Ok((call_by_model, token_by_model))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::ConnectionTrait;

    const HOUR_MS: i64 = 3_600_000;

    async fn test_db() -> sea_orm::DatabaseConnection {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        let url = format!("sqlite:///{}?mode=rwc", path.display());
        let db = crate::db::connect(&url).await.unwrap();
        let ts = "2024-01-01T00:00:00Z";
        db.execute_unprepared(&format!(
            "INSERT INTO provider (name, enable, base_url, api_key, custom_header, protocol_type, billing_mode, extra, sort_order, proxy_enabled, proxy_addr, created_at, updated_at) \
             VALUES ('测试供应商', 1, 'https://a.example', 'k', '{{}}', 0, 1, '{{}}', 0, 0, '', '{ts}', '{ts}')"
        )).await.unwrap();
        db
    }

    async fn insert_req(db: &sea_orm::DatabaseConnection, rid: &str, vm: i32, start: i64) {
        db.execute_unprepared(&format!(
            "INSERT INTO request (request_id, virtual_model_id, provider_id, model_id, stream, \
             input_tokens, input_cache_tokens, input_cache_rate, output_tokens, output_tokens_time, \
             tps, start_time, end_time, request_time, success, fail_reason, total_tokens, api_key_name) \
             VALUES ('{rid}', {vm}, 1, 'gpt-4o', 0, 10, 0, 0.0, NULL, NULL, 0.0, {start}, {start}, 500, 1, NULL, 100, 'itest-key')"
        )).await.unwrap();
    }

    #[tokio::test]
    async fn charts_live_by_model_with_vm_filter() {
        let db = test_db().await;
        let t0 = (1_700_000_000_000i64 / HOUR_MS) * HOUR_MS;
        insert_req(&db, "vmf1", 1, t0 + 1).await;
        insert_req(&db, "vmf2", 2, t0 + 1).await;
        let query = ChartsQuery {
            start_time: Some(t0),
            end_time: Some(t0 + 2 * HOUR_MS),
            provider_id: None,
            virtual_model_id: Some(1),
            model_id: None,
            api_key: None,
            granularity: Some("hour".to_string()),
        };
        let coverage = snap::Coverage {
            snapshots: vec![],
            live: vec![(t0, t0 + 2 * HOUR_MS)],
        };
        let (call_by_model, _) = model_distribution(&db, &query, &coverage, 480)
            .await
            .unwrap();
        assert_eq!(call_by_model.len(), 1);
        assert_eq!(call_by_model[0].model_id, "gpt-4o");
    }
}

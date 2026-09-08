use super::*;

/// 每桶延迟分位点（毫秒；该桶无样本时字段为 0）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PercentilePoint {
    bucket_start: i64,
    p50: f64,
    p90: f64,
    p95: f64,
    p99: f64,
}

/// 失败原因分布条目（空/缺失原因归「无原因」，由前端文案呈现）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FailureReasonItem {
    reason: String,
    count: i64,
}

/// 按 API Key 聚合的调用量条目。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiKeyRankItem {
    api_key_name: String,
    value: i64,
}

/// 性能与可靠性分析（insight）：一次返回失败诊断 / 延迟分位 / Token 结构 / 吞吐四组数据。
///
/// 口径：失败相关基于全量请求（成功+失败都计数）；延迟与 Token 相关基于成功请求
/// （`success = 1`，与赛马/指标一致）；全部由 request 表现有字段聚合，无 schema 变更。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InsightResponse {
    // 失败诊断
    /// 每桶全部调用数（成功+失败；成功/失败堆叠面积图基准）。
    call_trend: Vec<TrendPoint>,
    failure_trend: Vec<TrendPoint>,
    failure_rate_trend: Vec<FloatTrendPoint>,
    failure_reasons: Vec<FailureReasonItem>,
    // 延迟分位
    ttft_percentiles: Vec<PercentilePoint>,
    latency_percentiles: Vec<PercentilePoint>,
    // Token 结构
    input_token_trend: Vec<TrendPoint>,
    output_token_trend: Vec<TrendPoint>,
    cache_hit_rate_trend: Vec<FloatTrendPoint>,
    output_tokens_per_sec_trend: Vec<FloatTrendPoint>,
    // 吞吐 / 调用入口
    api_key_rank: Vec<ApiKeyRankItem>,
    rpm_trend: Vec<TrendPoint>,
    tpm_trend: Vec<FloatTrendPoint>,
    stream_ratio_trend: Vec<FloatTrendPoint>,
}

/// 性能与可靠性分析：失败诊断 / 延迟分位 / Token 结构 / 吞吐 四组聚合。
///
/// 与 charts 共用同一套窗口/粒度/时区解析与过滤参数（providerId/virtualModelId/modelId）。
/// 月/年桶：失败率、缓存命中率、流式占比等比值在归并后的桶上重算；
/// 延迟分位在月/年桶上退化（分位需要逐值，跨月合并语义含糊），返回空数组。
pub(crate) async fn insight(
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

    // WHERE 公共条件：时间窗口（半开）+ 可选过滤（与 charts 同口径）。
    let mut where_sql = String::from("r.start_time >= ? AND r.start_time < ?");
    let mut params: Vec<sea_orm::Value> = vec![window.start.into(), window.end.into()];
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

    let db = &state.db;
    let month_mode = matches!(window.granularity, Granularity::Month | Granularity::Year);
    let bucket_expr = window.bucket_expr();

    // 每桶分组聚合（全量或成功请求），返回 (bucket, value) 对；月/年桶先按本地日聚合。
    let group_rows = |value_expr: &str, success_only: bool| {
        let success_cond = if success_only {
            " AND r.success = 1"
        } else {
            ""
        };
        format!(
            "SELECT {bucket_expr} AS bucket, {value_expr} AS value \
             FROM request r WHERE {where_sql}{success_cond} GROUP BY bucket"
        )
    };

    // 全量：每桶调用数 / 失败数 / 流式数 / total_tokens（吞吐用）。
    let (call_rows, fail_rows, stream_rows, tpm_rows, api_key_rows) = match tokio::try_join!(
        db.query_all_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            group_rows("COUNT(*)", false),
            params.clone(),
        )),
        db.query_all_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            group_rows("COALESCE(SUM(CASE WHEN r.success = 0 THEN 1 ELSE 0 END), 0)", false),
            params.clone(),
        )),
        db.query_all_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            group_rows("COALESCE(SUM(CASE WHEN r.stream THEN 1 ELSE 0 END), 0)", false),
            params.clone(),
        )),
        db.query_all_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            group_rows("COALESCE(SUM(r.total_tokens), 0)", false),
            params.clone(),
        )),
        db.query_all_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT r.api_key_name, COUNT(*) AS value FROM request r WHERE {where_sql} GROUP BY r.api_key_name"
            ),
            params.clone(),
        )),
    ) {
        Ok(rows) => rows,
        Err(e) => return response::db_error(e.to_string()),
    };

    // 成功请求：输入 / 输出 token、缓存 token、输出耗时（秒）。
    let (input_rows, output_rows, cache_rows, out_time_rows) = match tokio::try_join!(
        db.query_all_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            group_rows("COALESCE(SUM(r.input_tokens), 0)", true),
            params.clone(),
        )),
        db.query_all_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            group_rows("COALESCE(SUM(r.output_tokens), 0)", true),
            params.clone(),
        )),
        db.query_all_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            group_rows("COALESCE(SUM(r.input_cache_tokens), 0)", true),
            params.clone(),
        )),
        db.query_all_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            group_rows(
                "COALESCE(SUM(CASE WHEN r.output_tokens_time > 0 THEN r.output_tokens / (r.output_tokens_time / 1000.0) ELSE 0 END), 0)",
                true,
            ),
            params.clone(),
        )),
    ) {
        Ok(rows) => rows,
        Err(e) => return response::db_error(e.to_string()),
    };

    // 失败原因分布：全窗口按 fail_reason 分组（仅失败请求；NULL/空串归空串，
    // 前端显示「无原因」）。
    let mut reason_map: std::collections::BTreeMap<String, i64> = std::collections::BTreeMap::new();
    if let Ok(rows) = db
        .query_all_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT COALESCE(NULLIF(r.fail_reason, ''), '') AS reason, COUNT(*) AS count \
                 FROM request r WHERE {where_sql} AND r.success = 0 GROUP BY reason"
            ),
            params.clone(),
        ))
        .await
    {
        for row in &rows {
            let reason: String = row.try_get("", "reason").unwrap_or_default();
            let count: i64 = row.try_get("", "count").unwrap_or(0);
            *reason_map.entry(reason).or_insert(0) += count;
        }
    } else {
        return response::db_error("失败原因统计查询失败");
    }

    // 延迟分位：每桶逐值拉回（仅成功请求；ttft 只计流式有首 token 的行）。
    // month_mode（月/年粒度）下分位恒为空数组，短路前置：不发起这两条
    // 全窗逐值扫描（此前整窗扫完才在 group_percentiles 丢弃，S5）。
    let (ttft_rows, latency_rows) = if month_mode {
        (Vec::new(), Vec::new())
    } else {
        match tokio::try_join!(
            db.query_all_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "SELECT {bucket_expr} AS bucket, r.ttft AS value FROM request r \
                     WHERE {where_sql} AND r.success = 1 AND r.ttft IS NOT NULL"
                ),
                params.clone(),
            )),
            db.query_all_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "SELECT {bucket_expr} AS bucket, r.request_time AS value FROM request r \
                     WHERE {where_sql} AND r.success = 1"
                ),
                params.clone(),
            )),
        ) {
            Ok(rows) => rows,
            Err(e) => return response::db_error(e.to_string()),
        }
    };

    // ---- 归并/填充 ----

    // 整数值趋势填充：小时/天按桶索引补零；月/年按本地日归并自然月/年。
    let fill_int_series = |map: &std::collections::HashMap<i64, f64>| -> Vec<TrendPoint> {
        if month_mode {
            let day_indexes = map
                .iter()
                .map(|(&bucket, &value)| (bucket, value.round() as i64))
                .collect::<Vec<_>>();
            let (starts, values, _) = merge_natural_periods(
                &day_indexes,
                &[],
                window.start,
                window.end,
                tz,
                matches!(window.granularity, Granularity::Month),
            );
            starts
                .into_iter()
                .zip(values)
                .map(|(bucket_start, value)| TrendPoint {
                    bucket_start,
                    value,
                })
                .collect()
        } else {
            window
                .bucket_range()
                .map(|bucket| TrendPoint {
                    bucket_start: window.bucket_start_ms(bucket),
                    value: map.get(&bucket).copied().unwrap_or(0.0).round() as i64,
                })
                .collect()
        }
    };

    // 浮点值趋势填充：语义同 fill_int_series，但保留小数（比率/速率类指标）。
    let fill_float_series = |map: &std::collections::HashMap<i64, f64>| -> Vec<FloatTrendPoint> {
        if month_mode {
            let day_indexes = map
                .iter()
                .map(|(&bucket, &value)| (bucket, value.round() as i64))
                .collect::<Vec<_>>();
            let (starts, values, _) = merge_natural_periods(
                &day_indexes,
                &[],
                window.start,
                window.end,
                tz,
                matches!(window.granularity, Granularity::Month),
            );
            starts
                .into_iter()
                .zip(values)
                .map(|(bucket_start, value)| FloatTrendPoint {
                    bucket_start,
                    value: value as f64,
                })
                .collect()
        } else {
            window
                .bucket_range()
                .map(|bucket| FloatTrendPoint {
                    bucket_start: window.bucket_start_ms(bucket),
                    value: map.get(&bucket).copied().unwrap_or(0.0),
                })
                .collect()
        }
    };

    // 按桶起点（bucket_start）索引 map，供月/年归并后按桶起点重算比率。
    // 小时/天：桶索引反推桶起点；月/年：SQL 桶是本地日索引，需归并到自然月/年起点
    //（与 merge_natural_periods 的 to_bucket_start 一致），否则与 failure_trend 的
    // 桶起点（每月/年 1 日 0 点）对不上，比率会全部错配为 0。
    let index_by_start = |map: &std::collections::HashMap<i64, f64>| {
        let mut by_start: std::collections::HashMap<i64, f64> = std::collections::HashMap::new();
        for (&bucket, &value) in map {
            let start = if month_mode {
                let day_ms = bucket * DAY_MS - i64::from(tz_offset_minutes) * 60_000;
                let date = chrono::DateTime::from_timestamp_millis(day_ms)
                    .map(|t| t.with_timezone(&tz))
                    .and_then(|local| {
                        let (y, m) = if matches!(window.granularity, Granularity::Month) {
                            (local.year(), local.month())
                        } else {
                            (local.year(), 0)
                        };
                        chrono::NaiveDate::from_ymd_opt(y, if m > 0 { m } else { 1 }, 1)
                            .and_then(|d| d.and_hms_opt(0, 0, 0))
                            .and_then(|dt| dt.and_local_timezone(tz).single())
                            .map(|dt| dt.timestamp_millis())
                    });
                match date {
                    Some(ms) => ms,
                    None => continue,
                }
            } else {
                window.bucket_start_ms(bucket)
            };
            *by_start.entry(start).or_insert(0.0) += value;
        }
        by_start
    };

    let to_map = |rows: &[sea_orm::QueryResult]| -> std::collections::HashMap<i64, f64> {
        let mut map = std::collections::HashMap::new();
        for row in rows {
            let bucket: i64 = row.try_get("", "bucket").unwrap_or(0);
            // SQLite 的 SUM(INTEGER) 返回 INTEGER、SUM(REAL)/除法为 REAL：先试 f64 再试 i64。
            let value: f64 = row
                .try_get("", "value")
                .ok()
                .or_else(|| row.try_get::<i64>("", "value").ok().map(|v| v as f64))
                .unwrap_or(0.0);
            // SQL GROUP BY bucket 保证桶唯一，直接覆盖即可。
            map.insert(bucket, value);
        }
        map
    };

    let calls_map = to_map(&call_rows);
    let fails_map = to_map(&fail_rows);
    let streams_map = to_map(&stream_rows);
    let tpm_map = to_map(&tpm_rows);
    let inputs_map = to_map(&input_rows);
    let outputs_map = to_map(&output_rows);
    let caches_map = to_map(&cache_rows);
    let out_time_rates_map = to_map(&out_time_rows);

    let failure_trend = fill_int_series(&fails_map);
    let call_trend = fill_int_series(&calls_map);
    let input_token_trend = fill_int_series(&inputs_map);
    let output_token_trend = fill_int_series(&outputs_map);
    let output_per_sec_trend = fill_float_series(&out_time_rates_map);

    // 比率重算：月/年桶按归并后桶起点，小时/天按桶索引。
    let calls_by_start = index_by_start(&calls_map);
    let fails_by_start = index_by_start(&fails_map);
    let streams_by_start = index_by_start(&streams_map);
    let inputs_by_start = index_by_start(&inputs_map);
    let caches_by_start = index_by_start(&caches_map);
    // 比率按归并后的桶起点重算：以 failure_trend 的桶起点为对齐锚（所有 int/float
    // 序列都用同一组零填充桶，保证三组比率对齐同一时间轴；若未来某序列改条件填充，
    // 需同步改这里的锚定）。
    let ratio_by_start = |total_by_start: &std::collections::HashMap<i64, f64>,
                          part_by_start: &std::collections::HashMap<i64, f64>|
     -> Vec<FloatTrendPoint> {
        let aligned_starts: Vec<i64> = failure_trend.iter().map(|p| p.bucket_start).collect();
        aligned_starts
            .into_iter()
            .map(|start| {
                let total = total_by_start.get(&start).copied().unwrap_or(0.0);
                let part = part_by_start.get(&start).copied().unwrap_or(0.0);
                FloatTrendPoint {
                    bucket_start: start,
                    value: if total > 0.0 { part / total } else { 0.0 },
                }
            })
            .collect()
    };
    let failure_rate_trend = ratio_by_start(&calls_by_start, &fails_by_start);
    let stream_ratio_final = ratio_by_start(&calls_by_start, &streams_by_start);
    // 缓存命中率趋势单独保留 5 位小数，与 summary / SQL 侧 cache_hit_rate_sql
    // 口径一致（failure_rate / stream_ratio 保持原精度）。
    let cache_rate_final = ratio_by_start(&inputs_by_start, &caches_by_start)
        .into_iter()
        .map(|point| FloatTrendPoint {
            bucket_start: point.bucket_start,
            value: round_5(point.value),
        })
        .collect::<Vec<_>>();

    // RPM/TPM：仅小时桶有意义。RPM=每小时请求数（÷窗口小时数）；TPM=每小时 token 总量 ÷60 得每分钟。
    let (rpm_trend, tpm_trend) = if matches!(window.granularity, Granularity::Hour) {
        let hours = ((window.end - window.start) as f64 / HOUR_MS as f64).max(1.0);
        (
            fill_int_series(&calls_map)
                .into_iter()
                .map(|point| TrendPoint {
                    bucket_start: point.bucket_start,
                    value: ((point.value as f64 / hours).round()) as i64,
                })
                .collect::<Vec<_>>(),
            // 桶内 token 总量 ÷ 60 分钟 = 每分钟 token（保留小数）。
            fill_int_series(&tpm_map)
                .into_iter()
                .map(|point| FloatTrendPoint {
                    bucket_start: point.bucket_start,
                    value: point.value as f64 / 60.0,
                })
                .collect::<Vec<_>>(),
        )
    } else {
        (Vec::new(), Vec::new())
    };

    // API Key 排行：按调用数降序（Top N + 其他由前端处理）。
    let mut api_key_rank = api_key_rows
        .iter()
        .map(|row| ApiKeyRankItem {
            api_key_name: row.try_get("", "api_key_name").unwrap_or_default(),
            value: row.try_get::<i64>("", "value").unwrap_or(0),
        })
        .collect::<Vec<_>>();
    api_key_rank.sort_by_key(|item| std::cmp::Reverse(item.value));

    // 延迟分位：小时/天桶逐值分组算分位；月/年返回空数组。
    let group_percentiles = |rows: &[sea_orm::QueryResult]| -> Vec<PercentilePoint> {
        if month_mode {
            return Vec::new();
        }
        let mut buckets: std::collections::BTreeMap<i64, Vec<f64>> =
            std::collections::BTreeMap::new();
        for row in rows {
            let bucket: i64 = row.try_get("", "bucket").unwrap_or(0);
            // ttft/request_time 是 INTEGER 列：先试 f64 再试 i64。
            let value: f64 = row
                .try_get("", "value")
                .ok()
                .or_else(|| row.try_get::<i64>("", "value").ok().map(|v| v as f64))
                .unwrap_or(0.0);
            buckets.entry(bucket).or_default().push(value);
        }
        // 按桶补零对齐（无样本桶输出 0）。
        window
            .bucket_range()
            .map(|bucket| {
                let mut values = buckets.get(&bucket).cloned().unwrap_or_default();
                values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                PercentilePoint {
                    bucket_start: window.bucket_start_ms(bucket),
                    p50: percentile(&values, 0.5),
                    p90: percentile(&values, 0.9),
                    p95: percentile(&values, 0.95),
                    p99: percentile(&values, 0.99),
                }
            })
            .collect()
    };
    let ttft_percentiles = group_percentiles(&ttft_rows);
    let latency_percentiles = group_percentiles(&latency_rows);

    (
        StatusCode::OK,
        Json(Response::success(InsightResponse {
            call_trend,
            failure_trend,
            failure_rate_trend,
            failure_reasons: reason_map
                .into_iter()
                .map(|(reason, count)| FailureReasonItem { reason, count })
                .collect(),
            ttft_percentiles,
            latency_percentiles,
            input_token_trend,
            output_token_trend,
            cache_hit_rate_trend: cache_rate_final,
            output_tokens_per_sec_trend: output_per_sec_trend,
            api_key_rank,
            rpm_trend,
            tpm_trend,
            stream_ratio_trend: stream_ratio_final,
        })),
    )
}

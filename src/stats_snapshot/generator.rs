//! 快照生成器：固化闭桶（ADR-0021、spec §生成器）。
//! 单遍扫描桶内 request 行，按 7 个主体模式聚合可加和指标；hour/day 桶另取
//! 成功行原始值算分位标量。全部行在同一事务内幂等 upsert，提交 = 桶固化完成。

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};

use super::registry::metrics;
use super::{Frame, percentile};

/// 主体模式：key_expr 产出该模式的 entity 文本；映射不到（JOIN 为 NULL）的请求
/// 归入 NULL 键组，写行时跳过（不产该主体行）。whole 恒产出 '' 行（空桶哨兵）。
/// 聚合与分位取值共用同一 key 语义，保证两路产出的 entity 文本一致。
struct Pattern {
    entity_type: &'static str,
    key_expr: &'static str,
    joins: &'static str,
}

const PATTERNS: [Pattern; 7] = [
    Pattern {
        entity_type: super::ENTITY_WHOLE,
        key_expr: "''",
        joins: "",
    },
    Pattern {
        entity_type: super::ENTITY_PROVIDER,
        key_expr: "CAST(r.provider_id AS TEXT)",
        joins: "",
    },
    Pattern {
        entity_type: super::ENTITY_MODEL,
        key_expr: "CAST(pm.model_id AS TEXT)",
        joins: "LEFT JOIN provider_model pm ON pm.provider_id = r.provider_id AND pm.provider_model_id = r.model_id",
    },
    Pattern {
        entity_type: super::ENTITY_VIRTUAL_MODEL,
        key_expr: "CAST(r.virtual_model_id AS TEXT)",
        joins: "",
    },
    Pattern {
        entity_type: super::ENTITY_API_KEY,
        key_expr: "CAST(k.id AS TEXT)",
        joins: "LEFT JOIN api_key k ON k.name = r.api_key_name",
    },
    Pattern {
        entity_type: super::ENTITY_VM_MEMBER,
        key_expr: "CASE WHEN pm.model_id IS NULL THEN NULL \
                   ELSE CAST(r.virtual_model_id AS TEXT) || ',' || CAST(pm.model_id AS TEXT) END",
        joins: "LEFT JOIN provider_model pm ON pm.provider_id = r.provider_id AND pm.provider_model_id = r.model_id",
    },
    Pattern {
        entity_type: super::ENTITY_API_KEY_MODEL,
        key_expr: "CASE WHEN k.id IS NULL OR pm.model_id IS NULL THEN NULL \
                   ELSE CAST(k.id AS TEXT) || ',' || CAST(pm.model_id AS TEXT) END",
        joins: "LEFT JOIN api_key k ON k.name = r.api_key_name \
                LEFT JOIN provider_model pm ON pm.provider_id = r.provider_id AND pm.provider_model_id = r.model_id",
    },
];

/// 可加和指标 (别名, 表达式)：同一遍扫描产出全量与成功两个全集——成功全集用
/// `CASE WHEN success = 1` 条件求和，与实时端点「WHERE success = 1 后聚合」
/// 严格等价；无 success 条件的为全量全集口径。与 stats 端点 SQL 逐条对账
/// （spec §指标注册表；TPS 分子 = 成功全集 SUM(output_tokens)，复用
/// OUTPUT_TOKENS，故无独立 tps_out_sum）。
const METRIC_EXPRS: [(&str, &str); 16] = [
    // 全量全集
    (metrics::CALLS, "COUNT(*)"),
    (
        metrics::FAIL_CALLS,
        "SUM(CASE WHEN r.success = 0 THEN 1 ELSE 0 END)",
    ),
    (
        metrics::STREAM_CALLS,
        "SUM(CASE WHEN r.stream THEN 1 ELSE 0 END)",
    ),
    (metrics::TOKENS_ALL, "SUM(r.total_tokens)"),
    (metrics::INPUT_TOKENS_ALL, "SUM(r.input_tokens)"),
    (metrics::CACHE_TOKENS_ALL, "SUM(r.input_cache_tokens)"),
    (
        metrics::SUCCESS_CALLS,
        "SUM(CASE WHEN r.success = 1 THEN 1 ELSE 0 END)",
    ),
    // 成功全集
    (
        metrics::TOTAL_TOKENS,
        "SUM(CASE WHEN r.success = 1 THEN r.total_tokens END)",
    ),
    (
        metrics::INPUT_TOKENS,
        "SUM(CASE WHEN r.success = 1 THEN r.input_tokens END)",
    ),
    (
        metrics::CACHE_TOKENS,
        "SUM(CASE WHEN r.success = 1 THEN r.input_cache_tokens END)",
    ),
    (
        metrics::OUTPUT_TOKENS,
        "SUM(CASE WHEN r.success = 1 THEN r.output_tokens END)",
    ),
    (
        metrics::TTFT_SUM,
        "SUM(CASE WHEN r.success = 1 THEN r.ttft END)",
    ),
    (
        metrics::TTFT_N,
        "SUM(CASE WHEN r.success = 1 AND r.ttft IS NOT NULL THEN 1 ELSE 0 END)",
    ),
    (
        metrics::REQUEST_TIME_SUM,
        "SUM(CASE WHEN r.success = 1 THEN r.request_time END)",
    ),
    (
        metrics::TPS_TIME_SUM,
        "SUM(CASE WHEN r.success = 1 AND r.tps > 0 AND r.output_tokens > 0 \
                  THEN r.output_tokens / r.tps ELSE 0 END)",
    ),
    (
        metrics::OUT_SEC_SUM,
        "SUM(CASE WHEN r.success = 1 AND r.output_tokens_time > 0 \
                  THEN r.output_tokens / (r.output_tokens_time / 1000.0) ELSE 0 END)",
    ),
];

/// 成功全集可加和指标原语（读路径兑底 SQL 与快照端共用，避免两套表达式漂移）：
/// (指标键, 表达式)。表达式自带 success 条件（WHERE 再带 success=1 亦等价）。
pub(crate) const SUCCESS_PRIM_EXPRS: [(&str, &str); 9] = [
    (
        metrics::SUCCESS_CALLS,
        "SUM(CASE WHEN r.success = 1 THEN 1 ELSE 0 END)",
    ),
    (
        metrics::TOTAL_TOKENS,
        "SUM(CASE WHEN r.success = 1 THEN r.total_tokens END)",
    ),
    (
        metrics::INPUT_TOKENS,
        "SUM(CASE WHEN r.success = 1 THEN r.input_tokens END)",
    ),
    (
        metrics::CACHE_TOKENS,
        "SUM(CASE WHEN r.success = 1 THEN r.input_cache_tokens END)",
    ),
    (
        metrics::OUTPUT_TOKENS,
        "SUM(CASE WHEN r.success = 1 THEN r.output_tokens END)",
    ),
    (
        metrics::TTFT_SUM,
        "SUM(CASE WHEN r.success = 1 THEN r.ttft END)",
    ),
    (
        metrics::TTFT_N,
        "SUM(CASE WHEN r.success = 1 AND r.ttft IS NOT NULL THEN 1 ELSE 0 END)",
    ),
    (
        metrics::REQUEST_TIME_SUM,
        "SUM(CASE WHEN r.success = 1 THEN r.request_time END)",
    ),
    (
        metrics::TPS_TIME_SUM,
        "SUM(CASE WHEN r.success = 1 AND r.tps > 0 AND r.output_tokens > 0 \
                  THEN r.output_tokens / r.tps ELSE 0 END)",
    ),
];

/// 固化一个闭桶：可加和指标（7 主体模式）+ hour/day 分位标量，单事务幂等 upsert。
///
/// 事务第一个语句必须是写（先无条件写 whole 哨兵行）：SQLite WAL 下 DEFERRED
/// 事务先读后写时，读快照期间若有其他连接提交过，首次写升级会立即报
/// SQLITE_BUSY_SNAPSHOT（517 database is locked），busy_timeout 无效；写先行
/// 则在无快照的干净点升级，该竞态结构性不可能。非空桶的真实聚合行随后
/// upsert 覆盖哨兵（幂等，终态不变）。
pub async fn finalize_bucket(db: &DatabaseConnection, frame: Frame) -> anyhow::Result<()> {
    let txn = db.begin().await?;
    // 哨兵先行：空桶标志 + 事务写锁早取（升级点无读快照）。非空桶被后续覆盖。
    for (metric, _) in &METRIC_EXPRS {
        upsert_row(&txn, frame, super::ENTITY_WHOLE, "", metric, 0.0).await?;
    }
    let select_list = METRIC_EXPRS
        .iter()
        .map(|(metric, expr)| format!("{expr} AS {metric}"))
        .collect::<Vec<_>>()
        .join(", ");

    for pattern in &PATTERNS {
        let sql = format!(
            "SELECT {key} AS entity, {metrics} \
             FROM request r {joins} \
             WHERE r.start_time >= ? AND r.start_time < ? \
             GROUP BY entity",
            key = pattern.key_expr,
            metrics = select_list,
            joins = pattern.joins,
        );
        let rows = txn
            .query_all_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                sql,
                [frame.start.into(), frame.end.into()],
            ))
            .await?;
        for row in rows {
            let Some(entity) = row.try_get::<String>("", "entity").ok() else {
                continue; // 映射不到（JOIN NULL）的请求不产主体行
            };
            for (metric, _) in &METRIC_EXPRS {
                let value: f64 = row
                    .try_get("", metric)
                    .ok()
                    .or_else(|| row.try_get::<i64>("", metric).ok().map(|v| v as f64))
                    .unwrap_or(0.0);
                upsert_row(&txn, frame, pattern.entity_type, &entity, metric, value).await?;
            }
        }
    }

    if super::percentile_level_ok(frame.level) {
        write_percentiles(&txn, frame).await?;
    }

    txn.commit().await?;
    Ok(())
}

/// 分位标量（与 insight 分位同谓词）：ttft = 成功行且 ttft 非空；
/// request_time = 全部成功行。无样本桶不写行（读侧按 0 补）。
async fn write_percentiles<C: ConnectionTrait>(txn: &C, frame: Frame) -> anyhow::Result<()> {
    for (value_expr, extra_cond, base_metric) in [
        ("r.ttft", "r.ttft IS NOT NULL", metrics::TTFT_P50),
        ("r.request_time", "1 = 1", metrics::REQUEST_TIME_P50),
    ] {
        let mut buckets: std::collections::BTreeMap<(String, String), Vec<f64>> =
            std::collections::BTreeMap::new();
        let sql = format!(
            "SELECT r.provider_id, r.virtual_model_id, pm.model_id AS pm_id, k.id AS ak_id, \
             {value_expr} AS value \
             FROM request r \
             LEFT JOIN provider_model pm ON pm.provider_id = r.provider_id AND pm.provider_model_id = r.model_id \
             LEFT JOIN api_key k ON k.name = r.api_key_name \
             WHERE r.start_time >= ? AND r.start_time < ? AND r.success = 1 AND {extra_cond}"
        );
        let rows = txn
            .query_all_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                sql,
                [frame.start.into(), frame.end.into()],
            ))
            .await?;
        for row in rows {
            let value: f64 = row
                .try_get("", "value")
                .ok()
                .or_else(|| row.try_get::<i64>("", "value").ok().map(|v| v as f64))
                .unwrap_or(0.0);
            let provider_id: i32 = row.try_get("", "provider_id").unwrap_or(0);
            let virtual_model_id: i32 = row.try_get("", "virtual_model_id").unwrap_or(0);
            let pm_id: Option<i32> = row.try_get("", "pm_id").ok();
            let ak_id: Option<i32> = row.try_get("", "ak_id").ok();
            for (entity_type, entity) in row_entities(provider_id, virtual_model_id, pm_id, ak_id) {
                buckets
                    .entry((entity_type.to_string(), entity))
                    .or_default()
                    .push(value);
            }
        }
        for ((entity_type, entity), mut values) in buckets {
            values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let p50_key = base_metric;
            let p90_key = base_metric.replace("_p50", "_p90");
            let p95_key = base_metric.replace("_p50", "_p95");
            let p99_key = base_metric.replace("_p50", "_p99");
            for (p, key) in [
                (percentile(&values, 0.5), p50_key),
                (percentile(&values, 0.9), p90_key.as_str()),
                (percentile(&values, 0.95), p95_key.as_str()),
                (percentile(&values, 0.99), p99_key.as_str()),
            ] {
                upsert_row(txn, frame, &entity_type, &entity, key, p).await?;
            }
        }
    }
    Ok(())
}

/// 一行请求归属的全部主体键（文本与聚合 SQL 的 key_expr 产出严格一致）。
fn row_entities(
    provider_id: i32,
    virtual_model_id: i32,
    pm_id: Option<i32>,
    ak_id: Option<i32>,
) -> Vec<(&'static str, String)> {
    let mut out = vec![
        (super::ENTITY_WHOLE, String::new()),
        (super::ENTITY_PROVIDER, provider_id.to_string()),
        (super::ENTITY_VIRTUAL_MODEL, virtual_model_id.to_string()),
    ];
    if let Some(pm_id) = pm_id {
        out.push((super::ENTITY_MODEL, pm_id.to_string()));
        out.push((
            super::ENTITY_VM_MEMBER,
            format!("{virtual_model_id},{pm_id}"),
        ));
    }
    if let Some(ak_id) = ak_id {
        out.push((super::ENTITY_API_KEY, ak_id.to_string()));
        if let Some(pm_id) = pm_id {
            out.push((super::ENTITY_API_KEY_MODEL, format!("{ak_id},{pm_id}")));
        }
    }
    out
}

async fn upsert_row<C: ConnectionTrait>(
    txn: &C,
    frame: Frame,
    entity_type: &str,
    entity: &str,
    metric: &str,
    value: f64,
) -> anyhow::Result<()> {
    // 参数全部来自内部受控值（枚举键/数字 id/固定指标名），字面量拼接安全；
    // SQLite 只允许 execute_unprepared 走无参写入。
    let sql = format!(
        "INSERT INTO request_log_snapshot \
         (duration_type, start_time, end_time, entity_type, entity, metric_type, metric_value) \
         VALUES ('{}', {}, {}, '{}', '{}', '{}', {}) \
         ON CONFLICT (duration_type, start_time, entity_type, entity, metric_type) \
         DO UPDATE SET metric_value = excluded.metric_value, end_time = excluded.end_time",
        frame.level.key(),
        frame.start,
        frame.end,
        entity_type,
        entity,
        metric,
        value
    );
    txn.execute_unprepared(&sql).await?;
    Ok(())
}

#[cfg(test)]
#[path = "generator_tests.rs"]
mod tests;

//! 赛马/指标共用：六指标快照合并（闭桶读快照原语 + 兑底段实时原语，
//! 展示/排序留在各端点）。单文件约束下从 rank.rs 拆出。

use std::collections::HashMap;

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::stats_snapshot as snap;

/// 六指标原语顺序（与 SUCCESS_PRIM_EXPRS 一一对应）：
/// success_calls, total_tokens, input_tokens, cache_tokens, output_tokens,
/// ttft_sum, ttft_n, request_time_sum, tps_time_sum。
pub(crate) const PRIM_COUNT: usize = 9;

/// 快照/兑底共用 SELECT 列（原语别名列表；顺序与 SUCCESS_PRIM_EXPRS 严格一致，
/// Prims 按下标访问即以此顺序为契约——增删指标两处同步）。
pub(crate) fn prim_select_list() -> String {
    snap::SUCCESS_PRIM_EXPRS
        .iter()
        .map(|(m, e)| format!("{e} AS {m}"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Prims(pub(crate) [f64; PRIM_COUNT]);

impl Prims {
    pub(crate) fn success_calls(&self) -> f64 {
        self.0[0]
    }
    pub(crate) fn total_tokens(&self) -> f64 {
        self.0[1]
    }
    pub(crate) fn input_tokens(&self) -> f64 {
        self.0[2]
    }
    pub(crate) fn cache_tokens(&self) -> f64 {
        self.0[3]
    }
    pub(crate) fn output_tokens(&self) -> f64 {
        self.0[4]
    }
    pub(crate) fn ttft_sum(&self) -> f64 {
        self.0[5]
    }
    pub(crate) fn ttft_n(&self) -> f64 {
        self.0[6]
    }
    pub(crate) fn request_time_sum(&self) -> f64 {
        self.0[7]
    }
    pub(crate) fn tps_time_sum(&self) -> f64 {
        self.0[8]
    }

    /// 现算六指标（与 rank_metric_sql 口径一致：空样本 avg 记 0、tps 分母为 0
    /// 记 0、cache 加权保留 5 位）。
    pub(crate) fn derive(&self) -> (i64, i64, f64, f64, f64, f64) {
        let request_count = self.success_calls().round() as i64;
        let total_tokens = self.total_tokens().round() as i64;
        let ttft = if self.ttft_n() > 0.0 {
            self.ttft_sum() / self.ttft_n()
        } else {
            0.0
        };
        let request_time = if self.success_calls() > 0.0 {
            self.request_time_sum() / self.success_calls()
        } else {
            0.0
        };
        let tps = if self.tps_time_sum() > 0.0 {
            self.output_tokens() / self.tps_time_sum()
        } else {
            0.0
        };
        let cache_hit_rate =
            crate::stats_snapshot::weighted_ratio(self.cache_tokens(), self.input_tokens());
        (
            request_count,
            total_tokens,
            ttft,
            request_time,
            tps,
            cache_hit_rate,
        )
    }
}

/// 把一行聚合结果按 (entity, 指标别名) 并入 map。
fn fold_row(map: &mut HashMap<String, Prims>, entity: String, row: &sea_orm::QueryResult) {
    let entry = map.entry(entity).or_default();
    for (i, (metric, _)) in snap::SUCCESS_PRIM_EXPRS.iter().enumerate() {
        let value: f64 = row
            .try_get("", metric)
            .ok()
            .or_else(|| row.try_get::<i64>("", metric).ok().map(|v| v as f64))
            .unwrap_or(0.0);
        entry.0[i] += value;
    }
}

/// 快照侧原语并入（闭桶帧按 level 分组取行）。
async fn fold_snapshot(
    db: &DatabaseConnection,
    cov: &snap::Coverage,
    entity_type: &str,
    exact: Option<&str>,
    map: &mut HashMap<String, Prims>,
) -> Result<(), String> {
    let mut by_level: std::collections::BTreeMap<snap::Level, Vec<snap::Frame>> =
        std::collections::BTreeMap::new();
    for frame in &cov.snapshots {
        by_level.entry(frame.level).or_default().push(*frame);
    }
    let prim_names: Vec<&str> = snap::SUCCESS_PRIM_EXPRS.iter().map(|(m, _)| *m).collect();
    for (level, frames) in &by_level {
        let rows = snap::snapshot_rows(db, *level, frames, entity_type, exact, &prim_names)
            .await
            .map_err(|e| e.to_string())?;
        for (_, _, entity, metric, value) in rows {
            if let Some(i) = prim_names.iter().position(|m| *m == metric) {
                map.entry(entity).or_default().0[i] += value;
            }
        }
    }
    Ok(())
}

/// 兑底侧原语并入：每段一条分组 SQL（grouped_select 返回 (sql, params)，
/// SQL 须输出 `key` 列 + 九列指标别名，WHERE 只内联段边界与固定值，
/// 可变过滤走 ? 占位由 params 提供）。
pub(crate) async fn fold_live(
    db: &DatabaseConnection,
    cov: &snap::Coverage,
    mut grouped_select: impl FnMut(i64, i64) -> (String, Vec<sea_orm::Value>),
    map: &mut HashMap<String, Prims>,
) -> Result<(), String> {
    for &(s, e) in &cov.live {
        let (sql, params) = grouped_select(s, e);
        let rows = db
            .query_all_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                sql,
                params,
            ))
            .await
            .map_err(|e| e.to_string())?;
        for row in rows {
            let entity: String = row.try_get("", "key").unwrap_or_default();
            fold_row(map, entity, &row);
        }
    }
    Ok(())
}

/// 合并后的主体 → 原语 map。
/// - snap：entity_type 行（exact 指定时只取该主体键）；
/// - live：grouped_select 输出 key（须与快照 entity 文本同域）。
pub(crate) async fn merged_prims(
    db: &DatabaseConnection,
    cov: &snap::Coverage,
    entity_type: &str,
    exact: Option<&str>,
    grouped_select: impl FnMut(i64, i64) -> (String, Vec<sea_orm::Value>),
) -> Result<HashMap<String, Prims>, String> {
    let mut map: HashMap<String, Prims> = HashMap::new();
    fold_snapshot(db, cov, entity_type, exact, &mut map).await?;
    fold_live(db, cov, grouped_select, &mut map).await?;
    Ok(map)
}

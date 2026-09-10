//! 读路径公共层：窗口覆盖计划（闭桶取快照 / 其余实时兑底）。
//! 端点拿到 Coverage 后自行把各 part 的贡献合并进自己的桶/汇总语义
//! （summary/charts/rank/metrics/insight 各端点口径不同，合并留在端点侧，
//! 这里只保证「闭桶判定 + 缺失兑底」是唯一来源）。

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use super::core::{Frame, Level, Part, decompose};

/// 覆盖计划：快照闭桶（帧列表）+ 实时兑底段（相邻已合并）。
#[derive(Clone, Debug, Default)]
pub(crate) struct Coverage {
    pub(crate) snapshots: Vec<Frame>,
    pub(crate) live: Vec<(i64, i64)>,
}

impl Coverage {
    pub(crate) fn is_empty(&self) -> bool {
        self.snapshots.is_empty() && self.live.is_empty()
    }
}

/// 窗口哨兵缺失 → 该闭桶降级为实时兑底段。
async fn demote_missing(db: &DatabaseConnection, level: Level, start: i64) -> anyhow::Result<bool> {
    let row = db
        .query_one_raw(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "SELECT 1 AS v FROM request_log_snapshot \
                 WHERE duration_type = '{}' AND start_time = {start} \
                   AND entity_type = 'whole' AND metric_type = 'calls' \
                 LIMIT 1",
                level.key()
            ),
        ))
        .await?;
    Ok(row.is_none())
}

/// 计算窗口 [start, end) 在 level 粒度下的覆盖计划：
/// 闭桶且哨兵行存在的整帧 → 快照；其余（边缘部分帧、未闭帧、缺哨兵闭帧）→ 兑底。
/// 哨兵存在性按帧集合一次查询（帧级逐查会把全历史窗口退化成上万次查询）。
pub(crate) async fn coverage(
    db: &DatabaseConnection,
    level: Level,
    offset_minutes: i32,
    start: i64,
    end: i64,
    now_ms: i64,
    margin_ms: i64,
) -> anyhow::Result<Coverage> {
    let plan = decompose(level, offset_minutes, start, end, now_ms, margin_ms);
    let mut snaps: Vec<Frame> = Vec::new();
    let mut live: Vec<(i64, i64)> = Vec::new();
    let mut candidates: Vec<Frame> = Vec::new();
    for part in plan {
        match part {
            Part::Snap(frame) => candidates.push(frame),
            Part::Live { start, end } => push_live(&mut live, start, end),
        }
    }
    // 一次查齐候选帧的哨兵（按 level 分组：跨层分解只会产生更细层的帧）。
    let mut by_level: std::collections::BTreeMap<Level, Vec<i64>> =
        std::collections::BTreeMap::new();
    for frame in &candidates {
        by_level.entry(frame.level).or_default().push(frame.start);
    }
    for (frame_level, starts) in by_level {
        let in_list = starts
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT start_time AS st FROM request_log_snapshot \
             WHERE duration_type = '{}' AND start_time IN ({in_list}) \
               AND entity_type = 'whole' AND metric_type = 'calls'",
            frame_level.key()
        );
        let rows = db
            .query_all_raw(Statement::from_string(DbBackend::Sqlite, sql))
            .await?;
        let present: std::collections::HashSet<i64> = rows
            .iter()
            .filter_map(|row| row.try_get("", "st").ok())
            .collect();
        for frame in &candidates {
            if frame.level != frame_level {
                continue;
            }
            if present.contains(&frame.start) {
                snaps.push(*frame);
            } else {
                // 闭桶但缺哨兵行（漏跑/跳跑）：实时兑底（桶本身已闭，语义不变）。
                push_live(&mut live, frame.start, frame.end);
            }
        }
    }
    Ok(Coverage {
        snapshots: snaps,
        live,
    })
}

fn push_live(live: &mut Vec<(i64, i64)>, start: i64, end: i64) {
    if end <= start {
        return;
    }
    if let Some((_, prev_end)) = live.last_mut()
        && *prev_end == start
    {
        *prev_end = end;
        return;
    }
    live.push((start, end));
}

/// 批量取快照行（按帧集合过滤）：
/// 返回 (start_time, entity_type, entity, metric_type, metric_value) 行流，
/// start_time 供调用方把行归属回自己的桶/段。
/// 主体过滤：`entity`（单主体精确）与 `entities`（主体集合，赛马单侧过滤）
/// 互斥传入，都为空时取全量主体（distribution 需要逐主体）。
pub(crate) async fn snapshot_rows(
    db: &DatabaseConnection,
    level: Level,
    frames: &[Frame],
    entity_type: &str,
    entity: Option<&str>,
    entities: Option<&[String]>,
    metrics: &[&str],
) -> anyhow::Result<Vec<(i64, String, String, String, f64)>> {
    if frames.is_empty() {
        return Ok(Vec::new());
    }
    if entities.is_some_and(|e| e.is_empty()) {
        return Ok(Vec::new());
    }
    let starts = frames
        .iter()
        .map(|f| f.start.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let metric_in = metrics
        .iter()
        .map(|m| format!("'{m}'"))
        .collect::<Vec<_>>()
        .join(", ");
    let mut sql = format!(
        "SELECT start_time AS st, entity_type AS et, entity AS e, metric_type AS m, metric_value AS v \
         FROM request_log_snapshot \
         WHERE duration_type = '{}' AND start_time IN ({starts}) \
           AND entity_type = '{entity_type}' AND metric_type IN ({metric_in})",
        level.key()
    );
    if let Some(entity) = entity {
        sql.push_str(&format!(" AND entity = '{entity}'"));
    }
    if let Some(entities) = entities {
        let in_list = entities
            .iter()
            .map(|e| format!("'{e}'"))
            .collect::<Vec<_>>()
            .join(", ");
        sql.push_str(&format!(" AND entity IN ({in_list})"));
    }
    let rows = db
        .query_all_raw(Statement::from_string(DbBackend::Sqlite, sql))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let (st, et, e, m) = (
            row.try_get::<i64>("", "st").ok(),
            row.try_get::<String>("", "et").ok(),
            row.try_get::<String>("", "e").ok(),
            row.try_get::<String>("", "m").ok(),
        );
        let (Some(st), Some(et), Some(e), Some(m)) = (st, et, e, m) else {
            continue;
        };
        let v: f64 = row
            .try_get("", "v")
            .ok()
            .or_else(|| row.try_get::<i64>("", "v").ok().map(|v| v as f64))
            .unwrap_or(0.0);
        out.push((st, et, e, m, v));
    }
    Ok(out)
}

/// 快照初始化完成后（meta initialized=1），最早 day 闭桶之前的兑底段按生成
/// 不变量恒无请求行——裁剪掉，避免全历史窗口每次全表扫描兑底。
pub(crate) async fn trim_zero_prefix(
    db: &DatabaseConnection,
    coverage: Coverage,
) -> anyhow::Result<Coverage> {
    let initialized = db
        .query_one_raw(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT 1 AS v FROM snapshot_meta WHERE key = 'initialized' AND value = '1'"
                .to_string(),
        ))
        .await?
        .is_some();
    if !initialized {
        return Ok(coverage);
    }
    let earliest: Option<i64> = db
        .query_one_raw(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT MIN(start_time) AS v FROM request_log_snapshot \
             WHERE duration_type = 'day' AND entity_type = 'whole' AND metric_type = 'calls'"
                .to_string(),
        ))
        .await?
        .and_then(|row| row.try_get("", "v").ok());
    let Some(earliest) = earliest else {
        return Ok(coverage);
    };
    let mut coverage = coverage;
    coverage.live = coverage
        .live
        .into_iter()
        .filter_map(|(s, e)| {
            let s = s.max(earliest);
            (e > s).then_some((s, e))
        })
        .collect();
    Ok(coverage)
}

/// 快照行里某闭桶（level,start）是否已有对应哨兵（供生成/自愈使用，读侧用
/// `coverage` 的降级逻辑，不需要直接调）。
pub(crate) async fn bucket_finalized(
    db: &DatabaseConnection,
    level: Level,
    start: i64,
) -> anyhow::Result<bool> {
    let row = db
        .query_one_raw(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "SELECT 1 AS v FROM request_log_snapshot \
                 WHERE duration_type = '{}' AND start_time = {start} \
                   AND entity_type = 'whole' AND metric_type = 'calls' LIMIT 1",
                level.key()
            ),
        ))
        .await?;
    Ok(row.is_some())
}

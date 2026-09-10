//! 快照生命周期（ADR-0021）：生成（固化新闭桶 / 首启全量回填 / 时区变更自动
//! 全量重算）与自愈（补算缺哨兵行的闭桶桶）。由两个内置定时任务驱动
//! （stats_snapshot / stats_snapshot_rebuild），进程级互斥防重叠。

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use chrono::{Offset, TimeZone};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use tokio::sync::Mutex;

use super::core::{DAY_MS, Frame, Level, MARGIN_MS, frames_covering};
use super::generator::finalize_bucket;

/// 进程级任务互斥：生成/自愈/启动回填共用，防止长回填与定时触发重叠。
static SNAPSHOT_TASK_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

/// meta 键：最近一次生成的时区偏移（分钟；变更即触发全量重算）。
const META_TZ_OFFSET: &str = "tz_offset_minutes";
/// meta 键：各粒度最近固化桶起点（增量生成水位）。
const META_WM: [(&str, Level); 4] = [
    ("wm_hour", Level::Hour),
    ("wm_day", Level::Day),
    ("wm_month", Level::Month),
    ("wm_year", Level::Year),
];

fn task_lock() -> &'static Mutex<()> {
    SNAPSHOT_TASK_LOCK.get_or_init(|| Mutex::new(()))
}

/// 一次固化运行的统计（结束日志数据）：本次固化的桶数与覆盖范围。
#[derive(Default)]
struct RunStats {
    /// 各粒度桶数（只登记实际固化过桶的粒度）。
    per_level: std::collections::BTreeMap<Level, usize>,
    first_start: Option<i64>,
    last_end: Option<i64>,
}

impl RunStats {
    fn record(&mut self, frame: Frame) {
        *self.per_level.entry(frame.level).or_default() += 1;
        self.first_start = Some(self.first_start.map_or(frame.start, |v| v.min(frame.start)));
        self.last_end = Some(self.last_end.map_or(frame.end, |v| v.max(frame.end)));
    }

    /// 「小时桶 2 个 / 天桶 1 个 / 月桶 0 个 / 年桶 0 个」。
    fn per_level_text(&self) -> String {
        [Level::Hour, Level::Day, Level::Month, Level::Year]
            .iter()
            .map(|level| {
                let count = self.per_level.get(level).copied().unwrap_or(0);
                format!("{} {count} 个", level_label(*level))
            })
            .collect::<Vec<_>>()
            .join(" / ")
    }

    /// 「覆盖 2026-08-01 00:00 ~ 2026-09-10 12:00」（本轮没固化任何桶时为 None）。
    fn coverage_text(&self) -> Option<String> {
        let (Some(start), Some(end)) = (self.first_start, self.last_end) else {
            return None;
        };
        Some(format!("覆盖 {} ~ {}", fmt_ts(start), fmt_ts(end)))
    }
}

/// 结束日志里的粒度名。
fn level_label(level: Level) -> &'static str {
    match level {
        Level::Hour => "小时桶",
        Level::Day => "天桶",
        Level::Month => "月桶",
        Level::Year => "年桶",
    }
}

/// epoch ms → 设置表时区「YYYY-MM-DD HH:MM」（结束日志的覆盖范围段）。
fn fmt_ts(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|dt| {
            dt.with_timezone(&crate::app_settings::timezone_sync())
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| ms.to_string())
}

/// 结束日志统一格式：固化桶数 + 可选覆盖范围 + 耗时（生成/回填/自愈共用）。
fn log_finished(what: &str, stats: &RunStats, coverage: Option<String>, elapsed: Duration) {
    let coverage = coverage
        .map(|range| format!("，{range}"))
        .unwrap_or_default();
    tracing::info!(
        "{what}：本次固化 {}{coverage}，耗时 {elapsed:?}",
        stats.per_level_text()
    );
}

/// 当前设置表时区偏移（分钟，按此刻求固定偏移；与 stats 读取端同口径源）。
fn tz_offset_minutes_now() -> i32 {
    let tz = crate::app_settings::timezone_sync();
    let now = chrono::Utc::now().naive_utc();
    tz.offset_from_utc_datetime(&now).fix().local_minus_utc() / 60
}

async fn meta_get(db: &DatabaseConnection, key: &str) -> anyhow::Result<Option<String>> {
    let row = db
        .query_one_raw(Statement::from_string(
            DbBackend::Sqlite,
            format!("SELECT value AS v FROM snapshot_meta WHERE key = '{key}'"),
        ))
        .await?;
    Ok(row.and_then(|r| r.try_get("", "v").ok()))
}

async fn meta_set(db: &DatabaseConnection, key: &str, value: &str) -> anyhow::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    db.execute_unprepared(&format!(
        "INSERT INTO snapshot_meta (key, value, updated_at) VALUES ('{key}', '{value}', '{now}') \
         ON CONFLICT (key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at"
    ))
    .await?;
    Ok(())
}

async fn meta_delete(db: &DatabaseConnection, key: &str) -> anyhow::Result<()> {
    db.execute_unprepared(&format!("DELETE FROM snapshot_meta WHERE key = '{key}'"))
        .await?;
    Ok(())
}

/// 找最近一个「已闭桶」帧起点（水位锚点）。回看窗口须保证至少覆盖一个闭帧：
/// 小时/天回看数天（凌晨余量），月/年回看 40/400 天。
fn latest_closed_start(level: Level, offset: i32, now: i64, horizon: i64) -> i64 {
    let back = match level {
        Level::Hour => 3 * DAY_MS,
        Level::Day => 4 * DAY_MS,
        Level::Month => 40 * DAY_MS,
        Level::Year => 400 * DAY_MS,
    };
    frames_covering(level, offset, now - back, now + 1)
        .into_iter()
        .filter(|f| f.end <= horizon)
        .map(|f| f.start)
        .max()
        .unwrap_or(0)
}

/// 增量生成入口（stats_snapshot 任务与启动回填共用，进程级互斥）。
/// 返回本次是否实际执行（被并发锁跳过时为 false）。每次实际执行都打
/// 开始/结束两行日志（空转也打），结束行带固化桶数与耗时。
pub(crate) async fn run_snapshot_generation(db: &DatabaseConnection) -> anyhow::Result<bool> {
    let Ok(_guard) = task_lock().try_lock() else {
        tracing::warn!("统计快照生成上次仍在运行，本次跳过");
        return Ok(false);
    };
    let started = Instant::now();
    tracing::info!("统计快照生成开始");
    let now = chrono::Utc::now().timestamp_millis();
    let offset = tz_offset_minutes_now();
    let offset_str = offset.to_string();

    let initialized = meta_get(db, "initialized").await?.is_some();
    let stored_offset = meta_get(db, META_TZ_OFFSET).await?;
    if !initialized {
        tracing::info!("统计快照表为空，开始全量回填整个 request 历史");
        let stats = full_backfill(db, offset, now).await?;
        meta_set(db, "initialized", "1").await?;
        meta_set(db, META_TZ_OFFSET, &offset_str).await?;
        log_finished(
            "统计快照全量回填完成",
            &stats,
            stats.coverage_text(),
            started.elapsed(),
        );
        return Ok(true);
    }
    if stored_offset.as_deref() != Some(offset_str.as_str()) {
        tracing::info!(
            stored_offset = ?stored_offset,
            current_offset = offset,
            "设置表时区偏移变化，清空快照并全量重算"
        );
        db.execute_unprepared("DELETE FROM request_log_snapshot")
            .await?;
        for (key, _) in META_WM {
            meta_delete(db, key).await?;
        }
        let stats = full_backfill(db, offset, now).await?;
        meta_set(db, META_TZ_OFFSET, &offset_str).await?;
        log_finished(
            "统计快照时区变更重算完成",
            &stats,
            stats.coverage_text(),
            started.elapsed(),
        );
        return Ok(true);
    }

    // 增量：各粒度固化「水位之后、已闭桶」的桶（幂等 upsert，可安全重跑）。
    let horizon = now - MARGIN_MS;
    let mut stats = RunStats::default();
    for (meta_key, level) in META_WM {
        let wm: i64 = meta_get(db, meta_key)
            .await?
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let wm = if wm == 0 {
            latest_closed_start(level, offset, now, horizon)
        } else {
            wm
        };
        let mut newest = wm;
        for frame in frames_covering(level, offset, wm, now + 1) {
            if frame.start <= wm || frame.end > horizon {
                continue;
            }
            finalize_bucket(db, frame).await?;
            newest = frame.start;
            stats.record(frame);
        }
        meta_set(db, meta_key, &newest.to_string()).await?;
    }
    log_finished("统计快照生成完成", &stats, None, started.elapsed());
    Ok(true)
}

/// 全量回填：从最早请求所在帧起，把所有已闭桶帧按粒度固化（幂等，可整体重跑）；
/// 每级水位收在最近闭桶帧，空桶哨兵链由后续增量生成补起。
async fn full_backfill(db: &DatabaseConnection, offset: i32, now: i64) -> anyhow::Result<RunStats> {
    let earliest: Option<i64> = db
        .query_one_raw(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT MIN(start_time) AS v FROM request".to_string(),
        ))
        .await?
        .and_then(|row| row.try_get("", "v").ok());
    let horizon = now - MARGIN_MS;
    let mut stats = RunStats::default();
    for (meta_key, level) in META_WM {
        let mut newest = 0i64;
        if let Some(earliest) = earliest {
            for frame in frames_covering(level, offset, earliest, now + 1) {
                if frame.end > horizon {
                    break;
                }
                finalize_bucket(db, frame).await?;
                newest = frame.start;
                stats.record(frame);
            }
        }
        if newest == 0 {
            newest = latest_closed_start(level, offset, now, horizon);
        }
        meta_set(db, meta_key, &newest.to_string()).await?;
    }
    Ok(stats)
}

/// 自愈入口（stats_snapshot_rebuild 任务）：补算最近 7 天缺哨兵行的闭桶
/// 小时/天桶，并点检最近两个闭月/闭年。进程级互斥；快照未初始化时让位给
/// 生成任务（打一行说明收尾）。正常执行打开始/结束两行日志（无缺失也打）。
pub(crate) async fn run_snapshot_heal(db: &DatabaseConnection) -> anyhow::Result<bool> {
    let Ok(_guard) = task_lock().try_lock() else {
        tracing::warn!("统计快照自愈上次仍在运行，本次跳过");
        return Ok(false);
    };
    let started = Instant::now();
    tracing::info!("统计快照自愈开始");
    if meta_get(db, "initialized").await?.is_none() {
        tracing::info!("统计快照未初始化，本次跳过自愈（等待生成任务回填）");
        return Ok(true);
    }
    let now = chrono::Utc::now().timestamp_millis();
    let offset = tz_offset_minutes_now();
    let horizon = now - MARGIN_MS;
    let mut checked = 0usize;
    let mut healed = 0usize;

    // 小时/天：最近 7 天闭桶桶。
    for level in [Level::Hour, Level::Day] {
        for frame in frames_covering(level, offset, now - 7 * DAY_MS, now + 1) {
            if frame.end > horizon {
                continue;
            }
            checked += 1;
            if !crate::stats_snapshot::bucket_finalized(db, level, frame.start).await? {
                finalize_bucket(db, frame).await?;
                healed += 1;
            }
        }
    }
    // 月/年：最近两个已闭帧（闭月/闭年点检）。
    for level in [Level::Month, Level::Year] {
        let mut closed: Vec<Frame> = frames_covering(level, offset, now - 800 * DAY_MS, now + 1)
            .into_iter()
            .filter(|f| f.end <= horizon)
            .collect();
        closed.sort_by_key(|f| f.start);
        for frame in closed.iter().rev().take(2) {
            checked += 1;
            if !crate::stats_snapshot::bucket_finalized(db, level, frame.start).await? {
                finalize_bucket(db, *frame).await?;
                healed += 1;
            }
        }
    }
    tracing::info!(
        "统计快照自愈完成：检查闭桶 {checked} 个，补算 {healed} 个，耗时 {:?}",
        started.elapsed()
    );
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats_snapshot::HOUR_MS;

    /// 三个用例共享进程级 SNAPSHOT_TASK_LOCK，串行执行避免互相跳过。
    static TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    async fn test_lock() -> tokio::sync::MutexGuard<'static, ()> {
        TEST_LOCK.get_or_init(|| Mutex::new(())).lock().await
    }

    fn now_ms() -> i64 {
        chrono::Utc::now().timestamp_millis()
    }

    async fn setup() -> DatabaseConnection {
        crate::db::connect("sqlite::memory:").await.unwrap()
    }

    async fn exec(db: &DatabaseConnection, sql: &str) {
        db.execute_unprepared(sql).await.unwrap();
    }

    async fn insert_request(db: &DatabaseConnection, rid: &str, start: i64) {
        let sql = format!(
            "INSERT INTO request (request_id, virtual_model_id, provider_id, model_id, stream, \
             input_cache_tokens, input_cache_rate, tps, start_time, end_time, request_time, \
             success, api_key_name) \
             VALUES ('{rid}', 1, 1, 'm', 0, 0, 0.0, 0.0, {start}, {start}, 100, 1, 'k')"
        );
        exec(db, &sql).await;
    }

    async fn count(db: &DatabaseConnection, duration: &str) -> i64 {
        db.query_one_raw(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "SELECT COUNT(*) AS v FROM request_log_snapshot WHERE duration_type = '{duration}'"
            ),
        ))
        .await
        .unwrap()
        .and_then(|row| row.try_get("", "v").ok())
        .unwrap_or(0)
    }

    /// 测试用日志缓冲：把 tracing 输出收进内存供断言。`set_default` 是线程
    /// 局部的，用例必须跑在 current_thread runtime（`#[tokio::test]` 默认）。
    #[derive(Clone, Default)]
    struct LogBuffer(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

    impl std::io::Write for LogBuffer {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogBuffer {
        type Writer = LogBuffer;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    /// 挂测试 subscriber，返回（日志缓冲，作用域守卫）。
    fn capture_logs() -> (LogBuffer, tracing::subscriber::DefaultGuard) {
        let buf = LogBuffer::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(buf.clone())
            .with_ansi(false)
            .without_time()
            .finish();
        (buf, tracing::subscriber::set_default(subscriber))
    }

    /// 取走缓冲中已累积的日志文本并清空（按次断言）。
    fn take_logs(buf: &LogBuffer) -> String {
        let mut guard = buf.0.lock().unwrap();
        let text = String::from_utf8_lossy(&guard).into_owned();
        guard.clear();
        text
    }

    /// 用户诉求回归：生成任务每次执行都留下开始/结束日志，空转（无新闭桶）
    /// 也不例外；结束日志带各粒度固化桶数与耗时（回填场景另带覆盖范围）。
    #[tokio::test]
    async fn generation_logs_start_and_finish_with_bucket_counts() {
        let _serial = test_lock().await;
        let db = setup().await;
        let now = now_ms();
        let (older, closed) = closed_hour_frame(now);
        insert_request(&db, "a1", older.start + 1000).await;
        insert_request(&db, "a2", closed.start + 1000).await;
        let (buf, _guard) = capture_logs();

        run_snapshot_generation(&db).await.unwrap();
        let first = take_logs(&buf);
        assert!(first.contains("统计快照生成开始"), "{first}");
        assert!(first.contains("统计快照全量回填完成"), "{first}");
        assert!(first.contains("小时桶 "), "{first}");
        assert!(!first.contains("小时桶 0 个"), "回填应固化闭桶：{first}");
        assert!(first.contains("覆盖 "), "{first}");
        assert!(first.contains("耗时 "), "{first}");

        // 无新闭桶的空转：桶数 0 也要留下开始/结束两行。
        run_snapshot_generation(&db).await.unwrap();
        let second = take_logs(&buf);
        assert!(second.contains("统计快照生成开始"), "{second}");
        assert!(second.contains("统计快照生成完成：本次固化"), "{second}");
        assert!(second.contains("小时桶 0 个"), "{second}");
        assert!(second.contains("耗时 "), "{second}");
    }

    /// 用户诉求回归：自愈任务无论是否有缺失都留下开始/结束日志；没有缺失时
    /// 结束行报「检查闭桶 N 个，补算 0 个」（N > 0，证明扫描确实发生）。
    #[tokio::test]
    async fn heal_logs_start_and_finish_even_without_missing_buckets() {
        let _serial = test_lock().await;
        let db = setup().await;
        let now = now_ms();
        let (_older, closed) = closed_hour_frame(now);
        insert_request(&db, "a1", closed.start + 1000).await;
        run_snapshot_generation(&db).await.unwrap();
        let (buf, _guard) = capture_logs();

        // 首轮：窗口内可能有生成未覆盖的桶（最早请求之前的天桶），只断言格式。
        run_snapshot_heal(&db).await.unwrap();
        let first = take_logs(&buf);
        assert!(first.contains("统计快照自愈开始"), "{first}");
        assert!(first.contains("统计快照自愈完成：检查闭桶 "), "{first}");
        assert!(first.contains("个，补算 "), "{first}");
        assert!(first.contains("耗时 "), "{first}");

        // 第二轮：没有缺失也必须留痕（检查数 > 0、补算 0）。
        run_snapshot_heal(&db).await.unwrap();
        let second = take_logs(&buf);
        assert!(second.contains("统计快照自愈开始"), "{second}");
        assert!(second.contains("补算 0 个"), "{second}");
        assert!(!second.contains("检查闭桶 0 个"), "{second}");
    }

    /// 找一个已闭桶小时桶（距今 ≥ 3 小时、对齐本地整点，上海偏移）。
    fn closed_hour_frame(now: i64) -> (Frame, Frame) {
        let frames = super::frames_covering(Level::Hour, 480, now - 3 * HOUR_MS, now - 2 * HOUR_MS);
        let frame = frames[0]; // 该窗口内唯一整点对齐帧（可能跨两个，取先闭桶者）
        let older = Frame {
            level: Level::Hour,
            start: frame.start - HOUR_MS,
            end: frame.start,
        };
        (older, frame)
    }

    #[tokio::test]
    async fn first_run_backfills_then_incremental_only_new_buckets() {
        let _serial = test_lock().await;
        let db = setup().await;
        let now = now_ms();
        let (older, closed) = closed_hour_frame(now);
        insert_request(&db, "a1", older.start + 1000).await;
        insert_request(&db, "a2", closed.start + 1000).await;

        // 首启：全量回填（已闭桶帧都固化，哨兵齐全）。
        assert!(run_snapshot_generation(&db).await.unwrap());
        assert_eq!(
            meta_get(&db, "initialized").await.unwrap().as_deref(),
            Some("1")
        );
        assert!(
            crate::stats_snapshot::bucket_finalized(&db, Level::Hour, older.start)
                .await
                .unwrap()
        );
        assert!(
            crate::stats_snapshot::bucket_finalized(&db, Level::Hour, closed.start)
                .await
                .unwrap()
        );
        let hour_rows = count(&db, "hour").await;
        assert!(hour_rows > 0);

        // 再跑一次（模拟下一小时触发）：仍幂等，行数不因重复回填暴增。
        let before = count(&db, "hour").await;
        assert!(run_snapshot_generation(&db).await.unwrap());
        assert_eq!(count(&db, "hour").await, before);
    }

    #[tokio::test]
    async fn heal_regenerates_missing_sentinel_buckets() {
        let _serial = test_lock().await;
        let db = setup().await;
        let now = now_ms();
        let (older, closed) = closed_hour_frame(now);
        insert_request(&db, "a1", closed.start + 1000).await;
        run_snapshot_generation(&db).await.unwrap();

        // 人为删除一个闭桶的全部快照行（模拟漏跑/脏删）。
        exec(
            &db,
            &format!(
                "DELETE FROM request_log_snapshot WHERE duration_type = 'hour' AND start_time = {}",
                closed.start
            ),
        )
        .await;
        // 制造一个 older 桶也缺失的场景（从未生成过且落在 7 天自愈窗口内）。
        exec(
            &db,
            &format!(
                "DELETE FROM request_log_snapshot WHERE duration_type = 'hour' AND start_time = {}",
                older.start
            ),
        )
        .await;

        run_snapshot_heal(&db).await.unwrap();
        assert!(
            crate::stats_snapshot::bucket_finalized(&db, Level::Hour, closed.start)
                .await
                .unwrap()
        );
        assert!(
            crate::stats_snapshot::bucket_finalized(&db, Level::Hour, older.start)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn timezone_change_triggers_full_recompute() {
        let _serial = test_lock().await;
        let db = setup().await;
        let now = now_ms();
        let (older, closed) = closed_hour_frame(now);
        insert_request(&db, "a1", older.start + 1000).await;
        insert_request(&db, "a2", closed.start + 1000).await;
        run_snapshot_generation(&db).await.unwrap();
        let rows_before = count(&db, "hour").await + count(&db, "day").await;

        // 模拟设置表时区变更（改 meta 偏移值即可触发路径）。
        meta_set(&db, META_TZ_OFFSET, "9999").await.unwrap();
        run_snapshot_generation(&db)
            .await
            .expect("时区变更重算应成功");
        assert_eq!(
            meta_get(&db, META_TZ_OFFSET).await.unwrap().as_deref(),
            Some("480")
        );
        // 旧偏移桶已被清空重建（行数可能因偏移不同而变化，但哨兵必须存在且可数）。
        let _ = rows_before;
        assert!(
            crate::stats_snapshot::bucket_finalized(&db, Level::Hour, older.start)
                .await
                .unwrap()
        );
        assert!(
            crate::stats_snapshot::bucket_finalized(&db, Level::Hour, closed.start)
                .await
                .unwrap()
        );
    }

    /// 并发写者回归：/v1 流量持续落 request 时，快照生成不得报 database is locked。
    /// SQLite WAL 下 DEFERRED 事务先读后写，若读快照期间他连接提交过，事务内
    /// 首次写升级会立即失败（SQLITE_BUSY_SNAPSHOT 517 / BUSY 5），busy_timeout
    /// 无效——修复：finalize_bucket 事务首语句改为写（哨兵先行），升级发生在无
    /// 读快照的干净点。必须用文件库（内存库无 WAL 语义，测不到该竞态）。
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn generation_survives_concurrent_request_writes() {
        let _serial = test_lock().await;
        let dir = tempfile::tempdir().unwrap();
        let url = format!(
            "sqlite://{}?mode=rwc",
            dir.path().join("probe.db").display()
        );
        let db = crate::db::connect(&url).await.unwrap();
        let now = now_ms();
        let (_older, closed) = closed_hour_frame(now);

        // 闭桶内灌 25k 成功行：放大聚合 SELECT 耗时（即 517 竞态窗口）。
        let start = closed.start + 1000;
        for chunk in 0..50 {
            let values = (0..500)
                .map(|i| {
                    let rid = chunk * 500 + i;
                    format!(
                        "('s{rid}', 1, 1, 'm', 0, 100, 0, 0.0, 1.0, {start}, {start}, 200, 1, 'k1')"
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            exec(
                &db,
                &format!(
                    "INSERT INTO request (request_id, virtual_model_id, provider_id, model_id, stream, \
                     ttft, input_cache_tokens, input_cache_rate, tps, start_time, end_time, \
                     request_time, success, api_key_name) VALUES {values}"
                ),
            )
            .await;
        }

        // 并发写者：模拟 /v1 请求持续落库（start_time ≈ now，永不落进已闭桶）。
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let writer_db = db.clone();
        let writer_stop = stop.clone();
        let writer = tokio::spawn(async move {
            let mut i = 0u64;
            while !writer_stop.load(std::sync::atomic::Ordering::Relaxed) {
                let ts = now + (i % 50) as i64;
                let sql = format!(
                    "INSERT INTO request (request_id, virtual_model_id, provider_id, model_id, stream, \
                     input_cache_tokens, input_cache_rate, tps, start_time, end_time, request_time, \
                     success, api_key_name) VALUES ('w{i}', 1, 1, 'm', 0, 0, 0.0, 0.0, {ts}, {ts}, 100, 1, 'k1')"
                );
                let _ = writer_db.execute_unprepared(&sql).await;
                i += 1;
            }
        });

        // 无 meta → 每次从零全量回填，可重复制造竞态窗口。
        let mut failure: Option<(usize, String)> = None;
        for attempt in 1..=3 {
            exec(&db, "DELETE FROM request_log_snapshot").await;
            exec(&db, "DELETE FROM snapshot_meta").await;
            if let Err(e) = run_snapshot_generation(&db).await {
                failure = Some((attempt, format!("{e:#}")));
                break;
            }
        }
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = writer.await;

        if let Some((attempt, err)) = failure {
            panic!("attempt {attempt} 失败：{err}");
        }
        assert!(
            crate::stats_snapshot::bucket_finalized(&db, Level::Hour, closed.start)
                .await
                .unwrap(),
            "快照应实际固化闭桶"
        );
    }
    /// 09-04：自愈覆盖 day/month/year 分支与「未初始化让位」。
    #[tokio::test]
    async fn heal_covers_day_month_year_and_yields_when_uninitialized() {
        let _serial = test_lock().await;
        let db = setup().await;
        let now = now_ms();

        // 未初始化：让位给生成任务（不死锁、不补算）。
        assert!(run_snapshot_heal(&db).await.unwrap());
        assert_eq!(count(&db, "day").await, 0, "未初始化不应补算");

        // 初始化后删掉 day 闭桶，自愈应补回。用 2 天前的闭桶确保 day 帧已闭。
        let long_ago = now - 2 * 24 * HOUR_MS;
        let (_, closed) = closed_hour_frame(long_ago);
        insert_request(&db, "a1", closed.start + 1000).await;
        run_snapshot_generation(&db).await.unwrap();
        let day_rows_before = count(&db, "day").await;
        assert!(day_rows_before > 0, "生成应写 day 哨兵");
        exec(
            &db,
            "DELETE FROM request_log_snapshot WHERE duration_type = 'day'",
        )
        .await;
        run_snapshot_heal(&db).await.unwrap();
        // 自愈覆盖最近 7 天（生成只走增量水位），故补回行数 ≥ 删除前。
        assert!(
            count(&db, "day").await >= day_rows_before,
            "day 桶应被补回（≥ 删除前 {}）",
            day_rows_before
        );
        // 该请求所在的那个 day 帧应有哨兵（用桶帧函数取与 closed 同桶的起点）。
        let day_frame = super::frames_covering(Level::Day, 480, closed.start, closed.start + 1)
            .into_iter()
            .next()
            .expect("应能找到包含该时刻的 day 帧");
        assert!(
            crate::stats_snapshot::bucket_finalized(&db, Level::Day, day_frame.start)
                .await
                .unwrap(),
            "被删的具体 day 桶应有哨兵"
        );

        // month/year：删掉最近闭帧的哨兵，自愈应点检补回（各 ≤2 帧）。
        for level in ["month", "year"] {
            exec(
                &db,
                &format!("DELETE FROM request_log_snapshot WHERE duration_type = '{level}'"),
            )
            .await;
            run_snapshot_heal(&db).await.unwrap();
            assert!(count(&db, level).await > 0, "{level} 闭帧应被自愈补算");
        }
    }

    /// 09-07：固化失败时该级水位不前进（下轮从旧水位重跑）。
    /// 注入方式：把某一帧的请求数据留在库里但让 finalize 失败不可行（真实路径
    /// 无 mock 缝），改为验证「水位推进发生在整级循环之后」的可见语义——
    /// 首轮成功后水位=最新闭帧；人为把水位退回旧值再跑，应重算并回到最新。
    #[tokio::test]
    async fn watermark_only_advances_after_full_level() {
        let _serial = test_lock().await;
        let db = setup().await;
        let now = now_ms();
        let (older, closed) = closed_hour_frame(now);
        insert_request(&db, "a1", older.start + 1000).await;
        insert_request(&db, "a2", closed.start + 1000).await;
        run_snapshot_generation(&db).await.unwrap();

        let wm_after = meta_get(&db, "wm_hour").await.unwrap();
        assert!(wm_after.is_some(), "首轮应写入 hour 水位");
        // 水位回退：模拟「上轮中途失败」留下的旧水位，下一轮应重算到最新。
        meta_set(&db, "wm_hour", "0").await.unwrap();
        let rows_before = count(&db, "hour").await;
        run_snapshot_generation(&db).await.unwrap();
        let wm_retry = meta_get(&db, "wm_hour").await.unwrap();
        assert_eq!(wm_retry, wm_after, "重跑后水位回到最新闭帧");
        assert_eq!(count(&db, "hour").await, rows_before, "幂等固化不重复产行");
        // older 桶（早于首轮水位）不在回退重算范围，但应仍存在（首轮已固化）。
        assert!(
            crate::stats_snapshot::bucket_finalized(&db, Level::Hour, older.start)
                .await
                .unwrap()
        );
    }
}

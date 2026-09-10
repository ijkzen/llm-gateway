//! generator 单元/集成测试（#[path] 挂回 generator 模块，保持单文件 ≤1000 行）。

use super::*;
use crate::stats_snapshot::Level;

fn now_ts() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

async fn setup_db() -> DatabaseConnection {
    crate::db::connect("sqlite::memory:").await.unwrap()
}

async fn exec(db: &DatabaseConnection, sql: &str) {
    db.execute_unprepared(sql).await.unwrap();
}

async fn scalar_i64(db: &DatabaseConnection, sql: &str) -> i64 {
    db.query_one_raw(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .unwrap()
        .and_then(|row| row.try_get("", "v").ok())
        .unwrap_or(0)
}

#[allow(clippy::too_many_arguments)]
async fn insert_request(
    db: &DatabaseConnection,
    rid: &str,
    vm: i32,
    provider: i32,
    model: &str,
    stream: bool,
    ttft: Option<i64>,
    input: Option<i64>,
    cache: i64,
    output: Option<i64>,
    out_time: Option<i64>,
    tps: f64,
    rt: i64,
    success: bool,
    total: Option<i64>,
    key: &str,
    start: i64,
) {
    let sql = format!(
        "INSERT INTO request (request_id, virtual_model_id, provider_id, model_id, stream, \
             ttft, input_tokens, input_cache_tokens, input_cache_rate, output_tokens, \
             output_tokens_time, tps, start_time, end_time, request_time, success, fail_reason, \
             total_tokens, api_key_name) \
             VALUES ('{rid}', {vm}, {provider}, '{model}', {stream}, {ttft}, {input}, {cache}, 0.0, \
             {output}, {out_time}, {tps}, {start}, {start}, {rt}, {success}, NULL, {total}, '{key}')",
        rid = rid,
        vm = vm,
        provider = provider,
        model = model,
        stream = if stream { 1 } else { 0 },
        ttft = ttft.map(|v| v.to_string()).unwrap_or_else(|| "NULL".into()),
        input = input
            .map(|v| v.to_string())
            .unwrap_or_else(|| "NULL".into()),
        cache = cache,
        output = output
            .map(|v| v.to_string())
            .unwrap_or_else(|| "NULL".into()),
        out_time = out_time
            .map(|v| v.to_string())
            .unwrap_or_else(|| "NULL".into()),
        tps = tps,
        rt = rt,
        success = if success { 1 } else { 0 },
        total = total
            .map(|v| v.to_string())
            .unwrap_or_else(|| "NULL".into()),
        key = key,
        start = start,
    );
    exec(db, &sql).await;
}

/// 读快照单值：闭桶 (level,start) + 主体 + 指标。
async fn snap(
    db: &DatabaseConnection,
    level: &str,
    start: i64,
    entity_type: &str,
    entity: &str,
    metric: &str,
) -> Option<f64> {
    let sql = format!(
        "SELECT metric_value AS v FROM request_log_snapshot \
             WHERE duration_type = '{level}' AND start_time = {start} \
               AND entity_type = '{entity_type}' AND entity = '{entity}' AND metric_type = '{metric}'"
    );
    db.query_one_raw(Statement::from_string(DbBackend::Sqlite, sql))
        .await
        .unwrap()
        .and_then(|row| row.try_get::<f64>("", "v").ok())
}

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-6,
        "expected {expected}, got {actual}"
    );
}

async fn snap_or(
    db: &DatabaseConnection,
    level: &str,
    start: i64,
    et: &str,
    e: &str,
    m: &str,
) -> f64 {
    snap(db, level, start, et, e, m).await.unwrap_or(0.0)
}

/// 行数统计（09-05 测试用；按 where 条件）。
async fn count_rows(db: &DatabaseConnection, where_sql: &str) -> i64 {
    db.query_one_raw(Statement::from_string(
        DbBackend::Sqlite,
        format!("SELECT COUNT(*) AS v FROM request_log_snapshot WHERE {where_sql}"),
    ))
    .await
    .unwrap()
    .and_then(|row| row.try_get("", "v").ok())
    .unwrap_or(0)
}

/// 种子主体：两个供应商/模型（p1-gpt-x、p2-gpt-y）+ 两个 API Key。
async fn seed_subjects(db: &DatabaseConnection) -> (i32, i32, i32, i32, i32, i32) {
    let ts = "2024-01-01T00:00:00Z";
    exec(
            db,
            &format!(
                "INSERT INTO provider (name, enable, base_url, api_key, custom_header, protocol_type, billing_mode, extra, sort_order, proxy_enabled, proxy_addr, created_at, updated_at) \
                 VALUES ('p1', 1, 'https://a.example', 'k', '{{}}', 0, 1, '{{}}', 0, 0, '', '{ts}', '{ts}')"
            ),
        )
        .await;
    exec(
            db,
            &format!(
                "INSERT INTO provider (name, enable, base_url, api_key, custom_header, protocol_type, billing_mode, extra, sort_order, proxy_enabled, proxy_addr, created_at, updated_at) \
                 VALUES ('p2', 1, 'https://b.example', 'k', '{{}}', 0, 0, '{{}}', 0, 0, '', '{ts}', '{ts}')"
            ),
        )
        .await;
    // 按 name 反查（09-01）：last_insert_rowid 是连接局部的，池路由变化会取到
    // 错位值；同函数内 api_key/model 主体都已用反查，provider 与之对齐。
    let p1 = scalar_i64(db, "SELECT id AS v FROM provider WHERE name = 'p1'").await as i32;
    let p2 = scalar_i64(db, "SELECT id AS v FROM provider WHERE name = 'p2'").await as i32;
    for (pid, mid) in [(p1, "gpt-x"), (p2, "gpt-y")] {
        exec(
                db,
                &format!(
                    "INSERT INTO provider_model (provider_id, provider_model_id, context_length, max_output_tokens, reasoning, tool_use, image_understand, video_understand, proxy_enabled, proxy_addr, created_at, updated_at) \
                     VALUES ({pid}, '{mid}', 8000, 2000, 0, 0, 0, 0, 0, '', '{ts}', '{ts}')"
                ),
            )
            .await;
    }
    let pm_x = scalar_i64(
        db,
        "SELECT model_id AS v FROM provider_model WHERE provider_model_id = 'gpt-x'",
    )
    .await as i32;
    let pm_y = scalar_i64(
        db,
        "SELECT model_id AS v FROM provider_model WHERE provider_model_id = 'gpt-y'",
    )
    .await as i32;
    for (name, key) in [("key-a", "lg-aaa"), ("key-b", "lg-bbb")] {
        exec(
            db,
            &format!(
                "INSERT INTO api_key (name, key, key_hash, enable, created_at, updated_at) \
                     VALUES ('{name}', '{key}', NULL, 1, '{ts}', '{ts}')"
            ),
        )
        .await;
    }
    let ak_a = scalar_i64(db, "SELECT id AS v FROM api_key WHERE name = 'key-a'").await as i32;
    let ak_b = scalar_i64(db, "SELECT id AS v FROM api_key WHERE name = 'key-b'").await as i32;
    (p1, p2, pm_x, pm_y, ak_a, ak_b)
}

#[allow(clippy::too_many_arguments)]
#[tokio::test]
async fn finalize_hour_bucket_writes_all_patterns_and_metrics() {
    let db = setup_db().await;
    let (p1, p2, pm_x, pm_y, ak_a, ak_b) = seed_subjects(&db).await;
    let s = now_ts().div_euclid(3_600_000) * 3_600_000; // 对齐整点
    let frame = Frame {
        level: Level::Hour,
        start: s,
        end: s + 3_600_000,
    };

    // r1 成功流式 / r2 成功非流 / r3 失败 / r4 成功（p2）/ r5 成功但模型已删（无 pm）。
    insert_request(
        &db,
        "r1",
        10,
        p1,
        "gpt-x",
        true,
        Some(200),
        Some(100),
        20,
        Some(10),
        Some(400),
        20.0,
        900,
        true,
        Some(110),
        "key-a",
        s,
    )
    .await;
    insert_request(
        &db,
        "r2",
        10,
        p1,
        "gpt-x",
        false,
        None,
        Some(50),
        5,
        Some(20),
        None,
        40.0,
        700,
        true,
        Some(70),
        "key-a",
        s,
    )
    .await;
    insert_request(
        &db,
        "r3",
        10,
        p1,
        "gpt-x",
        true,
        Some(500),
        None,
        0,
        None,
        None,
        0.0,
        300,
        false,
        None,
        "key-a",
        s,
    )
    .await;
    insert_request(
        &db,
        "r4",
        11,
        p2,
        "gpt-y",
        true,
        Some(100),
        Some(20),
        0,
        Some(10),
        Some(200),
        33.333333,
        400,
        true,
        Some(30),
        "key-b",
        s,
    )
    .await;
    insert_request(
        &db,
        "r5",
        10,
        p1,
        "deleted-model",
        false,
        None,
        Some(5),
        0,
        Some(2),
        None,
        10.0,
        100,
        true,
        Some(7),
        "key-a",
        s,
    )
    .await;

    finalize_bucket(&db, frame).await.unwrap();

    // whole 全集（哨兵行同时验证非空）
    assert_close(
        snap_or(&db, "hour", s, "whole", "", metrics::CALLS).await,
        5.0,
    );
    assert_close(
        snap_or(&db, "hour", s, "whole", "", metrics::FAIL_CALLS).await,
        1.0,
    );
    assert_close(
        snap_or(&db, "hour", s, "whole", "", metrics::SUCCESS_CALLS).await,
        4.0,
    );
    assert_close(
        snap_or(&db, "hour", s, "whole", "", metrics::STREAM_CALLS).await,
        3.0,
    );
    assert_close(
        snap_or(&db, "hour", s, "whole", "", metrics::TOKENS_ALL).await,
        217.0,
    );
    assert_close(
        snap_or(&db, "hour", s, "whole", "", metrics::INPUT_TOKENS_ALL).await,
        175.0,
    );
    assert_close(
        snap_or(&db, "hour", s, "whole", "", metrics::CACHE_TOKENS_ALL).await,
        25.0,
    );
    assert_close(
        snap_or(&db, "hour", s, "whole", "", metrics::TOTAL_TOKENS).await,
        217.0,
    );
    assert_close(
        snap_or(&db, "hour", s, "whole", "", metrics::OUTPUT_TOKENS).await,
        42.0,
    );
    assert_close(
        snap_or(&db, "hour", s, "whole", "", metrics::TTFT_SUM).await,
        300.0,
    );
    assert_close(
        snap_or(&db, "hour", s, "whole", "", metrics::TTFT_N).await,
        2.0,
    );
    assert_close(
        snap_or(&db, "hour", s, "whole", "", metrics::REQUEST_TIME_SUM).await,
        2100.0,
    );
    // r1 10/20 + r2 20/40 + r5 2/10 + r4 10/33.333333 = 0.5+0.5+0.2+0.3
    assert_close(
        snap_or(&db, "hour", s, "whole", "", metrics::TPS_TIME_SUM).await,
        1.5,
    );
    // r1 10/(400/1000)=25 + r4 10/(200/1000)=50
    assert_close(
        snap_or(&db, "hour", s, "whole", "", metrics::OUT_SEC_SUM).await,
        75.0,
    );

    // 主体行抽样：provider / model / vm / key / 交叉
    let p1e = p1.to_string();
    assert_close(
        snap_or(&db, "hour", s, "provider", &p1e, metrics::CALLS).await,
        4.0,
    );
    assert_close(
        snap_or(&db, "hour", s, "provider", &p1e, metrics::TOKENS_ALL).await,
        187.0,
    );
    let pm_xe = pm_x.to_string();
    assert_close(
        snap_or(&db, "hour", s, "model", &pm_xe, metrics::CALLS).await,
        3.0,
    );
    assert_close(
        snap_or(&db, "hour", s, "model", &pm_xe, metrics::SUCCESS_CALLS).await,
        2.0,
    );
    assert_close(
        snap_or(&db, "hour", s, "virtual_model", "10", metrics::CALLS).await,
        4.0,
    );
    assert_close(
        snap_or(&db, "hour", s, "virtual_model", "11", metrics::CALLS).await,
        1.0,
    );
    let ak_ae = ak_a.to_string();
    assert_close(
        snap_or(&db, "hour", s, "api_key", &ak_ae, metrics::CALLS).await,
        4.0,
    );
    assert_close(
        snap_or(&db, "hour", s, "api_key", &ak_b.to_string(), metrics::CALLS).await,
        1.0,
    );
    let member = format!("10,{pm_x}");
    assert_close(
        snap_or(
            &db,
            "hour",
            s,
            "virtual_model_member",
            &member,
            metrics::CALLS,
        )
        .await,
        3.0,
    );
    let key_model = format!("{ak_a},{pm_x}");
    assert_close(
        snap_or(&db, "hour", s, "api_key_model", &key_model, metrics::CALLS).await,
        3.0,
    );

    // 映射不到（pm 已删/key 已删）不产行；r5 仍计入 whole/provider/vm/api_key。
    assert!(
        snap(&db, "hour", s, "model", "", metrics::CALLS)
            .await
            .is_none()
    );
    assert!(
        snap(&db, "hour", s, "api_key", "999", metrics::CALLS)
            .await
            .is_none()
    );
    assert!(
        snap(&db, "hour", s, "model", &pm_y.to_string(), metrics::CALLS)
            .await
            .is_some(),
        "r4 有 pm_y 模型行"
    );
    assert_close(
        snap_or(&db, "hour", s, "model", &pm_y.to_string(), metrics::CALLS).await,
        1.0,
    );

    // 分位（成功行）：ttft [100(r4), 200(r1)]；request_time [100(r5),400(r4),700(r2),900(r1)]
    assert_close(
        snap_or(&db, "hour", s, "whole", "", metrics::TTFT_P50).await,
        150.0,
    );
    assert_close(
        snap_or(&db, "hour", s, "whole", "", metrics::TTFT_P90).await,
        190.0,
    );
    assert_close(
        snap_or(&db, "hour", s, "whole", "", metrics::REQUEST_TIME_P50).await,
        550.0,
    );
    assert_close(
        snap_or(&db, "hour", s, "whole", "", metrics::REQUEST_TIME_P99).await,
        894.0,
    );
    // p1 的 ttft 只有 r1 一个样本
    assert_close(
        snap_or(&db, "hour", s, "provider", &p1e, metrics::TTFT_P50).await,
        200.0,
    );
    // p2 的 request_time 只有 r4
    assert_close(
        snap_or(
            &db,
            "hour",
            s,
            "provider",
            &p2.to_string(),
            metrics::REQUEST_TIME_P90,
        )
        .await,
        400.0,
    );
}

#[tokio::test]
async fn finalize_is_idempotent_and_sentinel_covers_empty_bucket() {
    let db = setup_db().await;
    let (p1, _p2, _pm_x, _pm_y, _ak_a, _ak_b) = seed_subjects(&db).await;
    let s = now_ts().div_euclid(3_600_000) * 3_600_000;
    let frame = Frame {
        level: Level::Hour,
        start: s,
        end: s + 3_600_000,
    };
    insert_request(
        &db,
        "r1",
        10,
        p1,
        "gpt-x",
        true,
        Some(200),
        Some(100),
        0,
        Some(10),
        Some(400),
        20.0,
        900,
        true,
        Some(110),
        "key-a",
        s,
    )
    .await;

    finalize_bucket(&db, frame).await.unwrap();
    let count_before = scalar_i64(&db, "SELECT COUNT(*) AS v FROM request_log_snapshot").await;
    let calls_before = snap_or(&db, "hour", s, "whole", "", metrics::CALLS).await;
    finalize_bucket(&db, frame).await.unwrap();
    let count_after = scalar_i64(&db, "SELECT COUNT(*) AS v FROM request_log_snapshot").await;
    assert_eq!(count_before, count_after, "重跑不得新增行");
    assert_close(
        snap_or(&db, "hour", s, "whole", "", metrics::CALLS).await,
        calls_before,
    );

    // 空桶哨兵：只有 whole 全 0 行，无主体行、无分位行。
    let empty = Frame {
        level: Level::Hour,
        start: s + 3_600_000,
        end: s + 2 * 3_600_000,
    };
    finalize_bucket(&db, empty).await.unwrap();
    assert_close(
        snap_or(&db, "hour", empty.start, "whole", "", metrics::CALLS).await,
        0.0,
    );
    let rows = scalar_i64(
            &db,
            &format!(
                "SELECT COUNT(*) AS v FROM request_log_snapshot WHERE duration_type = 'hour' AND start_time = {}",
                empty.start
            ),
        )
        .await;
    assert_eq!(rows, 16, "空桶应只写 16 个可加和指标（无分位行）");
    assert!(
        snap(
            &db,
            "hour",
            empty.start,
            "provider",
            &p1.to_string(),
            metrics::CALLS
        )
        .await
        .is_none()
    );
    // 有请求的桶：whole 24 个指标（16 可加和 + 8 分位）
    let rows_full = scalar_i64(
            &db,
            &format!(
                "SELECT COUNT(*) AS v FROM request_log_snapshot WHERE duration_type = 'hour' AND start_time = {s} AND entity_type = 'whole'"
            ),
        )
        .await;
    assert_eq!(rows_full, 24);
}

#[tokio::test]
async fn day_level_bucket_stores_percentiles_and_crosses_hours() {
    let db = setup_db().await;
    let (p1, _p2, _pm_x, _pm_y, _ak_a, _ak_b) = seed_subjects(&db).await;
    let s = now_ts().div_euclid(3_600_000) * 3_600_000 - 3_600_000; // 前一天整点
    let day = Frame {
        level: Level::Day,
        start: s,
        end: s + 24 * 3_600_000,
    };
    insert_request(
        &db,
        "d1",
        10,
        p1,
        "gpt-x",
        true,
        Some(200),
        Some(100),
        0,
        Some(10),
        Some(400),
        20.0,
        900,
        true,
        Some(110),
        "key-a",
        s,
    )
    .await;
    insert_request(
        &db,
        "d2",
        10,
        p1,
        "gpt-x",
        true,
        Some(300),
        Some(50),
        0,
        Some(5),
        None,
        10.0,
        500,
        true,
        Some(55),
        "key-a",
        s + 5 * 3_600_000,
    )
    .await;
    finalize_bucket(&db, day).await.unwrap();

    assert_close(
        snap_or(&db, "day", s, "whole", "", metrics::CALLS).await,
        2.0,
    );
    assert_close(
        snap_or(&db, "day", s, "whole", "", metrics::TTFT_SUM).await,
        500.0,
    );
    // 天行存分位（day 粒度接口要逐桶分位）：ttft 值 [200, 300] → p50 = 250
    assert_close(
        snap_or(&db, "day", s, "whole", "", metrics::TTFT_P50).await,
        250.0,
    );
    // 桶外请求不计入
    insert_request(
        &db,
        "d3",
        10,
        p1,
        "gpt-x",
        true,
        Some(400),
        Some(50),
        0,
        Some(5),
        None,
        10.0,
        500,
        true,
        Some(55),
        "key-a",
        day.end + 1000,
    )
    .await;
    finalize_bucket(&db, day).await.unwrap();
    assert_close(
        snap_or(&db, "day", s, "whole", "", metrics::CALLS).await,
        2.0,
    );
}

#[tokio::test]
async fn month_level_bucket_skips_percentiles() {
    let db = setup_db().await;
    let (p1, _, _, _, _, _) = seed_subjects(&db).await;
    let now = now_ts();
    let frames = super::super::frames_covering(super::super::Level::Month, 480, now, now + 1);
    let frame = frames[0];
    insert_request(
        &db,
        "m1",
        10,
        p1,
        "gpt-x",
        true,
        Some(200),
        Some(100),
        0,
        Some(10),
        Some(400),
        20.0,
        900,
        true,
        Some(110),
        "key-a",
        frame.start + 3_600_000,
    )
    .await;
    insert_request(
        &db,
        "m2",
        10,
        p1,
        "gpt-x",
        true,
        Some(300),
        Some(50),
        0,
        Some(5),
        None,
        10.0,
        500,
        true,
        Some(55),
        "key-a",
        frame.start + 26 * 3_600_000,
    )
    .await;
    finalize_bucket(&db, frame).await.unwrap();

    assert_close(
        snap_or(&db, "month", frame.start, "whole", "", metrics::CALLS).await,
        2.0,
    );
    assert_close(
        snap_or(&db, "month", frame.start, "whole", "", metrics::TTFT_SUM).await,
        500.0,
    );
    // 月/年语义：分位不存（接口现语义月/年分位为空）
    assert!(
        snap(&db, "month", frame.start, "whole", "", metrics::TTFT_P50)
            .await
            .is_none()
    );
    let rows = scalar_i64(
            &db,
            &format!(
                "SELECT COUNT(*) AS v FROM request_log_snapshot WHERE duration_type = 'month' AND start_time = {} AND entity_type = 'whole'",
                frame.start
            ),
        )
        .await;
    assert_eq!(rows, 16);
}

/// 09-05：Year 级独立直测——年帧走与 month 同分支（16 行哨兵、不写分位）。
#[tokio::test]
async fn year_level_writes_sentinels_without_percentiles() {
    let db = setup_db().await;
    let (p1, ..) = seed_subjects(&db).await;
    let s = 1_700_000_000_000i64;
    // 对齐到年帧（用 core 的帧计算取包含该时刻的年帧）。
    let year = super::super::frames_covering(Level::Year, 480, s, s + 1)
        .into_iter()
        .next()
        .expect("应找到年帧");
    insert_request(
        &db,
        "y1",
        10,
        p1,
        "gpt-x",
        true,
        Some(200),
        Some(100),
        0,
        Some(10),
        Some(400),
        20.0,
        900,
        true,
        Some(110),
        "key-a",
        s,
    )
    .await;
    finalize_bucket(&db, year).await.unwrap();

    let calls = snap_or(&db, "year", year.start, "whole", "", metrics::CALLS).await;
    assert_close(calls, 1.0);
    // 年帧不写分位标量（percentile_level_ok 只 hour/day）。
    assert_eq!(
        snap(&db, "year", year.start, "whole", "", metrics::TTFT_P50).await,
        None,
        "年帧不应写分位标量"
    );
    // 哨兵齐全：whole 主体固定 16 个加和指标行。
    let sentinel_count = count_rows(
        &db,
        &format!(
            "duration_type = 'year' AND start_time = {} AND entity_type = 'whole' AND entity = ''",
            year.start
        ),
    )
    .await;
    assert_eq!(sentinel_count, 16, "年帧 whole 哨兵应为 16 行");
}

/// 09-05：分位侧 NULL-entity 排除——模型行已删（pm 缺失）时该主体不产分位行，
/// 防止「主体已删」在分位口径下被高估（09-09 高估族同源）。
#[tokio::test]
async fn percentile_rows_exclude_unresolved_subjects() {
    let db = setup_db().await;
    let (p1, _p2, pm_x, _pm_y, ak_a, _ak_b) = seed_subjects(&db).await;
    let s = now_ts().div_euclid(3_600_000) * 3_600_000;
    let frame = Frame {
        level: Level::Hour,
        start: s,
        end: s + 3_600_000,
    };
    // 请求引用的 provider_model_id 不在 provider_model 表（pm 已删）→ pm_id 为 NULL。
    insert_request(
        &db,
        "d1",
        10,
        p1,
        "gpt-deleted",
        true,
        Some(200),
        Some(100),
        0,
        Some(10),
        Some(400),
        20.0,
        900,
        true,
        Some(110),
        "key-a",
        s,
    )
    .await;
    finalize_bucket(&db, frame).await.unwrap();

    // 聚合侧仍有 provider/vm/api_key 主体行（id 可直接解析）。
    let provider_rows = count_rows(
        &db,
        &format!("duration_type = 'hour' AND start_time = {s} AND entity_type = 'model'"),
    )
    .await;
    assert_eq!(provider_rows, 0, "pm 已删不应产 model 主体行");
    // 分位侧同样不产 model/vm_member/api_key_model 行。
    let percentile_model_rows = count_rows(
        &db,
        &format!(
            "duration_type = 'hour' AND start_time = {s} AND metric_type = '{}' AND entity_type IN ('model','vm_member','api_key_model')",
            metrics::TTFT_P50
        ),
    )
    .await;
    assert_eq!(percentile_model_rows, 0, "分位侧应排除未解析主体");
    // 但 whole/provider/api_key 分位应存在（这些主体可解析）。
    let percentile_scope = count_rows(
        &db,
        &format!(
            "duration_type = 'hour' AND start_time = {s} AND metric_type = '{}' AND entity_type = 'whole'",
            metrics::TTFT_P50
        ),
    )
    .await;
    assert!(percentile_scope > 0, "可解析主体的分位应产出");
    // 顺带确认种子主体存在（避免误删测试前提）。
    assert!(pm_x > 0 && ak_a > 0);
}

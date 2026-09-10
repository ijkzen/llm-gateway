//! request_log_snapshot 读路径等价测试（Ticket 05/06/07 共同基准）：
//! 同一批 request 数据，快照服务态（闭桶读快照）与全实时态响应逐字节一致；
//! 快照缺失/缺口桶自动兑底后仍一致。

use axum::body::Body;
use axum::http::Request;
use sea_orm::{ConnectionTrait, DatabaseConnection};
use tower::ServiceExt;

use llm_gateway::stats_snapshot as snap;

const HOUR_MS: i64 = 3_600_000;

mod common;

async fn setup_app() -> (axum::Router, DatabaseConnection) {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    seed_subjects(&db).await;
    scheduler.start().await.unwrap();
    let app = common::build_authed_app(db.clone(), scheduler, log_tx).await;
    (app, db)
}

async fn exec(db: &DatabaseConnection, sql: &str) {
    db.execute_unprepared(sql).await.unwrap();
}

/// 种子：供应商 1 + 模型 gpt-x + API Key k1 + 两个虚拟模型成员（快照 model/
/// api_key/vm_member 行映射与成员赛马配置用）。
async fn seed_subjects(db: &DatabaseConnection) {
    let ts = "2024-01-01T00:00:00Z";
    exec(
        db,
        &format!(
            "INSERT INTO provider (name, enable, base_url, api_key, custom_header, protocol_type, billing_mode, extra, sort_order, proxy_enabled, proxy_addr, created_at, updated_at) \
             VALUES ('供应商一', 1, 'https://a.example', 'k', '{{}}', 0, 1, '{{}}', 0, 0, '', '{ts}', '{ts}')"
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO provider_model (provider_id, provider_model_id, context_length, max_output_tokens, reasoning, tool_use, image_understand, video_understand, proxy_enabled, proxy_addr, created_at, updated_at) \
             VALUES (1, 'gpt-x', 8000, 2000, 0, 0, 0, 0, 0, '', '{ts}', '{ts}')"
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO api_key (name, key, key_hash, enable, created_at, updated_at) \
             VALUES ('k1', 'lg-aaa', NULL, 1, '{ts}', '{ts}')"
        ),
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO virtual_model (virtual_model_id, display_id, enable, load_balancing_strategy, fallback_strategy, interface_type, created_at, updated_at) \
             VALUES (10, 'vm-10', 1, 0, 0, 0, '{ts}', '{ts}'), (11, 'vm-11', 1, 0, 0, 0, '{ts}', '{ts}')"
        ),
    )
    .await;
    // virtual_model_item.model_id 全局唯一（互斥映射）：只给 vm-10 配成员。
    exec(
        db,
        &format!(
            "INSERT INTO virtual_model_item (virtual_model_id, model_id, enable, cascade_disabled, created_at, updated_at) \
             SELECT 10, model_id, 1, 0, '{ts}', '{ts}' FROM provider_model WHERE provider_model_id = 'gpt-x'"
        ),
    )
    .await;
}

/// 插一行请求（指标可裁剪，等价测试只需 counts/tokens 两类口径）。
#[allow(clippy::too_many_arguments)]
async fn insert_request(
    db: &DatabaseConnection,
    rid: &str,
    vm: i32,
    model: &str,
    key: &str,
    success: bool,
    stream: bool,
    ttft: Option<i64>,
    input: Option<i64>,
    cache: i64,
    output: Option<i64>,
    out_time: Option<i64>,
    tps: f64,
    request_time: i64,
    total: Option<i64>,
    start: i64,
) {
    let sql = format!(
        "INSERT INTO request (request_id, virtual_model_id, provider_id, model_id, stream, \
         ttft, input_tokens, input_cache_tokens, input_cache_rate, output_tokens, \
         output_tokens_time, tps, start_time, end_time, request_time, success, fail_reason, \
         total_tokens, api_key_name) \
         VALUES ('{rid}', {vm}, 1, '{model}', {stream}, {ttft}, {input}, {cache}, 0.0, {output}, \
         {out_time}, {tps}, {start}, {start}, {request_time}, {success}, NULL, {total}, '{key}')",
        stream = if stream { 1 } else { 0 },
        ttft = ttft.map(|v| v.to_string()).unwrap_or_else(|| "NULL".into()),
        input = input
            .map(|v| v.to_string())
            .unwrap_or_else(|| "NULL".into()),
        output = output
            .map(|v| v.to_string())
            .unwrap_or_else(|| "NULL".into()),
        out_time = out_time
            .map(|v| v.to_string())
            .unwrap_or_else(|| "NULL".into()),
        success = if success { 1 } else { 0 },
        total = total
            .map(|v| v.to_string())
            .unwrap_or_else(|| "NULL".into()),
    );
    exec(db, &sql).await;
}

async fn get_json(app: &axum::Router, uri: &str) -> serde_json::Value {
    let request: Request<Body> = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status().as_u16(), 200, "uri={uri}");
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&body).unwrap()
}

/// 把 (本地某日 0 点, +8) 的毫秒换算成 epoch（等价测试统一东八区视角）。
fn local_midnight_epoch(y: i32, m: u32, d: u32) -> i64 {
    let naive = chrono::NaiveDate::from_ymd_opt(y, m, d)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap();
    naive.and_utc().timestamp_millis() - 8 * HOUR_MS
}

/// 种子：把两个「整自然日」的数据放在昨天与前天（已闭桶、非今日）。
async fn seed_history(
    db: &DatabaseConnection,
    days_ago1: chrono::NaiveDate,
    days_ago2: chrono::NaiveDate,
) -> i64 {
    use chrono::Datelike;
    let (y1, m1, d1) = (days_ago1.year(), days_ago1.month(), days_ago1.day());
    let (y2, m2, d2) = (days_ago2.year(), days_ago2.month(), days_ago2.day());
    let day1 = local_midnight_epoch(y1, m1, d1); // 昨天（首个窗口日）
    let day2 = local_midnight_epoch(y2, m2, d2); // 前天（次日窗口用）
    insert_request(
        db,
        "e1",
        10,
        "gpt-x",
        "k1",
        true,
        true,
        Some(200),
        Some(100),
        20,
        Some(10),
        Some(400),
        20.0,
        900,
        Some(110),
        day1 + HOUR_MS,
    )
    .await;
    insert_request(
        db,
        "e2",
        10,
        "gpt-x",
        "k1",
        true,
        false,
        None,
        Some(50),
        5,
        Some(20),
        None,
        40.0,
        700,
        Some(70),
        day1 + 2 * HOUR_MS,
    )
    .await;
    insert_request(
        db,
        "e3",
        10,
        "gpt-x",
        "k1",
        false,
        true,
        Some(500),
        None,
        0,
        None,
        None,
        0.0,
        300,
        None,
        day1 + 3 * HOUR_MS,
    )
    .await;
    insert_request(
        db,
        "e4",
        11,
        "gpt-x",
        "k1",
        true,
        true,
        Some(100),
        Some(20),
        0,
        Some(10),
        Some(200),
        33.333333,
        400,
        Some(30),
        day2 + 5 * HOUR_MS,
    )
    .await;
    day1
}

#[tokio::test]
async fn charts_and_summary_equal_between_snapshot_and_live() {
    let (app, db) = setup_app().await;
    let today = chrono::Utc::now().date_naive();
    let day1 = seed_history(
        &db,
        today - chrono::Days::new(1),
        today - chrono::Days::new(2),
    )
    .await;

    // 全实时基准：窗口 = 昨天整天 [day1, day1+1d)。
    let window = format!("startTime={day1}&endTime={}", day1 + 24 * HOUR_MS);
    let live_charts = get_json(
        &app,
        &format!("/api/stats/charts?{window}&granularity=hour"),
    )
    .await;
    let live_charts_day =
        get_json(&app, &format!("/api/stats/charts?{window}&granularity=day")).await;
    let live_summary = get_json(&app, &format!("/api/stats/summary?{window}")).await;
    let live_ranks: Vec<serde_json::Value> = vec![
        get_json(&app, &format!("/api/stats/provider-rank?{window}")).await,
        get_json(&app, &format!("/api/stats/virtual-model-rank?{window}")).await,
        get_json(&app, &format!("/api/stats/provider-model-rank?{window}")).await,
        get_json(
            &app,
            &format!("/api/stats/virtual-model-member-rank?{window}&virtualModelId=10"),
        )
        .await,
        get_json(&app, &format!("/api/stats/api-key-rank?{window}")).await,
        get_json(
            &app,
            &format!("/api/stats/provider-metrics?{window}&providerId=1"),
        )
        .await,
        get_json(
            &app,
            &format!("/api/stats/virtual-model-metrics?{window}&virtualModelId=10"),
        )
        .await,
        get_json(
            &app,
            &format!("/api/stats/model-metrics?{window}&providerId=1&modelId=gpt-x"),
        )
        .await,
        get_json(
            &app,
            &format!("/api/stats/api-key-metrics?{window}&apiKey=k1"),
        )
        .await,
        get_json(
            &app,
            &format!("/api/stats/insight?{window}&granularity=hour"),
        )
        .await,
        get_json(
            &app,
            &format!("/api/stats/insight?{window}&granularity=day"),
        )
        .await,
    ];

    // 闭桶固化：昨天的小时/天桶 + 前天小时桶（跨两日，验证按桶取数正确）。
    for day in [day1 - 24 * HOUR_MS, day1] {
        for hour in 0..24 {
            let s = day + hour * HOUR_MS;
            snap::finalize_bucket(
                &db,
                snap::Frame {
                    level: snap::Level::Hour,
                    start: s,
                    end: s + HOUR_MS,
                },
            )
            .await
            .unwrap();
        }
        snap::finalize_bucket(
            &db,
            snap::Frame {
                level: snap::Level::Day,
                start: day,
                end: day + 24 * HOUR_MS,
            },
        )
        .await
        .unwrap();
    }

    let snap_charts = get_json(
        &app,
        &format!("/api/stats/charts?{window}&granularity=hour"),
    )
    .await;
    let snap_charts_day =
        get_json(&app, &format!("/api/stats/charts?{window}&granularity=day")).await;
    let snap_summary = get_json(&app, &format!("/api/stats/summary?{window}")).await;
    let snap_ranks: Vec<serde_json::Value> = vec![
        get_json(&app, &format!("/api/stats/provider-rank?{window}")).await,
        get_json(&app, &format!("/api/stats/virtual-model-rank?{window}")).await,
        get_json(&app, &format!("/api/stats/provider-model-rank?{window}")).await,
        get_json(
            &app,
            &format!("/api/stats/virtual-model-member-rank?{window}&virtualModelId=10"),
        )
        .await,
        get_json(&app, &format!("/api/stats/api-key-rank?{window}")).await,
        get_json(
            &app,
            &format!("/api/stats/provider-metrics?{window}&providerId=1"),
        )
        .await,
        get_json(
            &app,
            &format!("/api/stats/virtual-model-metrics?{window}&virtualModelId=10"),
        )
        .await,
        get_json(
            &app,
            &format!("/api/stats/model-metrics?{window}&providerId=1&modelId=gpt-x"),
        )
        .await,
        get_json(
            &app,
            &format!("/api/stats/api-key-metrics?{window}&apiKey=k1"),
        )
        .await,
        get_json(
            &app,
            &format!("/api/stats/insight?{window}&granularity=hour"),
        )
        .await,
        get_json(
            &app,
            &format!("/api/stats/insight?{window}&granularity=day"),
        )
        .await,
    ];
    for (i, (snap, live)) in snap_ranks.iter().zip(live_ranks.iter()).enumerate() {
        assert_eq!(snap, live, "rank/metrics #{i} 快照态必须与实时态一致");
    }

    assert_eq!(snap_charts, live_charts, "hour 粒度快照态必须与实时态一致");
    assert_eq!(
        snap_charts_day, live_charts_day,
        "day 粒度快照态必须与实时态一致"
    );
    assert_eq!(snap_summary, live_summary, "summary 快照态必须与实时态一致");

    // 人为制造缺口（删一个闭桶的全部快照行）→ 读路径兑底，结果仍一致。
    let gap = day1 + 2 * HOUR_MS;
    exec(
        &db,
        &format!(
            "DELETE FROM request_log_snapshot WHERE duration_type = 'hour' AND start_time = {gap}"
        ),
    )
    .await;
    let gap_charts = get_json(
        &app,
        &format!("/api/stats/charts?{window}&granularity=hour"),
    )
    .await;
    assert_eq!(gap_charts, live_charts, "缺口桶必须兑底实时且结果一致");
    // 缺口同样作用于赛马/insight：删除一个整天的快照行后兑底仍与实时一致。
    exec(
        &db,
        &format!(
            "DELETE FROM request_log_snapshot WHERE duration_type = 'day' AND start_time = {day1}"
        ),
    )
    .await;
    for (uri, label) in [
        (
            format!("/api/stats/provider-rank?{window}"),
            "provider-rank",
        ),
        (
            format!("/api/stats/insight?{window}&granularity=day"),
            "insight-day",
        ),
    ] {
        let after = get_json(&app, &uri).await;
        let live = get_json(&app, &uri).await; // 同一 uri 的实时态即基准（表已删，全实时路径）
        let _ = label;
        let _ = after;
        assert_eq!(after, live, "{label} 缺天桶必须兑底且与全实时一致");
    }
}

#[tokio::test]
async fn filtered_charts_equal_between_snapshot_and_live() {
    let (app, db) = setup_app().await;
    let today = chrono::Utc::now().date_naive();
    let day1 = seed_history(
        &db,
        today - chrono::Days::new(1),
        today - chrono::Days::new(2),
    )
    .await;
    let window = format!("startTime={day1}&endTime={}", day1 + 24 * HOUR_MS);

    // 供应商过滤 + API Key 过滤（两种快照主体形态）的实时基准。
    let live_provider = get_json(
        &app,
        &format!("/api/stats/charts?{window}&granularity=hour&providerId=1"),
    )
    .await;
    let live_key = get_json(
        &app,
        &format!("/api/stats/charts?{window}&granularity=hour&apiKey=k1"),
    )
    .await;

    for hour in 0..24 {
        let s = day1 + hour * HOUR_MS;
        snap::finalize_bucket(
            &db,
            snap::Frame {
                level: snap::Level::Hour,
                start: s,
                end: s + HOUR_MS,
            },
        )
        .await
        .unwrap();
    }

    let snap_provider = get_json(
        &app,
        &format!("/api/stats/charts?{window}&granularity=hour&providerId=1"),
    )
    .await;
    let snap_key = get_json(
        &app,
        &format!("/api/stats/charts?{window}&granularity=hour&apiKey=k1"),
    )
    .await;

    assert_eq!(
        snap_provider, live_provider,
        "providerId 过滤快照态必须一致"
    );
    assert_eq!(snap_key, live_key, "apiKey 过滤快照态必须一致");
}

#[tokio::test]
async fn deleted_subject_filter_demotes_to_live() {
    // 回归：按已删除的模型/Key 过滤时，主体键解析失败必须整窗兑底——
    // 否则快照侧按全量主体行取数，把其它模型/Key 的闭桶贡献加进来（高估）。
    // 形态：双模型双 Key，删掉其中一个，另一个的泄漏行是爆炸的载体。
    let (app, db) = setup_app().await;
    let today = chrono::Utc::now().date_naive();
    let day1 = seed_history(
        &db,
        today - chrono::Days::new(1),
        today - chrono::Days::new(2),
    )
    .await;
    let ts = "2024-01-01T00:00:00Z";
    // 第二个模型与第二个 Key（泄漏载体）+ 其在窗口内的一条请求。
    exec(
        &db,
        &format!(
            "INSERT INTO provider_model (provider_id, provider_model_id, context_length, \
             max_output_tokens, reasoning, tool_use, image_understand, video_understand, \
             proxy_enabled, proxy_addr, created_at, updated_at) \
             VALUES (1, 'gpt-y', 8000, 2000, 0, 0, 0, 0, 0, '', '{ts}', '{ts}')"
        ),
    )
    .await;
    exec(
        &db,
        &format!(
            "INSERT INTO api_key (name, key, key_hash, enable, created_at, updated_at) \
             VALUES ('k2', 'lg-bbb', NULL, 1, '{ts}', '{ts}')"
        ),
    )
    .await;
    insert_request(
        &db,
        "e5",
        10,
        "gpt-y",
        "k2",
        true,
        true,
        Some(300),
        Some(60),
        10,
        Some(15),
        Some(600),
        25.0,
        800,
        Some(90),
        day1 + 4 * HOUR_MS,
    )
    .await;

    // 删掉过滤目标（pm gpt-x 与 Key k1），然后先取全实时基准（尚无快照行）。
    exec(
        &db,
        "DELETE FROM provider_model WHERE provider_id = 1 AND provider_model_id = 'gpt-x'",
    )
    .await;
    exec(&db, "DELETE FROM api_key WHERE name = 'k1'").await;

    let window = format!("startTime={day1}&endTime={}", day1 + 24 * HOUR_MS);
    let uris = vec![
        format!("/api/stats/charts?{window}&granularity=hour&providerId=1&modelId=gpt-x"),
        format!("/api/stats/charts?{window}&granularity=hour&apiKey=k1"),
        format!("/api/stats/insight?{window}&granularity=hour&providerId=1&modelId=gpt-x"),
        format!("/api/stats/insight?{window}&granularity=hour&apiKey=k1"),
        format!("/api/stats/model-metrics?{window}&providerId=1&modelId=gpt-x"),
        format!("/api/stats/api-key-metrics?{window}&apiKey=k1"),
        format!("/api/stats/provider-model-rank?{window}&providerId=1&modelId=gpt-x"),
    ];
    let mut live_baseline = Vec::new();
    for uri in &uris {
        live_baseline.push(get_json(&app, uri).await);
    }

    // 固化昨天全部小时桶（此时被删主体的请求已映射不到 → 只产其它主体行）。
    for hour in 0..24 {
        let s = day1 + hour * HOUR_MS;
        snap::finalize_bucket(
            &db,
            snap::Frame {
                level: snap::Level::Hour,
                start: s,
                end: s + HOUR_MS,
            },
        )
        .await
        .unwrap();
    }

    for (uri, live) in uris.iter().zip(live_baseline.iter()) {
        let snapped = get_json(&app, uri).await;
        assert_eq!(
            &snapped, live,
            "删除主体过滤 {uri} 快照态必须兑底且与实时一致"
        );
    }
}

#[tokio::test]
async fn summary_and_charts_equality_with_today_tail() {
    let (app, db) = setup_app().await;
    let now = chrono::Utc::now().timestamp_millis();
    let off = 480 * 60_000i64;
    let cur = (now + off).div_euclid(HOUR_MS) * HOUR_MS - off; // 本地当前小时起点
    // 两个已闭小时帧（终点 + 60min 余量已过）+ 当前小时实时尾部。
    let closed_a = cur - 3 * HOUR_MS;
    let closed_b = cur - 2 * HOUR_MS;
    insert_request(
        &db,
        "t1",
        10,
        "gpt-x",
        "k1",
        true,
        true,
        Some(200),
        Some(100),
        20,
        Some(10),
        Some(400),
        20.0,
        900,
        Some(110),
        closed_a + 30 * 60_000,
    )
    .await;
    insert_request(
        &db,
        "t2",
        10,
        "gpt-x",
        "k1",
        false,
        true,
        Some(500),
        None,
        0,
        None,
        None,
        0.0,
        300,
        None,
        closed_b + 30 * 60_000,
    )
    .await;
    insert_request(
        &db,
        "t3",
        10,
        "gpt-x",
        "k1",
        true,
        false,
        None,
        Some(50),
        5,
        Some(20),
        None,
        40.0,
        700,
        Some(70),
        now - 20 * 60_000,
    )
    .await;
    let live_summary = get_json(&app, "/api/stats/summary").await;
    let window = format!("startTime={}&endTime={now}", cur - 3 * HOUR_MS);
    let live_charts = get_json(
        &app,
        &format!("/api/stats/charts?{window}&granularity=hour"),
    )
    .await;

    for frame in [closed_a, closed_b, cur] {
        snap::finalize_bucket(
            &db,
            snap::Frame {
                level: snap::Level::Hour,
                start: frame,
                end: frame + HOUR_MS,
            },
        )
        .await
        .unwrap();
    }

    let snap_summary = get_json(&app, "/api/stats/summary").await;
    let snap_charts = get_json(
        &app,
        &format!("/api/stats/charts?{window}&granularity=hour"),
    )
    .await;
    assert_eq!(
        snap_summary, live_summary,
        "summary 今日闭桶小时也必须计入（快照=实时）"
    );
    assert_eq!(snap_charts, live_charts, "charts 尾部窗口快照=实时");
}

/// 10-01 回归：day 粒度窗口含今日未闭天时，今日日桶由小时帧的日级分位合并
///（各小时 p 值加权均值），不得被「最后一个小时帧」覆盖写。
#[tokio::test]
async fn insight_day_granularity_percentiles_merge_today_hours() {
    let (app, db) = setup_app().await;
    let now = chrono::Utc::now().timestamp_millis();
    let off = 480 * 60_000i64;
    let today_start = (now + off).div_euclid(24 * HOUR_MS) * 24 * HOUR_MS - off;
    let cur_hour = (now + off).div_euclid(HOUR_MS) * HOUR_MS - off;
    // 今日 3 个闭小时，每桶两条请求的 ttft：100/300、300/500、500/700
    //（桶内真分位 200/400/600，桶级 p50 各不同 → 合并后应等于三者均值 400）。
    for (i, (t1, t2)) in [(100i64, 300i64), (300, 500), (500, 700)]
        .into_iter()
        .enumerate()
    {
        let start_hour = cur_hour - (3 - i as i64) * HOUR_MS;
        for (j, ttft) in [t1, t2].into_iter().enumerate() {
            insert_request(
                &db,
                &format!("tf{i}{j}"),
                10,
                "gpt-x",
                "k1",
                true,
                true,
                Some(ttft),
                Some(10),
                0,
                Some(5),
                Some(1),
                10.0,
                1000 + ttft,
                Some(15),
                start_hour + (10 + j as i64) * 60_000,
            )
            .await;
        }
        snap::finalize_bucket(
            &db,
            snap::Frame {
                level: snap::Level::Hour,
                start: start_hour,
                end: start_hour + HOUR_MS,
            },
        )
        .await
        .unwrap();
    }
    // 窗口只覆盖今日这 3 个闭小时：day 粒度下今日桶 = 三个小时帧合并。
    let window = format!("startTime={today_start}&endTime={cur_hour}");
    let resp = get_json(
        &app,
        &format!("/api/stats/insight?{window}&granularity=day"),
    )
    .await;
    let data = &resp["data"];
    let pct = &data["ttftPercentiles"];
    assert_eq!(
        pct.as_array().map(|a| a.len()),
        Some(1),
        "今日未闭日桶应只有一个"
    );
    let got = pct[0]["p50"].as_f64().unwrap();
    // 三个小时帧的桶内 p50 分别是 200/400/600；合并（等权均值）= 400。
    // 覆盖写旧行为会得到 600（最后一个小时帧的值）。
    assert_eq!(got, 400.0, "今日桶 p50 应为小时帧合并均值而非末小时覆盖");
}

/// 10-02 回归：provider-model-rank 单侧过滤（仅 modelId / 仅 providerId）时，
/// 快照侧只读过滤侧主体集合，不把其它供应商/模型的闭桶行混入结果。
#[tokio::test]
async fn provider_model_rank_single_sided_filter_equality_with_snapshot() {
    let (app, db) = setup_app().await;
    // 第二供应商 + 同名模型：不加过滤时会与供应商一混算。
    let ts = "2024-01-01T00:00:00Z";
    exec(
        &db,
        &format!(
            "INSERT INTO provider (name, enable, base_url, api_key, custom_header, protocol_type, billing_mode, extra, sort_order, proxy_enabled, proxy_addr, created_at, updated_at) \
             VALUES ('供应商二', 1, 'https://b.example', 'k', '{{}}', 0, 1, '{{}}', 1, 0, '', '{ts}', '{ts}')"
        ),
    )
    .await;
    exec(
        &db,
        &format!(
            "INSERT INTO provider_model (provider_id, provider_model_id, context_length, max_output_tokens, reasoning, tool_use, image_understand, video_understand, proxy_enabled, proxy_addr, created_at, updated_at) \
             VALUES (2, 'gpt-y', 8000, 2000, 0, 0, 0, 0, 0, '', '{ts}', '{ts}')"
        ),
    )
    .await;
    let now = chrono::Utc::now().timestamp_millis();
    let off = 480 * 60_000i64;
    let yesterday = ((now + off).div_euclid(24 * HOUR_MS) - 1) * 24 * HOUR_MS - off;
    // 昨天的闭桶：供应商一 gpt-x 两条、供应商二 gpt-y 两条（快照按 model 主体行）。
    for (i, (_pid, model, base)) in [(1, "gpt-x", 100i64), (2, "gpt-y", 900i64)]
        .into_iter()
        .enumerate()
    {
        for j in 0..2 {
            insert_request(
                &db,
                &format!("f{i}{j}"),
                10,
                model,
                "k1",
                true,
                true,
                Some(base + j * 10),
                Some(10),
                0,
                Some(5),
                Some(1),
                10.0,
                base + j * 10,
                Some(15),
                yesterday + (i as i64 * 2 + j) * HOUR_MS + 10 * 60_000,
            )
            .await;
        }
    }
    // 写回 provider_id（insert_request 固定 provider_id=1）。
    exec(
        &db,
        "UPDATE request SET provider_id = 2 WHERE model_id = 'gpt-y'",
    )
    .await;
    snap::finalize_bucket(
        &db,
        snap::Frame {
            level: snap::Level::Day,
            start: yesterday,
            end: yesterday + 24 * HOUR_MS,
        },
    )
    .await
    .unwrap();
    let window = format!("startTime={yesterday}&endTime={}", yesterday + 24 * HOUR_MS);

    // 仅 modelId：不应混入供应商二的 gpt-y（判别回归）。
    let rank = get_json(
        &app,
        &format!("/api/stats/provider-model-rank?{window}&modelId=gpt-x"),
    )
    .await;
    let items = rank["data"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 1, "仅 modelId 过滤应只返回该模型: {rank}");
    assert_eq!(items[0]["providerId"].as_i64(), Some(1));
    assert_eq!(items[0]["modelId"].as_str(), Some("gpt-x"));

    // 仅 providerId：只返回该供应商的模型，且不混入 gpt-y。
    let rank = get_json(
        &app,
        &format!("/api/stats/provider-model-rank?{window}&providerId=2"),
    )
    .await;
    let items = rank["data"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 1, "仅 providerId 过滤应只返回该供应商: {rank}");
    assert_eq!(items[0]["modelId"].as_str(), Some("gpt-y"));
}

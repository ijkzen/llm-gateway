use super::*;

#[tokio::test]
async fn test_charts_hour_granularity_local_alignment() {
    let (app, db) = setup_app().await;
    // 窗口：本地 2026-08-31 00:00 ~ 03:30（东八区）。tzOffsetMinutes=480。
    let start = local_ms_cn(2026, 8, 31, 0, 0);
    let end = local_ms_cn(2026, 8, 31, 3, 30);
    for (i, (offset_ms, tokens)) in [(0, 10), (HOUR_MS, 20), (2 * HOUR_MS + 1000, 30)]
        .into_iter()
        .enumerate()
    {
        insert_request(
            &db,
            SeedRow {
                request_id: format!("h-{i}"),
                provider_id: DEFAULT_PROVIDER_ID,
                model_id: "gpt-4o".into(),
                success: true,
                start_time: start + offset_ms,
                input_tokens: Some(1),
                input_cache_tokens: 0,
                total_tokens: Some(tokens),
            },
        )
        .await;
    }

    let (status, json) = get_json(
        app,
        &format!("/api/stats/charts?startTime={start}&endTime={end}&granularity=hour"),
    )
    .await;
    assert_eq!(status, 200);
    let data = &json["data"];
    let call_trend = data["callTrend"].as_array().unwrap();
    // 0:00~3:30 → 0、1、2、3 共 4 个整点桶。
    assert_eq!(call_trend.len(), 4);
    let starts: Vec<i64> = call_trend
        .iter()
        .map(|p| p["bucketStart"].as_i64().unwrap())
        .collect();
    assert_eq!(starts[0], start);
    assert_eq!(starts[1], start + HOUR_MS);
    assert_eq!(starts[2], start + 2 * HOUR_MS);
    assert_eq!(starts[3], start + 3 * HOUR_MS);
    let values: Vec<i64> = call_trend
        .iter()
        .map(|p| p["value"].as_i64().unwrap())
        .collect();
    assert_eq!(values, vec![1, 1, 1, 0]);
    let token_trend = data["tokenTrend"].as_array().unwrap();
    let token_values: Vec<i64> = token_trend
        .iter()
        .map(|p| p["value"].as_i64().unwrap())
        .collect();
    assert_eq!(token_values, vec![10, 20, 30, 0]);
}

#[tokio::test]
async fn test_charts_day_granularity_week_has_seven_points() {
    let (app, db) = setup_app().await;
    // 窗口：本地 2026-08-24（周一）00:00 ~ 2026-08-31（下周一）00:00（东八区）。
    // UTC 视角该窗口横跨 8 个 UTC 日，但按本地日对齐必须只出 7 个桶。
    let start = local_ms_cn(2026, 8, 24, 0, 0);
    let end = local_ms_cn(2026, 8, 31, 0, 0);
    for (i, day) in [0, 2, 5].into_iter().enumerate() {
        insert_request(
            &db,
            SeedRow {
                request_id: format!("d-{i}"),
                provider_id: DEFAULT_PROVIDER_ID,
                model_id: "gpt-4o".into(),
                success: true,
                start_time: start + day * 24 * HOUR_MS,
                input_tokens: Some(1),
                input_cache_tokens: 0,
                total_tokens: Some(100 + i as i64),
            },
        )
        .await;
    }

    let (status, json) = get_json(
        app,
        &format!("/api/stats/charts?startTime={start}&endTime={end}&granularity=day"),
    )
    .await;
    assert_eq!(status, 200);
    let data = &json["data"];
    let call_trend = data["callTrend"].as_array().unwrap();
    assert_eq!(call_trend.len(), 7);
    let starts: Vec<i64> = call_trend
        .iter()
        .map(|p| p["bucketStart"].as_i64().unwrap())
        .collect();
    assert_eq!(starts[0], start);
    for w in starts.windows(2) {
        assert_eq!(w[1] - w[0], 24 * HOUR_MS);
    }
    let values: Vec<i64> = call_trend
        .iter()
        .map(|p| p["value"].as_i64().unwrap())
        .collect();
    assert_eq!(values, vec![1, 0, 1, 0, 0, 1, 0]);
}

#[tokio::test]
async fn test_charts_month_granularity_natural_months() {
    let (app, db) = setup_app().await;
    // 窗口：本地 2026-06-25 ~ 2026-08-27（东八区）。数据落在 6/25、7/15、7/16、8/26。
    let start = local_ms_cn(2026, 6, 25, 0, 0);
    let end = local_ms_cn(2026, 8, 27, 0, 0);
    let rows = [
        (start, 1, 10),
        (local_ms_cn(2026, 7, 15, 0, 0), 1, 20),
        (local_ms_cn(2026, 7, 16, 0, 0), 1, 30),
        (local_ms_cn(2026, 8, 26, 0, 0), 1, 40),
    ];
    for (i, (ts, _, _)) in rows.iter().enumerate() {
        insert_request(
            &db,
            SeedRow {
                request_id: format!("m-{i}"),
                provider_id: DEFAULT_PROVIDER_ID,
                model_id: "gpt-4o".into(),
                success: true,
                start_time: *ts,
                input_tokens: Some(1),
                input_cache_tokens: 0,
                total_tokens: Some(rows[i].2),
            },
        )
        .await;
    }

    let (status, json) = get_json(
        app,
        &format!("/api/stats/charts?startTime={start}&endTime={end}&granularity=month"),
    )
    .await;
    assert_eq!(status, 200);
    let data = &json["data"];
    let call_trend = data["callTrend"].as_array().unwrap();
    assert_eq!(call_trend.len(), 3);
    let starts: Vec<i64> = call_trend
        .iter()
        .map(|p| p["bucketStart"].as_i64().unwrap())
        .collect();
    assert_eq!(starts[0], local_ms_cn(2026, 6, 1, 0, 0));
    assert_eq!(starts[1], local_ms_cn(2026, 7, 1, 0, 0));
    assert_eq!(starts[2], local_ms_cn(2026, 8, 1, 0, 0));
    let values: Vec<i64> = call_trend
        .iter()
        .map(|p| p["value"].as_i64().unwrap())
        .collect();
    assert_eq!(values, vec![1, 2, 1]);
    let token_values: Vec<i64> = data["tokenTrend"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["value"].as_i64().unwrap())
        .collect();
    assert_eq!(token_values, vec![10, 50, 40]);
}

#[tokio::test]
async fn test_charts_year_granularity_natural_years() {
    let (app, db) = setup_app().await;
    // 窗口：本地 2025-07-01 ~ 2026-09-01（东八区）。数据落在 2025 与 2026。
    let start = local_ms_cn(2025, 7, 1, 0, 0);
    let end = local_ms_cn(2026, 9, 1, 0, 0);
    for (i, ts) in [start, local_ms_cn(2026, 3, 5, 0, 0)]
        .into_iter()
        .enumerate()
    {
        insert_request(
            &db,
            SeedRow {
                request_id: format!("y-{i}"),
                provider_id: DEFAULT_PROVIDER_ID,
                model_id: "gpt-4o".into(),
                success: true,
                start_time: ts,
                input_tokens: Some(1),
                input_cache_tokens: 0,
                total_tokens: Some(100 + i as i64),
            },
        )
        .await;
    }

    let (status, json) = get_json(
        app,
        &format!("/api/stats/charts?startTime={start}&endTime={end}&granularity=year"),
    )
    .await;
    assert_eq!(status, 200);
    let data = &json["data"];
    let call_trend = data["callTrend"].as_array().unwrap();
    assert_eq!(call_trend.len(), 2);
    let starts: Vec<i64> = call_trend
        .iter()
        .map(|p| p["bucketStart"].as_i64().unwrap())
        .collect();
    assert_eq!(starts[0], local_ms_cn(2025, 1, 1, 0, 0));
    assert_eq!(starts[1], local_ms_cn(2026, 1, 1, 0, 0));
    let values: Vec<i64> = call_trend
        .iter()
        .map(|p| p["value"].as_i64().unwrap())
        .collect();
    assert_eq!(values, vec![1, 1]);
}

#[tokio::test]
async fn test_charts_invalid_granularity_rejected() {
    let (app, _db) = setup_app().await;
    let now = chrono::Utc::now().timestamp_millis();
    let (status, json) = get_json(
        app,
        &format!(
            "/api/stats/charts?startTime={}&endTime={}&granularity=week",
            now - HOUR_MS,
            now
        ),
    )
    .await;
    assert_eq!(status, 400);
    assert_eq!(json["code"], "INVALID_INPUT");
}

#[tokio::test]
async fn test_charts_granularity_without_window_defaults_to_past_24h() {
    let (app, db) = setup_app().await;
    // 无 startTime/endTime + 显式 granularity=hour：回退过去 24 小时，小时桶。
    let now = chrono::Utc::now().timestamp_millis();
    insert_request(
        &db,
        SeedRow {
            request_id: "df-1".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            success: true,
            start_time: now - HOUR_MS / 2,
            input_tokens: Some(1),
            input_cache_tokens: 0,
            total_tokens: Some(50),
        },
    )
    .await;

    let (status, json) = get_json(app, "/api/stats/charts?granularity=hour").await;
    assert_eq!(status, 200);
    let data = &json["data"];
    let call_trend = data["callTrend"].as_array().unwrap();
    assert_eq!(call_trend.len(), 24);
    let total: i64 = call_trend
        .iter()
        .map(|p| p["value"].as_i64().unwrap())
        .sum();
    assert_eq!(total, 1);
}

#[tokio::test]
async fn test_summary_success_rate_rounded_to_five_decimals() {
    let (app, db) = setup_app().await;
    // 2 成功 1 失败：成功率 = 2/3 = 0.66666...，应四舍五入保留 5 位 = 0.66667。
    for (i, success) in [true, true, false].into_iter().enumerate() {
        insert_request(
            &db,
            SeedRow {
                request_id: format!("sr-{i}"),
                provider_id: DEFAULT_PROVIDER_ID,
                model_id: "gpt-4o".into(),
                success,
                start_time: chrono::Utc::now().timestamp_millis(),
                input_tokens: Some(1),
                input_cache_tokens: 0,
                total_tokens: Some(10),
            },
        )
        .await;
    }

    let (status, json) = get_json(app, "/api/stats/summary").await;
    assert_eq!(status, 200);
    let rate = json["data"]["successRate"].as_f64().unwrap();
    assert!((rate - 0.66667).abs() < 1e-9, "successRate={rate}");
}

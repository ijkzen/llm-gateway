use super::*;

#[tokio::test]
async fn test_summary_empty_table_returns_zeros() {
    let (app, _db) = setup_app().await;

    let (status, json) = get_json(app, "/api/stats/summary").await;
    assert_eq!(status, 200);

    let data = &json["data"];
    assert_eq!(data["totalRequests"], 0);
    assert_eq!(data["successRate"], 0.0);
    assert_eq!(data["totalTokens"], 0);
    assert_eq!(data["cacheHitRate"], 0.0);
}

#[tokio::test]
async fn test_summary_aggregates_all_history() {
    let (app, db) = setup_app().await;
    let now = chrono::Utc::now().timestamp_millis();
    // 一条超出 24h 窗口的旧数据，summary 仍应计入。
    let old = now - 48 * HOUR_MS;

    for (i, row) in [
        SeedRow {
            request_id: "r1".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            success: true,
            start_time: now,
            input_tokens: Some(100),
            input_cache_tokens: 40,
            total_tokens: Some(150),
        },
        SeedRow {
            request_id: "r2".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            success: true,
            start_time: now,
            input_tokens: Some(100),
            input_cache_tokens: 20,
            total_tokens: Some(150),
        },
        SeedRow {
            request_id: "r3".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "claude-sonnet".into(),
            success: false,
            start_time: old,
            input_tokens: Some(200),
            input_cache_tokens: 60,
            total_tokens: Some(300),
        },
        // usage 缺失的一行：token 统计应忽略 NULL。
        SeedRow {
            request_id: "r4".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gemini-pro".into(),
            success: true,
            start_time: now,
            input_tokens: None,
            input_cache_tokens: 0,
            total_tokens: None,
        },
    ]
    .into_iter()
    .enumerate()
    {
        let _ = i;
        insert_request(&db, row).await;
    }

    let (status, json) = get_json(app, "/api/stats/summary").await;
    assert_eq!(status, 200);

    let data = &json["data"];
    assert_eq!(data["totalRequests"], 4);
    assert_eq!(data["successRate"], 0.75);
    assert_eq!(data["totalTokens"], 600);
    // 加权缓存命中率：(40+20+60) / (100+100+200) = 0.3
    assert_eq!(data["cacheHitRate"], 0.3);
}

#[tokio::test]
async fn test_charts_returns_24_zero_filled_buckets() {
    let (app, _db) = setup_app().await;

    let (status, json) = get_json(app, "/api/stats/charts").await;
    assert_eq!(status, 200);

    let data = &json["data"];
    let call_trend = data["callTrend"].as_array().unwrap();
    let token_trend = data["tokenTrend"].as_array().unwrap();
    assert_eq!(call_trend.len(), 24);
    assert_eq!(token_trend.len(), 24);
    for point in call_trend.iter().chain(token_trend.iter()) {
        assert_eq!(point["value"], 0);
    }
    // 桶按时间升序、相邻间隔一小时。
    let starts: Vec<i64> = call_trend
        .iter()
        .map(|p| p["bucketStart"].as_i64().unwrap())
        .collect();
    for w in starts.windows(2) {
        assert_eq!(w[1] - w[0], HOUR_MS);
    }
    assert_eq!(data["callByModel"].as_array().unwrap().len(), 0);
    assert_eq!(data["tokenByModel"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn test_summary_with_window_filters_to_range() {
    let (app, db) = setup_app().await;
    let now = chrono::Utc::now().timestamp_millis();
    // 窗口 [now, now+1h) 内两条、窗口外两条（更早 + 恰在终点）。
    for row in [
        SeedRow {
            request_id: "win1".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            success: true,
            start_time: now,
            input_tokens: Some(100),
            input_cache_tokens: 40,
            total_tokens: Some(150),
        },
        SeedRow {
            request_id: "win2".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            success: false,
            start_time: now + 10,
            input_tokens: Some(100),
            input_cache_tokens: 0,
            total_tokens: Some(200),
        },
        // 早于窗口起点：不计入。
        SeedRow {
            request_id: "before".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "claude-sonnet".into(),
            success: true,
            start_time: now - HOUR_MS,
            input_tokens: Some(1000),
            input_cache_tokens: 500,
            total_tokens: Some(2000),
        },
        // 恰在窗口终点（endTime）：半开区间不含 → 不计入。
        SeedRow {
            request_id: "at-end".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gemini-pro".into(),
            success: true,
            start_time: now + HOUR_MS,
            input_tokens: Some(1000),
            input_cache_tokens: 500,
            total_tokens: Some(2000),
        },
    ]
    .into_iter()
    {
        insert_request(&db, row).await;
    }

    let uri = format!(
        "/api/stats/summary?startTime={}&endTime={}",
        now,
        now + HOUR_MS
    );
    let (status, json) = get_json(app, &uri).await;
    assert_eq!(status, 200);

    let data = &json["data"];
    assert_eq!(data["totalRequests"], 2);
    assert_eq!(data["successRate"], 0.5);
    assert_eq!(data["totalTokens"], 350);
    // 加权缓存命中率：(40+0) / (100+100) = 0.2
    assert_eq!(data["cacheHitRate"], 0.2);
}

#[tokio::test]
async fn test_summary_with_invalid_window_returns_400() {
    let (app, _db) = setup_app().await;

    // 只传一端：参数不完整。
    let (status, _json) = get_json(app.clone(), "/api/stats/summary?startTime=1000").await;
    assert_eq!(status, 400);

    // 终点不晚于起点：窗口非法。
    let (status, _json) = get_json(app, "/api/stats/summary?startTime=2000&endTime=2000").await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn test_charts_aggregates_by_hour_and_model() {
    let (app, db) = setup_app().await;
    let now = chrono::Utc::now().timestamp_millis();
    let current_bucket_start = (now / HOUR_MS) * HOUR_MS;
    let prev_bucket_start = current_bucket_start - HOUR_MS;
    let outside_window = current_bucket_start - 24 * HOUR_MS;

    let rows = vec![
        // 当前小时：gpt-4o 两笔（含一笔失败，仍计入调用数）。
        SeedRow {
            request_id: "c1".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            success: true,
            start_time: current_bucket_start + 1,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(100),
        },
        SeedRow {
            request_id: "c2".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            success: false,
            start_time: current_bucket_start + 2,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: None,
        },
        // 上一小时：gpt-4o 一笔 + claude 一笔。
        SeedRow {
            request_id: "c3".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            success: true,
            start_time: prev_bucket_start + 1,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(50),
        },
        SeedRow {
            request_id: "c4".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "claude-sonnet".into(),
            success: true,
            start_time: prev_bucket_start + 2,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(30),
        },
        // 窗口外：不应出现在任何图表数据中。
        SeedRow {
            request_id: "c5".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "old-model".into(),
            success: true,
            start_time: outside_window,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(999),
        },
    ];
    for row in rows {
        insert_request(&db, row).await;
    }

    let (status, json) = get_json(app, "/api/stats/charts").await;
    assert_eq!(status, 200);
    let data = &json["data"];

    let call_trend = data["callTrend"].as_array().unwrap();
    assert_eq!(call_trend.len(), 24);
    let current = &call_trend[23];
    let prev = &call_trend[22];
    assert_eq!(current["bucketStart"], current_bucket_start);
    assert_eq!(current["value"], 2);
    assert_eq!(prev["value"], 2);
    assert!(call_trend[..22].iter().all(|p| p["value"] == 0));

    let token_trend = data["tokenTrend"].as_array().unwrap();
    assert_eq!(token_trend[23]["value"], 100);
    assert_eq!(token_trend[22]["value"], 80);

    let call_by_model = data["callByModel"].as_array().unwrap();
    assert_eq!(call_by_model.len(), 2);
    let gpt = call_by_model
        .iter()
        .find(|m| m["modelId"] == "gpt-4o")
        .unwrap();
    assert_eq!(gpt["value"], 3);
    assert_eq!(gpt["providerName"], DEFAULT_PROVIDER_NAME);
    let claude = call_by_model
        .iter()
        .find(|m| m["modelId"] == "claude-sonnet")
        .unwrap();
    assert_eq!(claude["value"], 1);
    assert_eq!(claude["providerName"], DEFAULT_PROVIDER_NAME);

    let token_by_model = data["tokenByModel"].as_array().unwrap();
    let gpt_tokens = token_by_model
        .iter()
        .find(|m| m["modelId"] == "gpt-4o")
        .unwrap();
    assert_eq!(gpt_tokens["value"], 150);
    assert_eq!(gpt_tokens["providerName"], DEFAULT_PROVIDER_NAME);
}

#[tokio::test]
async fn test_stats_requires_auth() {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    scheduler.start().await.unwrap();
    // 未注入凭证的 app：/api/stats 应被会话中间件拦截。
    let app = common::build_app(db, scheduler, log_tx);

    let request: Request<Body> = Request::builder()
        .method("GET")
        .uri("/api/stats/summary")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), 401);
}

#[tokio::test]
async fn test_charts_splits_same_model_across_providers() {
    let (app, db) = setup_app().await;
    let now = chrono::Utc::now().timestamp_millis();
    // 第二个供应商注册同名模型：分布应按 (provider, model) 拆成两行。
    seed_provider(&db, 2, "第二供应商").await;

    for (i, row) in [
        SeedRow {
            request_id: "p1".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            success: true,
            start_time: now,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(50),
        },
        SeedRow {
            request_id: "p2".into(),
            provider_id: 2,
            model_id: "gpt-4o".into(),
            success: true,
            start_time: now,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(30),
        },
    ]
    .into_iter()
    .enumerate()
    {
        let _ = i;
        insert_request(&db, row).await;
    }

    let (status, json) = get_json(app, "/api/stats/charts").await;
    assert_eq!(status, 200);
    let data = &json["data"];

    let call_by_model = data["callByModel"].as_array().unwrap();
    assert_eq!(call_by_model.len(), 2);
    let first = call_by_model
        .iter()
        .find(|m| m["providerName"] == DEFAULT_PROVIDER_NAME && m["modelId"] == "gpt-4o")
        .unwrap();
    assert_eq!(first["value"], 1);
    let second = call_by_model
        .iter()
        .find(|m| m["providerName"] == "第二供应商" && m["modelId"] == "gpt-4o")
        .unwrap();
    assert_eq!(second["value"], 1);

    let token_by_model = data["tokenByModel"].as_array().unwrap();
    assert_eq!(token_by_model.len(), 2);
    let first_tokens = token_by_model
        .iter()
        .find(|m| m["providerName"] == DEFAULT_PROVIDER_NAME && m["modelId"] == "gpt-4o")
        .unwrap();
    assert_eq!(first_tokens["value"], 50);
    let second_tokens = token_by_model
        .iter()
        .find(|m| m["providerName"] == "第二供应商" && m["modelId"] == "gpt-4o")
        .unwrap();
    assert_eq!(second_tokens["value"], 30);
}

#[tokio::test]
async fn test_charts_provider_deleted_falls_back_to_empty_name() {
    let (app, db) = setup_app().await;
    let now = chrono::Utc::now().timestamp_millis();
    // 供应商已删除（provider 表无该行）：providerName 应为空串，仍按 model_id 聚合。
    for (i, row) in [
        SeedRow {
            request_id: "d1".into(),
            provider_id: 99,
            model_id: "ghost-model".into(),
            success: true,
            start_time: now,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(10),
        },
        SeedRow {
            request_id: "d2".into(),
            provider_id: 99,
            model_id: "ghost-model".into(),
            success: true,
            start_time: now,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(20),
        },
    ]
    .into_iter()
    .enumerate()
    {
        let _ = i;
        insert_request(&db, row).await;
    }

    let (status, json) = get_json(app, "/api/stats/charts").await;
    assert_eq!(status, 200);
    let data = &json["data"];

    let call_by_model = data["callByModel"].as_array().unwrap();
    assert_eq!(call_by_model.len(), 1);
    let ghost = &call_by_model[0];
    assert_eq!(ghost["providerName"], "");
    assert_eq!(ghost["modelId"], "ghost-model");
    assert_eq!(ghost["value"], 2);
}

#[tokio::test]
async fn test_charts_with_window_and_provider_filter() {
    let (app, db) = setup_app().await;
    // 固定窗口 [T0, T0+3h)，两个供应商各一笔；带 providerId 应只返回该供应商。
    let t0 = (1_700_000_000_000i64 / HOUR_MS) * HOUR_MS;
    for row in [
        SeedRow {
            request_id: "w1".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            success: true,
            start_time: t0 + 1,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(100),
        },
        // 供应商 2（另一个 provider）的请求，不应出现在 providerId=1 过滤结果中。
        SeedRow {
            request_id: "w2".into(),
            provider_id: 2,
            model_id: "claude-sonnet".into(),
            success: true,
            start_time: t0 + 1,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(200),
        },
        // 窗口外请求，不应出现。
        SeedRow {
            request_id: "w3".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            success: true,
            start_time: t0 - 1,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(999),
        },
    ] {
        insert_request(&db, row).await;
    }

    // 带 startTime/endTime + providerId 过滤。
    let (status, json) = get_json(
        app.clone(),
        &format!(
            "/api/stats/charts?startTime={t0}&endTime={}&providerId={}",
            t0 + 3 * HOUR_MS,
            DEFAULT_PROVIDER_ID
        ),
    )
    .await;
    assert_eq!(status, 200);
    let data = &json["data"];
    let call_trend = data["callTrend"].as_array().unwrap();
    // 3 小时窗口 → 3 个桶（小时粒度）。
    assert_eq!(call_trend.len(), 3);
    let total_calls: i64 = call_trend
        .iter()
        .map(|p| p["value"].as_i64().unwrap())
        .sum();
    assert_eq!(total_calls, 1); // 只有 w1
    let call_by_model = data["callByModel"].as_array().unwrap();
    assert_eq!(call_by_model.len(), 1);
    assert_eq!(call_by_model[0]["modelId"], "gpt-4o");
    assert_eq!(call_by_model[0]["value"], 1);

    // 不带 providerId：两个供应商都出现。
    let (status, json) = get_json(
        app,
        &format!(
            "/api/stats/charts?startTime={t0}&endTime={}",
            t0 + 3 * HOUR_MS
        ),
    )
    .await;
    assert_eq!(status, 200);
    let data = &json["data"];
    let call_by_model = data["callByModel"].as_array().unwrap();
    assert_eq!(call_by_model.len(), 2);
}

#[tokio::test]
async fn test_charts_day_granularity_for_long_window() {
    let (app, db) = setup_app().await;
    // 5 天窗口 → 天桶粒度（5 个桶），验证 >48h 用天桶。
    // t0 对齐设置表时区（缺省 Asia/Shanghai +480）的本地午夜，桶边界整齐。
    let t0 = ((1_700_000_000_000i64 / HOUR_MS / 24) * 24 * HOUR_MS) - 8 * HOUR_MS;
    let day_ms = 24 * HOUR_MS;
    for row in [
        SeedRow {
            request_id: "d1".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            success: true,
            start_time: t0 + 1,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(100),
        },
        SeedRow {
            request_id: "d2".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            success: true,
            start_time: t0 + 2 * day_ms + 1,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(200),
        },
    ] {
        insert_request(&db, row).await;
    }

    let (status, json) = get_json(
        app,
        &format!(
            "/api/stats/charts?startTime={t0}&endTime={}",
            t0 + 5 * day_ms
        ),
    )
    .await;
    assert_eq!(status, 200);
    let data = &json["data"];
    let call_trend = data["callTrend"].as_array().unwrap();
    assert_eq!(call_trend.len(), 5);
    // 相邻桶间隔一天。
    let starts: Vec<i64> = call_trend
        .iter()
        .map(|p| p["bucketStart"].as_i64().unwrap())
        .collect();
    for w in starts.windows(2) {
        assert_eq!(w[1] - w[0], day_ms);
    }
    let values: Vec<i64> = call_trend
        .iter()
        .map(|p| p["value"].as_i64().unwrap())
        .collect();
    assert_eq!(values, vec![1, 0, 1, 0, 0]);
}

#[tokio::test]
async fn test_charts_virtual_model_filter() {
    let (app, db) = setup_app().await;
    // 固定窗口内两个虚拟模型各一笔；带 virtualModelId 只返回该虚拟模型。
    let t0 = (1_700_000_000_000i64 / HOUR_MS) * HOUR_MS;
    // 直接插 request（virtual_model_id=2 的行需要绕过 insert_request 的硬编码 1）。
    let end_time = t0 + 500;
    for (rid, vm_id, model) in [("vmf1", 1, "gpt-4o"), ("vmf2", 2, "claude-sonnet")] {
        request_entity::ActiveModel {
            request_id: Set(rid.to_string()),
            virtual_model_id: Set(vm_id),
            provider_id: Set(DEFAULT_PROVIDER_ID),
            model_id: Set(model.to_string()),
            stream: Set(false),
            ttft: Set(None),
            input_tokens: Set(Some(10)),
            input_cache_tokens: Set(0),
            input_cache_rate: Set(0.0),
            output_tokens: Set(None),
            output_tokens_time: Set(None),
            tps: Set(0.0),
            start_time: Set(t0 + 1),
            end_time: Set(end_time),
            request_time: Set(500),
            success: Set(true),
            fail_reason: Set(None),
            total_tokens: Set(Some(100)),
            api_key_name: Set("itest-key".to_string()),
        }
        .insert(&db)
        .await
        .unwrap();
    }

    // 带 virtualModelId=1：只返回 gpt-4o。
    let (status, json) = get_json(
        app.clone(),
        &format!(
            "/api/stats/charts?startTime={t0}&endTime={}&virtualModelId=1",
            t0 + 2 * HOUR_MS
        ),
    )
    .await;
    assert_eq!(status, 200);
    let data = &json["data"];
    let call_by_model = data["callByModel"].as_array().unwrap();
    assert_eq!(call_by_model.len(), 1);
    assert_eq!(call_by_model[0]["modelId"], "gpt-4o");

    // 不带过滤：两个虚拟模型的模型都出现。
    let (status, json) = get_json(
        app,
        &format!(
            "/api/stats/charts?startTime={t0}&endTime={}",
            t0 + 2 * HOUR_MS
        ),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(json["data"]["callByModel"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn test_charts_provider_and_model_filter() {
    let (app, db) = setup_app().await;
    // 固定窗口内：供应商 A 两个模型、B 一个模型；带 providerId+modelId 只返回该模型。
    let t0 = (1_700_000_000_000i64 / HOUR_MS) * HOUR_MS;
    let end_time = t0 + 500;
    for (rid, pid, model) in [
        ("pmf1", DEFAULT_PROVIDER_ID, "gpt-4o"),
        ("pmf2", DEFAULT_PROVIDER_ID, "deepseek-v3"),
        ("pmf3", 2, "claude-sonnet"),
    ] {
        request_entity::ActiveModel {
            request_id: Set(rid.to_string()),
            virtual_model_id: Set(1),
            provider_id: Set(pid),
            model_id: Set(model.to_string()),
            stream: Set(false),
            ttft: Set(None),
            input_tokens: Set(Some(10)),
            input_cache_tokens: Set(0),
            input_cache_rate: Set(0.0),
            output_tokens: Set(None),
            output_tokens_time: Set(None),
            tps: Set(0.0),
            start_time: Set(t0 + 1),
            end_time: Set(end_time),
            request_time: Set(500),
            success: Set(true),
            fail_reason: Set(None),
            total_tokens: Set(Some(100)),
            api_key_name: Set("itest-key".to_string()),
        }
        .insert(&db)
        .await
        .unwrap();
    }

    // providerId + modelId 组合过滤：只返回 gpt-4o。
    let (status, json) = get_json(
        app,
        &format!(
            "/api/stats/charts?startTime={t0}&endTime={}&providerId={}&modelId=gpt-4o",
            t0 + 2 * HOUR_MS,
            DEFAULT_PROVIDER_ID
        ),
    )
    .await;
    assert_eq!(status, 200);
    let data = &json["data"];
    let call_by_model = data["callByModel"].as_array().unwrap();
    assert_eq!(call_by_model.len(), 1);
    assert_eq!(call_by_model[0]["modelId"], "gpt-4o");
    let total_calls: i64 = data["callTrend"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["value"].as_i64().unwrap())
        .sum();
    assert_eq!(total_calls, 1);
}

/// provider-metrics：供应商级 6 指标聚合（成功行 + 窗口过滤）。
#[tokio::test]
async fn test_provider_metrics_aggregates_success_rows_in_window() {
    let (app, db) = setup_app().await;
    let now = chrono::Utc::now().timestamp_millis();
    let t0 = now - 24 * HOUR_MS;

    // 成功 2 条 + 失败 1 条（失败不计入指标）。
    for row in [
        SeedRow {
            request_id: "pm-ok1".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            success: true,
            start_time: t0,
            input_tokens: Some(100),
            input_cache_tokens: 40,
            total_tokens: Some(150),
        },
        SeedRow {
            request_id: "pm-ok2".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "claude-3".into(),
            success: true,
            start_time: t0 + HOUR_MS,
            input_tokens: Some(200),
            input_cache_tokens: 0,
            total_tokens: Some(300),
        },
        SeedRow {
            request_id: "pm-fail".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            success: false,
            start_time: t0 + 2 * HOUR_MS,
            input_tokens: Some(999),
            input_cache_tokens: 0,
            total_tokens: Some(999),
        },
    ] {
        insert_request(&db, row).await;
    }
    // 窗口外的成功行不计入。
    insert_request(
        &db,
        SeedRow {
            request_id: "pm-outside".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            success: true,
            start_time: now,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(20),
        },
    )
    .await;

    let (status, json) = get_json(
        app,
        &format!(
            "/api/stats/provider-metrics?providerId={}&startTime={t0}&endTime={}",
            DEFAULT_PROVIDER_ID, now
        ),
    )
    .await;
    assert_eq!(status, 200);
    let data = &json["data"];
    assert_eq!(data["providerId"], DEFAULT_PROVIDER_ID);
    assert_eq!(data["providerName"], DEFAULT_PROVIDER_NAME);
    assert_eq!(data["requestCount"], 2);
    assert_eq!(data["totalTokens"], 450);
    // 缓存命中率 = 40 / (100+200) = 0.13333…，后端 ROUND(...,5) 保留 5 位。
    let cache_rate = data["cacheHitRate"].as_f64().unwrap();
    assert!(
        (cache_rate - 0.13333).abs() < 1e-9,
        "cacheHitRate={cache_rate}"
    );
}

/// provider-metrics：缺参 / 窗口非法返回 400。
#[tokio::test]
async fn test_provider_metrics_validation() {
    let (app, _db) = setup_app().await;

    let (status, _) = get_json(app.clone(), "/api/stats/provider-metrics").await;
    assert_eq!(status, 400);
    let (status, _) = get_json(
        app.clone(),
        "/api/stats/provider-metrics?providerId=1&startTime=100&endTime=100",
    )
    .await;
    assert_eq!(status, 400);
}

/// virtual-model-metrics：按虚拟模型过滤聚合。
#[tokio::test]
async fn test_virtual_model_metrics_aggregates() {
    let (app, db) = setup_app().await;
    let now = chrono::Utc::now().timestamp_millis();
    let t0 = now - HOUR_MS;

    for row in [
        SeedRow {
            request_id: "vm-m1".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            success: true,
            start_time: t0,
            input_tokens: Some(100),
            input_cache_tokens: 50,
            total_tokens: Some(200),
        },
        SeedRow {
            request_id: "vm-m2".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            success: true,
            start_time: t0 + 100,
            input_tokens: Some(100),
            input_cache_tokens: 0,
            total_tokens: Some(200),
        },
    ] {
        insert_request(&db, row).await;
    }

    let (status, json) = get_json(
        app,
        &format!("/api/stats/virtual-model-metrics?virtualModelId=1&startTime={t0}&endTime={now}"),
    )
    .await;
    assert_eq!(status, 200);
    let data = &json["data"];
    assert_eq!(data["virtualModelId"], 1);
    assert_eq!(data["requestCount"], 2);
    assert_eq!(data["totalTokens"], 400);
    // 缓存命中率 = 50 / 200 = 0.25。
    let cache_rate = data["cacheHitRate"].as_f64().unwrap();
    assert!(
        (cache_rate - 0.25).abs() < 1e-9,
        "cacheHitRate={cache_rate}"
    );
}

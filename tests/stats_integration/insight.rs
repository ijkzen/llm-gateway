use super::*;

#[tokio::test]
async fn test_insight_empty_table_returns_zeros() {
    let (app, _db) = setup_app().await;

    let (status, json) = get_json(app, "/api/stats/insight").await;
    assert_eq!(status, 200);
    let data = &json["data"];

    // 缺省窗口=过去 24 小时（小时桶）→ 24 个桶。
    assert_eq!(data["callTrend"].as_array().unwrap().len(), 24);
    assert_eq!(data["failureTrend"].as_array().unwrap().len(), 24);
    assert_eq!(data["failureRateTrend"].as_array().unwrap().len(), 24);
    assert_eq!(data["inputTokenTrend"].as_array().unwrap().len(), 24);
    assert_eq!(data["outputTokenTrend"].as_array().unwrap().len(), 24);
    assert_eq!(data["cacheHitRateTrend"].as_array().unwrap().len(), 24);
    assert_eq!(
        data["outputTokensPerSecTrend"].as_array().unwrap().len(),
        24
    );
    assert_eq!(data["streamRatioTrend"].as_array().unwrap().len(), 24);
    assert_eq!(data["ttftPercentiles"].as_array().unwrap().len(), 24);
    assert_eq!(data["latencyPercentiles"].as_array().unwrap().len(), 24);
    assert!(data["failureReasons"].as_array().unwrap().is_empty());
    assert!(data["apiKeyRank"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn test_insight_failure_diagnostics() {
    let (app, db) = setup_app().await;
    let now = chrono::Utc::now().timestamp_millis();
    let bucket_start = (now / HOUR_MS) * HOUR_MS;

    // 同桶：2 成功（无原因）+ 1 失败（429 限流）。
    for row in [
        FullRow {
            request_id: "i-f1".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            stream: false,
            ttft: None,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            output_tokens: Some(20),
            output_tokens_time: None,
            request_time: 500,
            success: true,
            fail_reason: None,
            total_tokens: Some(30),
            start_time: bucket_start + 1,
        },
        FullRow {
            request_id: "i-f2".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            stream: false,
            ttft: None,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            output_tokens: Some(20),
            output_tokens_time: None,
            request_time: 600,
            success: true,
            fail_reason: None,
            total_tokens: Some(30),
            start_time: bucket_start + 2,
        },
        FullRow {
            request_id: "i-f3".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            stream: false,
            ttft: None,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            output_tokens: None,
            output_tokens_time: None,
            request_time: 100,
            success: false,
            fail_reason: Some("上游 429 限流".to_string()),
            total_tokens: None,
            start_time: bucket_start + 3,
        },
    ] {
        insert_full(&db, row).await;
    }

    let (status, json) = get_json(
        app,
        &format!(
            "/api/stats/insight?startTime={}&endTime={}",
            bucket_start,
            bucket_start + HOUR_MS
        ),
    )
    .await;
    assert_eq!(status, 200);
    let data = &json["data"];

    let failure_trend = data["failureTrend"].as_array().unwrap();
    assert_eq!(failure_trend.len(), 1);
    assert_eq!(failure_trend[0]["value"], 1);

    // callTrend = 全部调用（2 成功 + 1 失败）= 3；堆叠面积基准应含成功与失败。
    let call_trend = data["callTrend"].as_array().unwrap();
    assert_eq!(call_trend.len(), 1);
    assert_eq!(call_trend[0]["value"], 3);

    let failure_rate_trend = data["failureRateTrend"].as_array().unwrap();
    assert_eq!(failure_rate_trend.len(), 1);
    let rate = failure_rate_trend[0]["value"].as_f64().unwrap();
    assert!((rate - 1.0 / 3.0).abs() < 1e-9, "failureRate={rate}");

    let failure_reasons = data["failureReasons"].as_array().unwrap();
    assert_eq!(failure_reasons.len(), 1);
    assert_eq!(failure_reasons[0]["reason"], "上游 429 限流");
    assert_eq!(failure_reasons[0]["count"], 1);

    // 2 成功 1 失败 → apiKeyRank 按调用数（全量）聚合 = 3。
    let api_key_rank = data["apiKeyRank"].as_array().unwrap();
    assert_eq!(api_key_rank.len(), 1);
    assert_eq!(api_key_rank[0]["apiKeyName"], "itest-key");
    assert_eq!(api_key_rank[0]["value"], 3);
}

#[tokio::test]
async fn test_insight_failure_reason_empty_maps_to_no_reason() {
    let (app, db) = setup_app().await;
    let now = chrono::Utc::now().timestamp_millis();
    let t0 = (now / HOUR_MS) * HOUR_MS;

    // 一条失败且 fail_reason 为空串，一条失败且 NULL：都应归「空串原因」（前端显示无原因）。
    for (rid, reason) in [("nr1", Some("".to_string())), ("nr2", None)] {
        insert_full(
            &db,
            FullRow {
                request_id: rid.into(),
                provider_id: DEFAULT_PROVIDER_ID,
                model_id: "gpt-4o".into(),
                stream: false,
                ttft: None,
                input_tokens: Some(1),
                input_cache_tokens: 0,
                output_tokens: None,
                output_tokens_time: None,
                request_time: 100,
                success: false,
                fail_reason: reason,
                total_tokens: None,
                start_time: t0 + 1,
            },
        )
        .await;
    }

    let (status, json) = get_json(
        app,
        &format!(
            "/api/stats/insight?startTime={}&endTime={}",
            t0,
            t0 + HOUR_MS
        ),
    )
    .await;
    assert_eq!(status, 200);
    let failure_reasons = json["data"]["failureReasons"].as_array().unwrap();
    assert_eq!(failure_reasons.len(), 1);
    assert_eq!(failure_reasons[0]["reason"], "");
    assert_eq!(failure_reasons[0]["count"], 2);
}

#[tokio::test]
async fn test_insight_latency_percentiles() {
    let (app, db) = setup_app().await;
    let now = chrono::Utc::now().timestamp_millis();
    let t0 = (now / HOUR_MS) * HOUR_MS;

    // 同一小时内 3 个成功流式请求：ttft=[100, 200, 300]，request_time=[500, 600, 700]。
    for (i, (ttft, rt)) in [(100i64, 500i64), (200, 600), (300, 700)]
        .into_iter()
        .enumerate()
    {
        insert_full(
            &db,
            FullRow {
                request_id: format!("lp-{i}"),
                provider_id: DEFAULT_PROVIDER_ID,
                model_id: "gpt-4o".into(),
                stream: true,
                ttft: Some(ttft),
                input_tokens: Some(10),
                input_cache_tokens: 0,
                output_tokens: Some(20),
                output_tokens_time: Some(400),
                request_time: rt,
                success: true,
                fail_reason: None,
                total_tokens: Some(30),
                start_time: t0 + i as i64 + 1,
            },
        )
        .await;
    }

    let (status, json) = get_json(
        app,
        &format!(
            "/api/stats/insight?startTime={}&endTime={}",
            t0,
            t0 + HOUR_MS
        ),
    )
    .await;
    assert_eq!(status, 200);
    let data = &json["data"];

    let ttft_p = data["ttftPercentiles"].as_array().unwrap();
    assert_eq!(ttft_p.len(), 1);
    // 线性插值：n=3 → p50=200, p90=280, p95=290, p99=298。
    assert_eq!(ttft_p[0]["p50"], 200.0);
    assert_eq!(ttft_p[0]["p90"], 280.0);
    assert_eq!(ttft_p[0]["p95"], 290.0);
    assert_eq!(ttft_p[0]["p99"], 298.0);

    let latency_p = data["latencyPercentiles"].as_array().unwrap();
    assert_eq!(latency_p.len(), 1);
    assert_eq!(latency_p[0]["p50"], 600.0);
    assert_eq!(latency_p[0]["p90"], 680.0);
    assert_eq!(latency_p[0]["p95"], 690.0);
    assert_eq!(latency_p[0]["p99"], 698.0);
}

#[tokio::test]
async fn test_insight_token_structure_and_throughput() {
    let (app, db) = setup_app().await;
    let now = chrono::Utc::now().timestamp_millis();
    let t0 = (now / HOUR_MS) * HOUR_MS;

    // 同一小时内 2 个成功请求：输入 [100, 100]、输出 [200, 300]、缓存 [40, 0]、
    // 输出耗时 [2000ms, 3000ms] → 每秒输出 token = 200/2 + 300/3 = 200。
    for (i, (input, output, cache, out_ms)) in
        [(100i64, 200i64, 40i64, 2000i64), (100, 300, 0, 3000)]
            .into_iter()
            .enumerate()
    {
        insert_full(
            &db,
            FullRow {
                request_id: format!("ts-{i}"),
                provider_id: DEFAULT_PROVIDER_ID,
                model_id: "gpt-4o".into(),
                stream: true,
                ttft: Some(100),
                input_tokens: Some(input),
                input_cache_tokens: cache,
                output_tokens: Some(output),
                output_tokens_time: Some(out_ms),
                request_time: 1000,
                success: true,
                fail_reason: None,
                total_tokens: Some(input + output),
                start_time: t0 + i as i64 + 1,
            },
        )
        .await;
    }

    let (status, json) = get_json(
        app,
        &format!(
            "/api/stats/insight?startTime={}&endTime={}",
            t0,
            t0 + HOUR_MS
        ),
    )
    .await;
    assert_eq!(status, 200);
    let data = &json["data"];

    let input_trend = data["inputTokenTrend"].as_array().unwrap();
    assert_eq!(input_trend[0]["value"], 200);
    let output_trend = data["outputTokenTrend"].as_array().unwrap();
    assert_eq!(output_trend[0]["value"], 500);

    let cache_rate = data["cacheHitRateTrend"].as_array().unwrap();
    assert_eq!(cache_rate[0]["value"].as_f64().unwrap(), 0.2);

    let out_per_sec = data["outputTokensPerSecTrend"].as_array().unwrap();
    let tps_val = out_per_sec[0]["value"].as_f64().unwrap();
    assert!((tps_val - 200.0).abs() < 1e-9, "outputPerSec={tps_val}");

    // 流式占比：2 个都流式 → 1.0。
    let stream_ratio = data["streamRatioTrend"].as_array().unwrap();
    assert_eq!(stream_ratio[0]["value"].as_f64().unwrap(), 1.0);

    // 小时桶 → RPM/TPM 有值（窗口 1 小时 → RPM=2；total_tokens=700 → TPM=700/60≈11.67 每分钟）。
    let rpm = data["rpmTrend"].as_array().unwrap();
    assert_eq!(rpm[0]["value"], 2);
    let tpm = data["tpmTrend"].as_array().unwrap();
    let tpm_val = tpm[0]["value"].as_f64().unwrap();
    assert!((tpm_val - 700.0 / 60.0).abs() < 1e-9, "tpm={tpm_val}");
}

#[tokio::test]
async fn test_insight_filters_by_provider_vm_model() {
    let (app, db) = setup_app().await;
    let now = chrono::Utc::now().timestamp_millis();
    let t0 = (now / HOUR_MS) * HOUR_MS;

    // 供应商 1：gpt-4o（1 次）；供应商 2：claude-sonnet（1 次）。
    for (rid, pid, model) in [
        ("flt1", DEFAULT_PROVIDER_ID, "gpt-4o"),
        ("flt2", 2, "claude-sonnet"),
    ] {
        insert_full(
            &db,
            FullRow {
                request_id: rid.into(),
                provider_id: pid,
                model_id: model.into(),
                stream: true,
                ttft: Some(100),
                input_tokens: Some(10),
                input_cache_tokens: 0,
                output_tokens: Some(20),
                output_tokens_time: Some(1000),
                request_time: 500,
                success: true,
                fail_reason: None,
                total_tokens: Some(30),
                start_time: t0 + 1,
            },
        )
        .await;
    }

    // providerId 过滤 → 只留供应商 1。
    let (status, json) = get_json(
        app.clone(),
        &format!(
            "/api/stats/insight?startTime={}&endTime={}&providerId={}",
            t0,
            t0 + HOUR_MS,
            DEFAULT_PROVIDER_ID
        ),
    )
    .await;
    assert_eq!(status, 200);
    let data = &json["data"];
    assert_eq!(data["apiKeyRank"].as_array().unwrap()[0]["value"], 1);

    // virtualModelId=1 + modelId 组合：仍只留 1 条。
    let (status, json) = get_json(
        app,
        &format!(
            "/api/stats/insight?startTime={}&endTime={}&virtualModelId=1&modelId=gpt-4o",
            t0,
            t0 + HOUR_MS
        ),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        json["data"]["apiKeyRank"].as_array().unwrap()[0]["value"],
        1
    );
}

#[tokio::test]
async fn test_insight_invalid_window_rejected() {
    // end<=start 回退缺省窗口（与 charts 一致，200）；非法 granularity 才是 400。
    let (app, _db) = setup_app().await;
    let (status, json) = get_json(app, "/api/stats/insight?startTime=100&endTime=100").await;
    assert_eq!(status, 200);
    assert_eq!(json["data"]["failureTrend"].as_array().unwrap().len(), 24);

    let (app, _db) = setup_app().await;
    let now = chrono::Utc::now().timestamp_millis();
    let (status, _) = get_json(
        app,
        &format!(
            "/api/stats/insight?startTime={}&endTime={}&granularity=week",
            now - HOUR_MS,
            now
        ),
    )
    .await;
    assert_eq!(status, 400);
}

/// 月/年粒度：比率在自然月归并后按桶起点重算，空月补零且比率为 0。
#[tokio::test]
async fn test_insight_month_granularity_recomputes_ratios() {
    let (app, db) = setup_app().await;
    // 窗口：2026-06-25 ~ 2026-08-27（东八区）。6 月 1 败 1 成、7 月无数据、8 月 1 成。
    let start = local_ms_cn(2026, 6, 25, 0, 0);
    let end = local_ms_cn(2026, 8, 27, 0, 0);

    for row in [
        FullRow {
            request_id: "m-fail-1".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            stream: true,
            ttft: Some(100),
            input_tokens: Some(100),
            input_cache_tokens: 0,
            output_tokens: Some(200),
            output_tokens_time: Some(2000),
            request_time: 500,
            success: false,
            fail_reason: Some("上游 500".to_string()),
            total_tokens: None,
            start_time: local_ms_cn(2026, 6, 25, 10, 0),
        },
        FullRow {
            request_id: "m-ok-1".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            stream: true,
            ttft: Some(100),
            input_tokens: Some(100),
            input_cache_tokens: 0,
            output_tokens: Some(200),
            output_tokens_time: Some(2000),
            request_time: 500,
            success: true,
            fail_reason: None,
            total_tokens: Some(300),
            start_time: local_ms_cn(2026, 6, 26, 10, 0),
        },
        FullRow {
            request_id: "m-ok-2".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            stream: true,
            ttft: Some(100),
            input_tokens: Some(100),
            input_cache_tokens: 0,
            output_tokens: Some(200),
            output_tokens_time: Some(2000),
            request_time: 500,
            success: true,
            fail_reason: None,
            total_tokens: Some(300),
            start_time: local_ms_cn(2026, 8, 26, 10, 0),
        },
    ] {
        insert_full(&db, row).await;
    }

    let (status, json) = get_json(
        app,
        &format!("/api/stats/insight?startTime={start}&endTime={end}&granularity=month"),
    )
    .await;
    assert_eq!(status, 200);
    let data = &json["data"];

    // 三个月桶：6/7/8 月，起点为各月 1 日。
    let failure_trend = data["failureTrend"].as_array().unwrap();
    assert_eq!(failure_trend.len(), 3);
    assert_eq!(
        failure_trend[0]["bucketStart"],
        local_ms_cn(2026, 6, 1, 0, 0)
    );
    assert_eq!(
        failure_trend[1]["bucketStart"],
        local_ms_cn(2026, 7, 1, 0, 0)
    );
    assert_eq!(
        failure_trend[2]["bucketStart"],
        local_ms_cn(2026, 8, 1, 0, 0)
    );
    assert_eq!(failure_trend[0]["value"], 1);
    assert_eq!(failure_trend[1]["value"], 0);
    assert_eq!(failure_trend[2]["value"], 0);

    // 失败率：6 月 2 条（1 败 1 成）= 1/2，7/8 月 = 0（7 月无流量也为 0，不除零）。
    let failure_rate = data["failureRateTrend"].as_array().unwrap();
    assert_eq!(failure_rate.len(), 3);
    let june_rate = failure_rate[0]["value"].as_f64().unwrap();
    assert!(
        (june_rate - 0.5).abs() < 1e-9,
        "june failureRate={june_rate}"
    );
    assert_eq!(failure_rate[1]["value"], 0.0);
    assert_eq!(failure_rate[2]["value"], 0.0);

    // 失败原因只来自失败请求（6 月 1 条），跨月仍按窗口全量聚合。
    let reasons = data["failureReasons"].as_array().unwrap();
    assert_eq!(reasons.len(), 1);
    assert_eq!(reasons[0]["reason"], "上游 500");

    // 分位在月粒度退化为空数组。
    assert_eq!(data["ttftPercentiles"].as_array().unwrap().len(), 0);
    assert_eq!(data["latencyPercentiles"].as_array().unwrap().len(), 0);

    // RPM/TPM 仅小时粒度，月粒度返回空。
    assert_eq!(data["rpmTrend"].as_array().unwrap().len(), 0);
    assert_eq!(data["tpmTrend"].as_array().unwrap().len(), 0);

    // 流式占比：6 月全流式 = 1.0，7 月 0。
    let stream_ratio = data["streamRatioTrend"].as_array().unwrap();
    assert_eq!(stream_ratio[0]["value"].as_f64().unwrap(), 1.0);
    assert_eq!(stream_ratio[1]["value"].as_f64().unwrap(), 0.0);
}

#[tokio::test]
async fn test_insight_year_granularity_trends_nonzero() {
    // 回归：year 粒度曾 fold 存 (y, 真实月) 而补零读取查 (y, 0)，趋势恒全零。
    let (app, db) = setup_app().await;
    // 窗口：2025-07-01 ~ 2026-09-01（东八区）→ 2025 / 2026 两个年桶。
    let start = local_ms_cn(2025, 7, 1, 0, 0);
    let end = local_ms_cn(2026, 9, 1, 0, 0);

    for row in [
        FullRow {
            request_id: "y-ok-1".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            stream: false,
            ttft: Some(100),
            input_tokens: Some(100),
            input_cache_tokens: 0,
            output_tokens: Some(200),
            output_tokens_time: Some(2000),
            request_time: 500,
            success: true,
            fail_reason: None,
            total_tokens: Some(300),
            start_time: local_ms_cn(2025, 8, 10, 10, 0),
        },
        FullRow {
            request_id: "y-fail-1".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            stream: false,
            ttft: Some(100),
            input_tokens: Some(100),
            input_cache_tokens: 0,
            output_tokens: Some(200),
            output_tokens_time: Some(2000),
            request_time: 500,
            success: false,
            fail_reason: Some("上游 500".to_string()),
            total_tokens: None,
            start_time: local_ms_cn(2026, 3, 5, 10, 0),
        },
    ] {
        insert_full(&db, row).await;
    }

    let (status, json) = get_json(
        app,
        &format!("/api/stats/insight?startTime={start}&endTime={end}&granularity=year"),
    )
    .await;
    assert_eq!(status, 200);
    let data = &json["data"];

    // 两个年桶：2025 / 2026，起点为各年 1 月 1 日（本地）。
    let call_trend = data["callTrend"].as_array().unwrap();
    assert_eq!(call_trend.len(), 2);
    assert_eq!(call_trend[0]["bucketStart"], local_ms_cn(2025, 1, 1, 0, 0));
    assert_eq!(call_trend[1]["bucketStart"], local_ms_cn(2026, 1, 1, 0, 0));
    assert_eq!(call_trend[0]["value"], 1);
    assert_eq!(call_trend[1]["value"], 1);

    let failure_trend = data["failureTrend"].as_array().unwrap();
    assert_eq!(failure_trend.len(), 2);
    assert_eq!(failure_trend[0]["value"], 0);
    assert_eq!(failure_trend[1]["value"], 1);

    // 失败率按年桶重算：2025 = 0/1，2026 = 1/1。
    let failure_rate = data["failureRateTrend"].as_array().unwrap();
    assert_eq!(failure_rate.len(), 2);
    assert_eq!(failure_rate[0]["value"].as_f64().unwrap(), 0.0);
    assert_eq!(failure_rate[1]["value"].as_f64().unwrap(), 1.0);

    // token 趋势同样按年归并（2026 失败行无 total_tokens → 0）。
    let input_token = data["inputTokenTrend"].as_array().unwrap();
    assert_eq!(input_token[0]["value"], 100);
    assert_eq!(input_token[1]["value"], 0);
}

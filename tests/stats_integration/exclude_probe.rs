//! 统计口径排除探测/测速流量（ADR-0024）：`test_model` 落库的探测行
//!（恢复探测 / 边界实测 / 弹窗测速，标记 vm=0、api_key_name='test'）不进入
//! summary/charts/insight/rank 任何端点的统计，但 request 表记录保留
//!（请求日志页仍可见）。

use super::*;

/// 造一行探测流量（与 test_model 落库同标记：vm=0、api_key_name='test'）。
async fn insert_probe(db: &DatabaseConnection, rid: &str, success: bool, start_time: i64) {
    insert_ak_row(
        db,
        ApiKeyRow {
            request_id: rid.into(),
            virtual_model_id: 0,
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            api_key_name: "test".into(),
            success,
            start_time,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(20),
        },
    )
    .await;
}

/// 造一行带失败原因的探测流量（insight 失败原因分布用）。
async fn insert_probe_failure_with_reason(
    db: &DatabaseConnection,
    rid: &str,
    reason: &str,
    start_time: i64,
) {
    request_entity::ActiveModel {
        request_id: Set(rid.into()),
        virtual_model_id: Set(0),
        provider_id: Set(DEFAULT_PROVIDER_ID),
        model_id: Set("gpt-4o".into()),
        stream: Set(false),
        ttft: Set(None),
        input_tokens: Set(Some(10)),
        input_cache_tokens: Set(0),
        input_cache_rate: Set(0.0),
        output_tokens: Set(None),
        output_tokens_time: Set(None),
        tps: Set(0.0),
        start_time: Set(start_time),
        end_time: Set(start_time + 100),
        request_time: Set(100),
        success: Set(false),
        fail_reason: Set(Some(reason.into())),
        total_tokens: Set(Some(20)),
        api_key_name: Set("test".into()),
    }
    .insert(db)
    .await
    .unwrap();
}

#[tokio::test]
async fn test_summary_excludes_probe_requests() {
    let (app, db) = setup_app().await;
    let now = chrono::Utc::now().timestamp_millis();

    // 正常流量：3 成功 + 1 失败 → 成功率 75%。
    for (rid, success) in [("n1", true), ("n2", true), ("n3", true), ("n4", false)] {
        insert_request(
            &db,
            SeedRow {
                request_id: rid.into(),
                provider_id: DEFAULT_PROVIDER_ID,
                model_id: "gpt-4o".into(),
                success,
                start_time: now,
                input_tokens: Some(100),
                input_cache_tokens: 10,
                total_tokens: Some(150),
            },
        )
        .await;
    }
    // 探测流量：永久不可用供应商的恢复探测场景——多条必失败行。
    for i in 0..5 {
        insert_probe(&db, &format!("p{i}"), false, now).await;
    }

    let (status, json) = get_json(app, "/api/stats/summary").await;
    assert_eq!(status, 200);
    let data = &json["data"];
    assert_eq!(data["totalRequests"], 4, "探测行不得计入总请求数");
    assert_eq!(data["successRate"], 0.75, "探测失败不得拉低成功率");
    assert_eq!(data["totalTokens"], 600);
}

#[tokio::test]
async fn test_charts_excludes_probe_requests() {
    let (app, db) = setup_app().await;
    let now = chrono::Utc::now().timestamp_millis();

    insert_request(
        &db,
        SeedRow {
            request_id: "n1".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            success: true,
            start_time: now,
            input_tokens: Some(100),
            input_cache_tokens: 10,
            total_tokens: Some(150),
        },
    )
    .await;
    // 探测成功行（测速场景）：大 token，若漏过滤会明显污染 token 趋势。
    for i in 0..3 {
        insert_probe(&db, &format!("p{i}"), true, now).await;
    }

    let (status, json) = get_json(app, "/api/stats/charts").await;
    assert_eq!(status, 200);
    let data = &json["data"];
    let call_sum: f64 = data["callTrend"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["value"].as_f64().unwrap())
        .sum();
    let token_sum: f64 = data["tokenTrend"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["value"].as_f64().unwrap())
        .sum();
    assert_eq!(call_sum, 1.0, "探测行不得计入调用趋势");
    assert_eq!(token_sum, 150.0, "探测行不得计入 token 趋势");
    // 模型分布同样只含正常行。
    let dist = data["callByModel"].as_array().unwrap();
    let dist_calls: f64 = dist.iter().map(|m| m["value"].as_f64().unwrap()).sum();
    assert_eq!(dist_calls, 1.0, "探测行不得计入模型分布");
}

#[tokio::test]
async fn test_insight_excludes_probe_requests() {
    let (app, db) = setup_app().await;
    let now = chrono::Utc::now().timestamp_millis();
    let bucket_start = (now / HOUR_MS) * HOUR_MS;

    // 正常失败一行（429 限流）+ 探测失败一行（连接超时，不应出现）。
    insert_full(
        &db,
        FullRow {
            request_id: "n1".into(),
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
            fail_reason: Some("上游 429 限流".into()),
            total_tokens: None,
            start_time: bucket_start + 1,
        },
    )
    .await;
    insert_probe_failure_with_reason(&db, "p1", "连接超时", bucket_start + 2).await;

    let (status, json) = get_json(app, "/api/stats/insight").await;
    assert_eq!(status, 200);
    let data = &json["data"];
    let reasons = data["failureReasons"].as_array().unwrap();
    assert_eq!(reasons.len(), 1, "探测失败原因不得出现：{reasons:?}");
    assert_eq!(reasons[0]["reason"], "上游 429 限流");
    assert_eq!(reasons[0]["count"], 1);

    // 失败趋势同样只数正常行：全窗失败数合计 = 1。
    let fail_sum: f64 = data["failureTrend"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["value"].as_f64().unwrap())
        .sum();
    assert_eq!(fail_sum, 1.0, "探测失败不得计入失败趋势");
}

#[tokio::test]
async fn test_provider_rank_excludes_probe_requests() {
    let (app, db) = setup_app().await;
    let now = chrono::Utc::now().timestamp_millis();
    let start = now - HOUR_MS;
    let end = now + HOUR_MS;

    // 正常成功 2 行 + 探测成功 4 行（若漏过滤 requestCount 会变成 6）。
    for rid in ["n1", "n2"] {
        insert_request(
            &db,
            SeedRow {
                request_id: rid.into(),
                provider_id: DEFAULT_PROVIDER_ID,
                model_id: "gpt-4o".into(),
                success: true,
                start_time: now,
                input_tokens: Some(100),
                input_cache_tokens: 10,
                total_tokens: Some(150),
            },
        )
        .await;
    }
    for i in 0..4 {
        insert_probe(&db, &format!("p{i}"), true, now).await;
    }

    let (status, json) = get_json(
        app,
        &format!("/api/stats/provider-rank?startTime={start}&endTime={end}"),
    )
    .await;
    assert_eq!(status, 200);
    let items = json["data"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["providerId"], DEFAULT_PROVIDER_ID);
    assert_eq!(items[0]["requestCount"], 2, "探测成功行不得计入排行");
    assert_eq!(items[0]["totalTokens"], 300.0);
}

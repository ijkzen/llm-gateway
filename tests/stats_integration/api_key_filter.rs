use super::*;

/// charts + insight 按 apiKey 过滤：只聚合该 key 的行。
#[tokio::test]
async fn test_charts_and_insight_filter_by_api_key() {
    let (app, db) = setup_app().await;
    seed_two_keys(&db).await;
    let t0 = (1_700_000_000_000i64 / HOUR_MS) * HOUR_MS;
    let window = t0 + 3 * HOUR_MS;

    // charts：key-a 只 2 次调用（1 成功 + 1 失败均计入 callTrend），callByModel 只含 gpt-4o。
    let (status, json) = get_json(
        app.clone(),
        &format!("/api/stats/charts?startTime={t0}&endTime={window}&apiKey=key-a"),
    )
    .await;
    assert_eq!(status, 200);
    let data = &json["data"];
    let total_calls: i64 = data["callTrend"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["value"].as_i64().unwrap())
        .sum();
    assert_eq!(total_calls, 2); // akf-1 + akf-4
    let call_by_model = data["callByModel"].as_array().unwrap();
    assert_eq!(call_by_model.len(), 1);
    assert_eq!(call_by_model[0]["modelId"], "gpt-4o");
    assert_eq!(call_by_model[0]["value"], 2);

    // charts：key-b 2 次（akf-2 + akf-3），callByModel 含 2 模型。
    let (status, json) = get_json(
        app.clone(),
        &format!("/api/stats/charts?startTime={t0}&endTime={window}&apiKey=key-b"),
    )
    .await;
    assert_eq!(status, 200);
    let total_calls: i64 = json["data"]["callTrend"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["value"].as_i64().unwrap())
        .sum();
    assert_eq!(total_calls, 2);
    assert_eq!(json["data"]["callByModel"].as_array().unwrap().len(), 2);

    // insight：key-a 的 apiKeyRank 只含 key-a 一行（hasTraffic 判定源）。
    let (status, json) = get_json(
        app,
        &format!("/api/stats/insight?startTime={t0}&endTime={window}&apiKey=key-a"),
    )
    .await;
    assert_eq!(status, 200);
    let rank = json["data"]["apiKeyRank"].as_array().unwrap();
    assert_eq!(rank.len(), 1);
    assert_eq!(rank[0]["apiKeyName"], "key-a");
}

/// 三个排行端点按 apiKey 过滤：只列出该 key 覆盖的维度行。
#[tokio::test]
async fn test_rank_endpoints_filter_by_api_key() {
    let (app, db) = setup_app().await;
    seed_two_keys(&db).await;
    let t0 = (1_700_000_000_000i64 / HOUR_MS) * HOUR_MS;
    let window = t0 + 3 * HOUR_MS;
    let window_param = format!("startTime={t0}&endTime={window}");

    // provider-rank：key-a → 只有 provider1。
    let (status, json) = get_json(
        app.clone(),
        &format!("/api/stats/provider-rank?{window_param}&apiKey=key-a"),
    )
    .await;
    assert_eq!(status, 200);
    let items = json["data"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["providerId"], DEFAULT_PROVIDER_ID);

    // provider-rank：key-b → provider1 + provider2。
    let (status, json) = get_json(
        app.clone(),
        &format!("/api/stats/provider-rank?{window_param}&apiKey=key-b"),
    )
    .await;
    assert_eq!(status, 200);
    let items = json["data"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);

    // virtual-model-rank：key-b → vm1 + vm2。
    let (status, json) = get_json(
        app.clone(),
        &format!("/api/stats/virtual-model-rank?{window_param}&apiKey=key-b"),
    )
    .await;
    assert_eq!(status, 200);
    let items = json["data"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    let ids: Vec<i32> = items
        .iter()
        .map(|i| i["virtualModelId"].as_i64().unwrap() as i32)
        .collect();
    assert!(ids.contains(&1) && ids.contains(&2));

    // provider-model-rank：key-a → 只有 (provider1, gpt-4o)；key-b → 两个模型。
    let (status, json) = get_json(
        app.clone(),
        &format!("/api/stats/provider-model-rank?{window_param}&apiKey=key-a"),
    )
    .await;
    assert_eq!(status, 200);
    let items = json["data"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["modelId"], "gpt-4o");

    let (status, json) = get_json(
        app,
        &format!("/api/stats/provider-model-rank?{window_param}&apiKey=key-b"),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(json["data"]["items"].as_array().unwrap().len(), 2);
}

/// 新端点 api-key-metrics：单 key 6 指标聚合 + key 隔离 + 缺参 400。
#[tokio::test]
async fn test_api_key_metrics_endpoint() {
    let (app, db) = setup_app().await;
    seed_two_keys(&db).await;
    let t0 = (1_700_000_000_000i64 / HOUR_MS) * HOUR_MS;
    let window = t0 + 3 * HOUR_MS;
    let window_param = format!("startTime={t0}&endTime={window}");

    // key-a：仅成功 1 条计入指标（失败行不计入），requestCount=1、totalTokens=200。
    let (status, json) = get_json(
        app.clone(),
        &format!("/api/stats/api-key-metrics?{window_param}&apiKey=key-a"),
    )
    .await;
    assert_eq!(status, 200);
    let data = &json["data"];
    assert_eq!(data["requestCount"], 1);
    assert_eq!(data["totalTokens"], 200);
    assert_eq!(data["cacheHitRate"], 0.4); // 40/100

    // key-b：requestCount=2、totalTokens=230、缓存率 20/(100+50)。
    let (status, json) = get_json(
        app.clone(),
        &format!("/api/stats/api-key-metrics?{window_param}&apiKey=key-b"),
    )
    .await;
    assert_eq!(status, 200);
    let data = &json["data"];
    assert_eq!(data["requestCount"], 2);
    assert_eq!(data["totalTokens"], 230);
    assert_eq!(data["cacheHitRate"].as_f64().unwrap(), 0.13333); // 20/(100+50) 保留 5 位

    // 无请求的 key：指标为 0（不 404）。
    let (status, json) = get_json(
        app.clone(),
        &format!("/api/stats/api-key-metrics?{window_param}&apiKey=no-such-key"),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(json["data"]["requestCount"], 0);

    // 缺 apiKey / 缺窗口 → 400。
    let (status, _) = get_json(
        app.clone(),
        &format!("/api/stats/api-key-metrics?{window_param}"),
    )
    .await;
    assert_eq!(status, 400);
    let (status, _) = get_json(app, "/api/stats/api-key-metrics?apiKey=key-a").await;
    assert_eq!(status, 400);
}

/// api-key-rank：现存 Key 行 LEFT JOIN api_key 补出 apiKeyId；已删除 Key 行为 null。
#[tokio::test]
async fn test_api_key_rank_carries_existing_key_id() {
    use llm_gateway::entity::api_key as api_key_entity;

    let (app, db) = setup_app().await;
    let t0 = (1_700_000_000_000i64 / HOUR_MS) * HOUR_MS;

    // 现存 key：id=7 name=alive-key。
    let now = chrono::Utc::now();
    api_key_entity::ActiveModel {
        id: Set(7),
        name: Set("alive-key".to_string()),
        key: Set("encrypted".to_string()),
        key_hash: Set(None),
        enable: Set(true),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(&db)
    .await
    .unwrap();

    // alive-key 两笔 + 历史 key（无 api_key 行）一笔。
    for row in [
        ApiKeyRow {
            request_id: "akid-1".into(),
            virtual_model_id: 1,
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            api_key_name: "alive-key".into(),
            success: true,
            start_time: t0 + 1,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(100),
        },
        ApiKeyRow {
            request_id: "akid-2".into(),
            virtual_model_id: 1,
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "claude-sonnet".into(),
            api_key_name: "alive-key".into(),
            success: true,
            start_time: t0 + 2,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(200),
        },
        ApiKeyRow {
            request_id: "akid-3".into(),
            virtual_model_id: 1,
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "deepseek-v3".into(),
            api_key_name: "deleted-key".into(),
            success: true,
            start_time: t0 + 3,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(300),
        },
    ] {
        insert_ak_row(&db, row).await;
    }

    let (status, json) = get_json(
        app,
        &format!(
            "/api/stats/api-key-rank?startTime={t0}&endTime={}",
            t0 + 2 * HOUR_MS
        ),
    )
    .await;
    assert_eq!(status, 200);
    let items = json["data"]["items"].as_array().unwrap();

    let alive = items
        .iter()
        .find(|i| i["apiKeyName"] == "alive-key")
        .unwrap();
    assert_eq!(alive["apiKeyId"], 7);
    assert_eq!(alive["requestCount"], 2);

    let deleted = items
        .iter()
        .find(|i| i["apiKeyName"] == "deleted-key")
        .unwrap();
    assert_eq!(deleted["apiKeyId"], serde_json::Value::Null);
}

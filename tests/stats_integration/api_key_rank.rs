use super::*;

/// 全量聚合：按 api_key_name 分组，6 指标正确 + 缺省 totalTokens 降序。
#[tokio::test]
async fn test_api_key_rank_aggregates_by_key_name() {
    let (app, db) = setup_app().await;
    let t0 = (1_700_000_000_000i64 / HOUR_MS) * HOUR_MS;
    for row in [
        ApiKeyRow {
            request_id: "ak-1".into(),
            virtual_model_id: 1,
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            api_key_name: "key-a".into(),
            success: true,
            start_time: t0 + 1,
            input_tokens: Some(100),
            input_cache_tokens: 40,
            total_tokens: Some(150),
        },
        // key-a 第二笔：累计 request_count=2、totalTokens=350、缓存率 40/200=0.2。
        ApiKeyRow {
            request_id: "ak-2".into(),
            virtual_model_id: 1,
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            api_key_name: "key-a".into(),
            success: true,
            start_time: t0 + 2,
            input_tokens: Some(100),
            input_cache_tokens: 0,
            total_tokens: Some(200),
        },
        ApiKeyRow {
            request_id: "ak-3".into(),
            virtual_model_id: 1,
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "claude-sonnet".into(),
            api_key_name: "key-b".into(),
            success: true,
            start_time: t0 + 3,
            input_tokens: Some(50),
            input_cache_tokens: 10,
            total_tokens: Some(100),
        },
        // 失败行不计入赛马指标。
        ApiKeyRow {
            request_id: "ak-4".into(),
            virtual_model_id: 1,
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            api_key_name: "key-b".into(),
            success: false,
            start_time: t0 + 4,
            input_tokens: Some(999),
            input_cache_tokens: 0,
            total_tokens: Some(999),
        },
        // 窗口外行不计入。
        ApiKeyRow {
            request_id: "ak-5".into(),
            virtual_model_id: 1,
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            api_key_name: "key-c".into(),
            success: true,
            start_time: t0 - 1,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(10),
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
    let data = &json["data"];
    assert_eq!(data["startTime"], t0);
    let items = data["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);

    // 缺省 totalTokens 降序：key-a (350) 在前，key-b (100) 在后。
    assert_eq!(items[0]["apiKeyName"], "key-a");
    assert_eq!(items[0]["requestCount"], 2);
    assert_eq!(items[0]["totalTokens"], 350);
    let cache_a = items[0]["cacheHitRate"].as_f64().unwrap();
    assert!((cache_a - 0.2).abs() < 1e-9, "cacheA={cache_a}");

    assert_eq!(items[1]["apiKeyName"], "key-b");
    assert_eq!(items[1]["requestCount"], 1);
    assert_eq!(items[1]["totalTokens"], 100);
    let cache_b = items[1]["cacheHitRate"].as_f64().unwrap();
    assert!((cache_b - 0.2).abs() < 1e-9, "cacheB={cache_b}");
}

/// 过滤组合：providerId / virtualModelId / providerId + modelId 各自生效。
#[tokio::test]
async fn test_api_key_rank_filters() {
    let (app, db) = setup_app().await;
    let t0 = (1_700_000_000_000i64 / HOUR_MS) * HOUR_MS;
    for row in [
        // 供应商 1 · 虚拟模型 1 · gpt-4o · key-a
        ApiKeyRow {
            request_id: "akf-1".into(),
            virtual_model_id: 1,
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            api_key_name: "key-a".into(),
            success: true,
            start_time: t0 + 1,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(100),
        },
        // 供应商 2 · 虚拟模型 1 · claude-sonnet · key-a（不同 provider）
        ApiKeyRow {
            request_id: "akf-2".into(),
            virtual_model_id: 1,
            provider_id: 2,
            model_id: "claude-sonnet".into(),
            api_key_name: "key-a".into(),
            success: true,
            start_time: t0 + 2,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(200),
        },
        // 供应商 1 · 虚拟模型 2 · deepseek-v3 · key-b（不同虚拟模型 + 不同模型，
        // 用于区分 virtualModelId 与 providerId+modelId 两种过滤口径）。
        ApiKeyRow {
            request_id: "akf-3".into(),
            virtual_model_id: 2,
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "deepseek-v3".into(),
            api_key_name: "key-b".into(),
            success: true,
            start_time: t0 + 3,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(300),
        },
        // 供应商 1 · 虚拟模型 1 · deepseek-v3 · key-c（不同模型，与 akf-3 同模型不同虚拟模型）。
        ApiKeyRow {
            request_id: "akf-4".into(),
            virtual_model_id: 1,
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "deepseek-v3".into(),
            api_key_name: "key-c".into(),
            success: true,
            start_time: t0 + 4,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(400),
        },
    ] {
        insert_ak_row(&db, row).await;
    }
    let window = format!("startTime={t0}&endTime={}", t0 + 2 * HOUR_MS);

    // providerId=1 过滤：只留 akf-1 / akf-3 / akf-4 → key-a/key-b/key-c。
    let (status, json) = get_json(
        app.clone(),
        &format!("/api/stats/api-key-rank?{window}&providerId={DEFAULT_PROVIDER_ID}"),
    )
    .await;
    assert_eq!(status, 200);
    let items = json["data"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 3);

    // virtualModelId=1 过滤：akf-1 / akf-2 / akf-4 → key-a（两笔合计 300）/ key-c。
    let (status, json) = get_json(
        app.clone(),
        &format!("/api/stats/api-key-rank?{window}&virtualModelId=1"),
    )
    .await;
    assert_eq!(status, 200);
    let items = json["data"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    let key_a = items.iter().find(|i| i["apiKeyName"] == "key-a").unwrap();
    assert_eq!(key_a["requestCount"], 2);
    assert_eq!(key_a["totalTokens"], 300);

    // providerId + modelId 过滤：只留 akf-1（provider 1 下 gpt-4o）→ key-a 一笔 100。
    let (status, json) = get_json(
        app,
        &format!(
            "/api/stats/api-key-rank?{window}&providerId={DEFAULT_PROVIDER_ID}&modelId=gpt-4o"
        ),
    )
    .await;
    assert_eq!(status, 200);
    let items = json["data"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["apiKeyName"], "key-a");
    assert_eq!(items[0]["totalTokens"], 100);
}

/// 排序：sortBy 白名单 + asc/desc 生效；非法 sortBy / 缺窗口返回 400。
#[tokio::test]
async fn test_api_key_rank_sort_and_validation() {
    let (app, db) = setup_app().await;
    let t0 = (1_700_000_000_000i64 / HOUR_MS) * HOUR_MS;
    for row in [
        ApiKeyRow {
            request_id: "aks-1".into(),
            virtual_model_id: 1,
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            api_key_name: "key-a".into(),
            success: true,
            start_time: t0 + 1,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(100),
        },
        ApiKeyRow {
            request_id: "aks-2".into(),
            virtual_model_id: 1,
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            api_key_name: "key-b".into(),
            success: true,
            start_time: t0 + 2,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(50),
        },
    ] {
        insert_ak_row(&db, row).await;
    }
    let window = format!("startTime={t0}&endTime={}", t0 + 2 * HOUR_MS);

    // sortBy=totalTokens&sortOrder=asc → key-b(50) 在前。
    let (status, json) = get_json(
        app.clone(),
        &format!("/api/stats/api-key-rank?{window}&sortBy=totalTokens&sortOrder=asc"),
    )
    .await;
    assert_eq!(status, 200);
    let items = json["data"]["items"].as_array().unwrap();
    assert_eq!(items[0]["apiKeyName"], "key-b");

    // sortBy=requestCount（白名单合法）→ 默认降序。
    let (status, _) = get_json(
        app.clone(),
        &format!("/api/stats/api-key-rank?{window}&sortBy=requestCount"),
    )
    .await;
    assert_eq!(status, 200);

    // 非法 sortBy → 400。
    let (status, _) = get_json(
        app.clone(),
        &format!("/api/stats/api-key-rank?{window}&sortBy=badKey"),
    )
    .await;
    assert_eq!(status, 400);

    // 缺窗口 → 400。
    let (status, _) = get_json(app.clone(), "/api/stats/api-key-rank?sortBy=totalTokens").await;
    assert_eq!(status, 400);

    // 过滤组合契约：providerId + virtualModelId 同传 → 400。
    let (status, _) = get_json(
        app.clone(),
        &format!(
            "/api/stats/api-key-rank?{window}&providerId={DEFAULT_PROVIDER_ID}&virtualModelId=1"
        ),
    )
    .await;
    assert_eq!(status, 400);

    // modelId 无 providerId → 400。
    let (status, _) = get_json(
        app,
        &format!("/api/stats/api-key-rank?{window}&modelId=gpt-4o"),
    )
    .await;
    assert_eq!(status, 400);
}

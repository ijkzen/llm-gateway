use super::*;

/// 开启网络代理的 provider，「刷新模型」请求应经 CONNECT 代理到达目标。
#[tokio::test]
async fn test_refresh_models_goes_through_provider_proxy() {
    let target = spawn_models_mock().await;
    let (proxy_addr, connect_counter) = spawn_connect_proxy().await;
    let (app, db) = setup_app().await;

    let active = provider::ActiveModel {
        name: Set(format!(
            "proxy-refresh-{}",
            chrono::Utc::now().timestamp_millis()
        )),
        enable: Set(true),
        base_url: Set(format!("{target}/v1")),
        api_key: Set(crypto::encrypt("sk-test")),
        custom_header: Set("{}".to_string()),
        protocol_type: Set(0),
        billing_mode: Set(0),
        extra: Set("{}".to_string()),
        proxy_enabled: Set(true),
        proxy_addr: Set(proxy_addr),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    let provider_id = active.insert(&db).await.unwrap().id;

    let (status, body) = send_json(
        app,
        "POST",
        &format!("/api/providers/{provider_id}/models/refresh"),
        Value::Null,
    )
    .await;
    assert_eq!(status, 200, "刷新失败：{body}");
    let ids: Vec<String> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["providerModelId"].as_str().unwrap().to_string())
        .collect();
    assert!(
        ids.contains(&"gpt-4o".to_string()),
        "应解析到 gpt-4o: {ids:?}"
    );
    assert_eq!(
        connect_counter.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "模型刷新应经 CONNECT 代理一次"
    );
}

/// 未开启代理的 provider，「刷新模型」仍直连成功。
#[tokio::test]
async fn test_refresh_models_direct_without_proxy() {
    let target = spawn_models_mock().await;
    let (app, db) = setup_app().await;
    let provider_id = seed_provider(&db, "direct-refresh").await;
    // 把 seed 的 base_url 指向本地 mock（seed 默认 api.example.com 不可达）。
    let row = provider::Entity::find_by_id(provider_id)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    let mut active: provider::ActiveModel = row.into();
    active.base_url = Set(format!("{target}/v1"));
    active.update(&db).await.unwrap();

    let (status, body) = send_json(
        app,
        "POST",
        &format!("/api/providers/{provider_id}/models/refresh"),
        Value::Null,
    )
    .await;
    assert_eq!(status, 200, "直连刷新失败：{body}");
}

/// 目录未命中但与目录条目相似度超 50% 的远端 ID → pending 态并携带建议。
#[tokio::test]
async fn test_refresh_pending_with_catalog_suggestions() {
    let target = spawn_models_mock_with_ids(&["gpt-4o-mini-x"]).await;
    let (app, db) = setup_app().await;
    let provider_id = seed_provider(&db, "pending-refresh").await;
    point_provider_at(&db, provider_id, &target).await;

    let (status, body) = send_json(
        app,
        "POST",
        &format!("/api/providers/{provider_id}/models/refresh"),
        Value::Null,
    )
    .await;
    assert_eq!(status, 200, "刷新失败：{body}");
    let list = body["data"].as_array().unwrap();
    assert_eq!(list.len(), 1, "远端仅一个 ID：{list:?}");
    assert_eq!(list[0]["matchState"], "pending", "应为待确认：{list:?}");
    // pending 候选本体不预填数值，等用户确认后由建议填充。
    assert!(list[0]["contextLength"].is_null());
    let suggestions = list[0]["suggestions"].as_array().expect("应携带建议");
    assert!(!suggestions.is_empty() && suggestions.len() <= 3);
    assert!(
        suggestions[0]["catalogId"]
            .as_str()
            .unwrap()
            .ends_with("gpt-4o-mini"),
        "最相似建议应为 gpt-4o-mini：{suggestions:?}"
    );
    assert!(suggestions[0]["contextLength"].as_i64().unwrap() > 0);
    assert!(suggestions[0]["maxOutputTokens"].as_i64().unwrap() > 0);
}

/// 目录未命中且无相似条目的远端 ID → manual 态、无建议。
#[tokio::test]
async fn test_refresh_manual_without_suggestions() {
    let target = spawn_models_mock_with_ids(&["qqqqwwwwzzzzxxxx"]).await;
    let (app, db) = setup_app().await;
    let provider_id = seed_provider(&db, "manual-refresh").await;
    point_provider_at(&db, provider_id, &target).await;

    let (status, body) = send_json(
        app,
        "POST",
        &format!("/api/providers/{provider_id}/models/refresh"),
        Value::Null,
    )
    .await;
    assert_eq!(status, 200, "刷新失败：{body}");
    let list = body["data"].as_array().unwrap();
    assert_eq!(list.len(), 1, "远端仅一个 ID：{list:?}");
    assert_eq!(list[0]["matchState"], "manual");
    assert!(list[0]["suggestions"].is_null());
}

// ─── 供应商模型级网络代理：CRUD 与校验 ────────────────────────────────────────

/// 创建/更新/批量创建支持模型级代理字段（proxyEnabled + proxyAddr），
/// 校验规则与供应商一致（开启时必填 + http:// + 无认证）。
#[tokio::test]
async fn test_model_proxy_crud_roundtrip() {
    let (app, db) = setup_app().await;
    let provider_id = seed_provider(&db, "p-proxy-crud").await;

    // 创建带模型级代理 → 响应含字段。
    let mut payload = model_payload("proxy-model");
    payload["proxyEnabled"] = json!(true);
    payload["proxyAddr"] = json!("http://127.0.0.1:7890");
    let (status, body) = send_json(
        app.clone(),
        "POST",
        &format!("/api/providers/{provider_id}/models"),
        payload,
    )
    .await;
    assert_eq!(status, 201, "创建失败：{body}");
    assert_eq!(body["data"]["proxyEnabled"], true);
    assert_eq!(body["data"]["proxyAddr"], "http://127.0.0.1:7890");
    let model_id = body["data"]["modelId"].as_i64().unwrap();

    // GET 列表能取回。
    let (status, body) = send_json(
        app.clone(),
        "GET",
        &format!("/api/providers/{provider_id}/models"),
        Value::Null,
    )
    .await;
    assert_eq!(status, 200);
    let listed = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["modelId"].as_i64() == Some(model_id))
        .unwrap();
    assert_eq!(listed["proxyEnabled"], true);
    assert_eq!(listed["proxyAddr"], "http://127.0.0.1:7890");

    // 更新代理字段。
    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/providers/{provider_id}/models/{model_id}"),
        json!({
            "providerModelId": "proxy-model",
            "contextLength": 128000,
            "maxOutputTokens": 4096,
            "proxyEnabled": true,
            "proxyAddr": "http://127.0.0.1:7891",
        }),
    )
    .await;
    assert_eq!(status, 200, "更新失败：{body}");
    assert_eq!(body["data"]["proxyAddr"], "http://127.0.0.1:7891");

    // 批量创建带代理字段。
    let (status, body) = send_json(
        app.clone(),
        "POST",
        &format!("/api/providers/{provider_id}/models/batch"),
        json!({
            "models": [
                {
                    "providerModelId": "batch-a",
                    "contextLength": 32000,
                    "maxOutputTokens": 2048,
                    "proxyEnabled": true,
                    "proxyAddr": "http://127.0.0.1:7892",
                },
                {
                    "providerModelId": "batch-b",
                    "contextLength": 32000,
                    "maxOutputTokens": 2048,
                },
            ],
        }),
    )
    .await;
    assert_eq!(status, 201, "批量创建失败：{body}");
    let created = body["data"].as_array().unwrap();
    assert_eq!(created.len(), 2);
    assert_eq!(created[0]["proxyAddr"], "http://127.0.0.1:7892");
    assert_eq!(created[1]["proxyEnabled"], false, "未传代理字段默认关闭");
    assert_eq!(created[1]["proxyAddr"], "");
}

/// 模型级代理校验：开启时地址必填、需 http:// 开头、不支持带认证地址 → 400。
#[tokio::test]
async fn test_model_proxy_validation_errors() {
    let (app, db) = setup_app().await;
    let provider_id = seed_provider(&db, "p-proxy-validate").await;

    // 开启但地址为空。
    let mut no_addr = model_payload("m1");
    no_addr["proxyEnabled"] = json!(true);
    no_addr["proxyAddr"] = json!("");
    let (status, body) = send_json(
        app.clone(),
        "POST",
        &format!("/api/providers/{provider_id}/models"),
        no_addr,
    )
    .await;
    assert_eq!(status, 400);
    assert!(body["msg"].as_str().unwrap().contains("代理地址"));

    // 地址非 http:// 开头。
    let mut bad_scheme = model_payload("m2");
    bad_scheme["proxyEnabled"] = json!(true);
    bad_scheme["proxyAddr"] = json!("https://127.0.0.1:7890");
    let (status, _) = send_json(
        app.clone(),
        "POST",
        &format!("/api/providers/{provider_id}/models"),
        bad_scheme,
    )
    .await;
    assert_eq!(status, 400);

    // 带认证的地址。
    let mut auth = model_payload("m3");
    auth["proxyEnabled"] = json!(true);
    auth["proxyAddr"] = json!("http://user:pass@127.0.0.1:7890");
    let (status, _) = send_json(
        app.clone(),
        "POST",
        &format!("/api/providers/{provider_id}/models"),
        auth,
    )
    .await;
    assert_eq!(status, 400);

    // 关闭代理时地址留空合法（回落供应商）。
    let mut off = model_payload("m4");
    off["proxyEnabled"] = json!(false);
    off["proxyAddr"] = json!("");
    let (status, _) = send_json(
        app,
        "POST",
        &format!("/api/providers/{provider_id}/models"),
        off,
    )
    .await;
    assert_eq!(status, 201);
}

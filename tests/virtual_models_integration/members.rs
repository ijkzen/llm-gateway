use super::*;

#[tokio::test]
async fn test_create_and_get_virtual_models() {
    let (app, db) = setup_app().await;
    let p1 = seed_provider(&db, "p1").await;
    let p2 = seed_provider(&db, "p2").await;
    let a = seed_provider_model(&db, p1, "gpt-4o@p1").await;
    let c = seed_provider_model(&db, p2, "gpt-4o@p2").await;

    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/virtual-models",
        vm_payload("gpt-4o", &[a, c]),
    )
    .await;
    assert_eq!(status, 201);
    assert_eq!(body["code"], "0");
    assert_eq!(body["data"]["displayId"], "gpt-4o");
    assert_eq!(body["data"]["enable"], true);
    assert_eq!(body["data"]["loadBalancingStrategy"], 3);
    assert_eq!(body["data"]["fallbackStrategy"], 1);
    let items = body["data"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert!(
        items
            .iter()
            .any(|it| it["providerId"] == p1 && it["providerModelId"] == "gpt-4o@p1")
    );
    assert!(items.iter().all(|it| it["providerEnable"] == true));

    let (status, body) = send_json(app.clone(), "GET", "/api/virtual-models", Value::Null).await;
    assert_eq!(status, 200);
    assert_eq!(body["data"].as_array().unwrap().len(), 1);

    let vm_id = body["data"][0]["virtualModelId"].as_i64().unwrap();
    let (status, body) = send_json(
        app,
        "GET",
        &format!("/api/virtual-models/{vm_id}"),
        Value::Null,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["data"]["displayId"], "gpt-4o");
    assert_eq!(body["data"]["items"].as_array().unwrap().len(), 2);
}

/// 成员条目透传模型级/供应商级网络代理字段（虚拟模型成员详情只读展示用）。
#[tokio::test]
async fn test_member_item_echoes_proxy_fields() {
    let (app, db) = setup_app().await;
    let p = seed_provider(&db, "proxy-p").await;
    // 供应商级开启代理（http://proxy.example.com:7890）。
    provider::ActiveModel {
        id: Set(p),
        proxy_enabled: Set(true),
        proxy_addr: Set("http://proxy.example.com:7890".to_string()),
        ..Default::default()
    }
    .update(&db)
    .await
    .unwrap();
    let m = seed_provider_model(&db, p, "gpt-4o").await;
    // 模型级开启代理（覆盖供应商级：http://model-proxy.example.com:7891）。
    provider_model::ActiveModel {
        model_id: Set(m),
        proxy_enabled: Set(true),
        proxy_addr: Set("http://model-proxy.example.com:7891".to_string()),
        ..Default::default()
    }
    .update(&db)
    .await
    .unwrap();

    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/virtual-models",
        vm_payload("vm", &[m]),
    )
    .await;
    assert_eq!(status, 201);
    let item = &body["data"]["items"][0];
    assert_eq!(item["providerProxyEnabled"], true);
    assert_eq!(item["providerProxyAddr"], "http://proxy.example.com:7890");
    assert_eq!(item["modelProxyEnabled"], true);
    assert_eq!(
        item["modelProxyAddr"],
        "http://model-proxy.example.com:7891"
    );

    // 模型级关闭但供应商级开启：只透传模型级关闭状态与供应商级地址（展示「继承」）。
    provider_model::ActiveModel {
        model_id: Set(m),
        proxy_enabled: Set(false),
        proxy_addr: Set(String::new()),
        ..Default::default()
    }
    .update(&db)
    .await
    .unwrap();
    let (_, body) = send_json(app, "GET", "/api/virtual-models", Value::Null).await;
    let item = &body["data"][0]["items"][0];
    assert_eq!(item["modelProxyEnabled"], false);
    assert_eq!(item["modelProxyAddr"], "");
    assert_eq!(item["providerProxyEnabled"], true);
    assert_eq!(item["providerProxyAddr"], "http://proxy.example.com:7890");
}

/// 成员排序：启用成员在前、组内无用量数据时按 virtualModelItemId 升序
/// （LB 静态基础序，与「无数据排后 + id 决平局」一致）。
#[tokio::test]
async fn test_member_sort_enabled_first_then_alphabetical() {
    let (app, db) = setup_app().await;
    let p1 = seed_provider(&db, "p1").await;
    // 故意乱序创建：z 开头停用、a 开头启用、m 开头停用、b 开头启用。
    // virtualModelItemId 按创建顺序递增：z(id1)、a(id2)、m(id3)、b(id4)。
    let z = seed_provider_model(&db, p1, "z-model").await;
    let a = seed_provider_model(&db, p1, "a-model").await;
    let m = seed_provider_model(&db, p1, "m-model").await;
    let b = seed_provider_model(&db, p1, "b-model").await;

    let payload = json!({
        "displayId": "sorted",
        "loadBalancingStrategy": 0,
        "fallbackStrategy": 0,
        "items": [
            {"modelId": z, "enable": false},
            {"modelId": a, "enable": true},
            {"modelId": m, "enable": false},
            {"modelId": b, "enable": true},
        ],
    });
    let (status, body) = send_json(app.clone(), "POST", "/api/virtual-models", payload).await;
    assert_eq!(status, 201);

    let items = body["data"]["items"].as_array().unwrap();
    let remote_ids: Vec<&str> = items
        .iter()
        .map(|it| it["providerModelId"].as_str().unwrap())
        .collect();
    // 启用在前且按 id 升序：a(id2)、b(id4)；停用按 id 升序：z(id1)、m(id3)。
    assert_eq!(
        remote_ids,
        vec!["a-model", "b-model", "z-model", "m-model"],
        "成员应启用优先 + 无用量时按 id 升序：{remote_ids:?}"
    );
}

/// 成员排序第二层：按虚拟模型 LB 策略分组（订阅制优先 → 订阅在前）。
#[tokio::test]
async fn test_member_sort_lb_strategy_grouping() {
    let (app, db) = setup_app().await;
    let payg = seed_provider(&db, "payg").await;
    let sub = seed_provider(&db, "sub").await;
    // 修改 sub 供应商为订阅制（billing_mode=1）。
    provider::ActiveModel {
        id: Set(sub),
        billing_mode: Set(1),
        ..Default::default()
    }
    .update(&db)
    .await
    .unwrap();

    // payg：z-payg（启用）、a-payg（停用）；sub：m-sub（启用）、b-sub（停用）。
    let z_payg = seed_provider_model(&db, payg, "z-payg").await;
    let a_payg = seed_provider_model(&db, payg, "a-payg").await;
    let m_sub = seed_provider_model(&db, sub, "m-sub").await;
    let b_sub = seed_provider_model(&db, sub, "b-sub").await;

    let payload = json!({
        "displayId": "lb-sorted",
        "loadBalancingStrategy": 0,
        "fallbackStrategy": 0,
        "items": [
            {"modelId": z_payg, "enable": true},
            {"modelId": a_payg, "enable": false},
            {"modelId": m_sub, "enable": true},
            {"modelId": b_sub, "enable": false},
        ],
    });
    let (status, body) = send_json(app.clone(), "POST", "/api/virtual-models", payload).await;
    assert_eq!(status, 201);

    let items = body["data"]["items"].as_array().unwrap();
    let remote_ids: Vec<&str> = items
        .iter()
        .map(|it| it["providerModelId"].as_str().unwrap())
        .collect();
    // 启用在前；启用组内订阅制（m-sub）在按量（z-payg）前，字母序持平；
    // 停用组内订阅制（b-sub）在按量（a-payg）前。
    assert_eq!(
        remote_ids,
        vec!["m-sub", "z-payg", "b-sub", "a-payg"],
        "订阅制优先策略下应按 订阅→按量 分组：{remote_ids:?}"
    );
}

/// 成员排序第二层：按量付费优先策略 → 按量在前。
#[tokio::test]
async fn test_member_sort_payg_first_grouping() {
    let (app, db) = setup_app().await;
    let payg = seed_provider(&db, "payg").await;
    let sub = seed_provider(&db, "sub").await;
    provider::ActiveModel {
        id: Set(sub),
        billing_mode: Set(1),
        ..Default::default()
    }
    .update(&db)
    .await
    .unwrap();

    let z_payg = seed_provider_model(&db, payg, "z-payg").await;
    let a_payg = seed_provider_model(&db, payg, "a-payg").await;
    let m_sub = seed_provider_model(&db, sub, "m-sub").await;
    let b_sub = seed_provider_model(&db, sub, "b-sub").await;

    let payload = json!({
        "displayId": "payg-sorted",
        "loadBalancingStrategy": 1,
        "fallbackStrategy": 0,
        "items": [
            {"modelId": z_payg, "enable": true},
            {"modelId": a_payg, "enable": false},
            {"modelId": m_sub, "enable": true},
            {"modelId": b_sub, "enable": false},
        ],
    });
    let (status, body) = send_json(app.clone(), "POST", "/api/virtual-models", payload).await;
    assert_eq!(status, 201);

    let items = body["data"]["items"].as_array().unwrap();
    let remote_ids: Vec<&str> = items
        .iter()
        .map(|it| it["providerModelId"].as_str().unwrap())
        .collect();
    // 启用组内按量（z-payg）在订阅（m-sub）前；停用组内按量（a-payg）在订阅（b-sub）前。
    assert_eq!(
        remote_ids,
        vec!["z-payg", "m-sub", "a-payg", "b-sub"],
        "按量付费优先策略下应按 按量→订阅 分组：{remote_ids:?}"
    );
}

/// 用量感知排序：策略 0 下订阅制组内按剩余百分比（5h→周→月）降序，
/// 无用量数据的成员排在有数据成员之后；耗尽成员不剔除（展示端展示全部）。
#[tokio::test]
async fn test_member_sort_usage_aware_within_subscription() {
    let (app, db) = setup_app().await;
    let p1 = seed_provider(&db, "p1").await;
    let p2 = seed_provider(&db, "p2").await;
    let p3 = seed_provider(&db, "p3").await;
    provider::ActiveModel {
        id: Set(p1),
        billing_mode: Set(1),
        ..Default::default()
    }
    .update(&db)
    .await
    .unwrap();
    provider::ActiveModel {
        id: Set(p2),
        billing_mode: Set(1),
        ..Default::default()
    }
    .update(&db)
    .await
    .unwrap();

    // 全部启用。p1 剩余 5h=95%、p2 剩余 5h=90%、p3 无用量数据。
    let m1 = seed_provider_model(&db, p1, "m1").await;
    let m2 = seed_provider_model(&db, p2, "m2").await;
    let m3 = seed_provider_model(&db, p3, "m3").await;

    // 写入 p1/p2 的订阅用量缓存（剩余百分比 95 vs 90，5h 决胜）。p3 无缓存。
    let quota = |provider_id: i32, five_hour: f64| llm_gateway::usage::types::UsageData {
        provider_id,
        fetched_at: chrono::Utc::now(),
        kind: llm_gateway::usage::types::UsageKind::Quota,
        plan: None,
        windows: vec![
            llm_gateway::usage::types::QuotaWindow::from_remaining_percent(
                llm_gateway::usage::types::WindowKind::FiveHour,
                five_hour,
                None,
            ),
        ],
        balances: vec![],
    };
    llm_gateway::usage::persist::write_usage_cache(&db, &quota(p1, 95.0))
        .await
        .unwrap();
    llm_gateway::usage::persist::write_usage_cache(&db, &quota(p2, 90.0))
        .await
        .unwrap();

    let payload = json!({
        "displayId": "usage-sorted-sub",
        "loadBalancingStrategy": 0,
        "fallbackStrategy": 0,
        "items": [
            {"modelId": m1, "enable": true},
            {"modelId": m2, "enable": true},
            {"modelId": m3, "enable": true},
        ],
    });
    let (status, body) = send_json(app.clone(), "POST", "/api/virtual-models", payload).await;
    assert_eq!(status, 201);

    let items = body["data"]["items"].as_array().unwrap();
    let remote_ids: Vec<&str> = items
        .iter()
        .map(|it| it["providerModelId"].as_str().unwrap())
        .collect();
    // 订阅组：p1(95%) 在 p2(90%) 前；无数据的 p3 排最末。
    assert_eq!(
        remote_ids,
        vec!["m1", "m2", "m3"],
        "订阅制组内应按剩余百分比降序、无数据排后：{remote_ids:?}"
    );
}

/// 用量感知排序：按量组内按主余额降序；无用量数据排后。
#[tokio::test]
async fn test_member_sort_usage_aware_within_payg() {
    let (app, db) = setup_app().await;
    let p1 = seed_provider(&db, "p1").await;
    let p2 = seed_provider(&db, "p2").await;
    let p3 = seed_provider(&db, "p3").await;

    let m1 = seed_provider_model(&db, p1, "m1").await;
    let m2 = seed_provider_model(&db, p2, "m2").await;
    let m3 = seed_provider_model(&db, p3, "m3").await;

    // p1 主余额 100、p2 主余额 50、p3 无余额数据。
    let balance = |provider_id: i32, amount: f64| llm_gateway::usage::types::UsageData {
        provider_id,
        fetched_at: chrono::Utc::now(),
        kind: llm_gateway::usage::types::UsageKind::Balance,
        plan: None,
        windows: vec![],
        balances: vec![llm_gateway::usage::types::BalanceItem {
            label: "余额".to_string(),
            amount,
            currency: None,
            primary: true,
        }],
    };
    llm_gateway::usage::persist::write_usage_cache(&db, &balance(p1, 100.0))
        .await
        .unwrap();
    llm_gateway::usage::persist::write_usage_cache(&db, &balance(p2, 50.0))
        .await
        .unwrap();

    let payload = json!({
        "displayId": "usage-sorted-payg",
        "loadBalancingStrategy": 0,
        "fallbackStrategy": 0,
        "items": [
            {"modelId": m1, "enable": true},
            {"modelId": m2, "enable": true},
            {"modelId": m3, "enable": true},
        ],
    });
    let (status, body) = send_json(app.clone(), "POST", "/api/virtual-models", payload).await;
    assert_eq!(status, 201);

    let items = body["data"]["items"].as_array().unwrap();
    let remote_ids: Vec<&str> = items
        .iter()
        .map(|it| it["providerModelId"].as_str().unwrap())
        .collect();
    // 按量组：p1(100) 在 p2(50) 前；无余额数据的 p3 排最末。
    assert_eq!(
        remote_ids,
        vec!["m1", "m2", "m3"],
        "按量组内应按主余额降序、无数据排后：{remote_ids:?}"
    );
}

#[tokio::test]
async fn test_create_virtual_model_validations() {
    let (app, db) = setup_app().await;
    let p1 = seed_provider(&db, "p1").await;
    let a = seed_provider_model(&db, p1, "a").await;

    // 空 displayId。
    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/virtual-models",
        vm_payload("  ", &[a]),
    )
    .await;
    assert_eq!(status, 400);
    assert!(body["msg"].as_str().unwrap().contains("模型 ID"));

    // 非法负载均衡策略。
    let mut payload = vm_payload("vm", &[a]);
    payload["loadBalancingStrategy"] = json!(4);
    let (status, body) = send_json(app.clone(), "POST", "/api/virtual-models", payload).await;
    assert_eq!(status, 400);
    assert!(body["msg"].as_str().unwrap().contains("负载均衡策略"));

    // 非法降级策略。
    let mut payload = vm_payload("vm", &[a]);
    payload["fallbackStrategy"] = json!(2);
    let (status, body) = send_json(app.clone(), "POST", "/api/virtual-models", payload).await;
    assert_eq!(status, 400);
    assert!(body["msg"].as_str().unwrap().contains("降级策略"));

    // 空 items。
    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/virtual-models",
        vm_payload("vm", &[]),
    )
    .await;
    assert_eq!(status, 400);
    assert!(
        body["msg"]
            .as_str()
            .unwrap()
            .contains("至少选择一个成员模型")
    );

    // 不存在的 model_id。
    let (status, body) =
        send_json(app, "POST", "/api/virtual-models", vm_payload("vm", &[999])).await;
    assert_eq!(status, 400);
    assert!(body["msg"].as_str().unwrap().contains("不存在"));
}

#[tokio::test]
async fn test_duplicate_display_id_rejected() {
    let (app, db) = setup_app().await;
    let p1 = seed_provider(&db, "p1").await;
    let a = seed_provider_model(&db, p1, "a").await;
    let b = seed_provider_model(&db, p1, "b").await;

    let (status, _) = send_json(
        app.clone(),
        "POST",
        "/api/virtual-models",
        vm_payload("vm-a", &[a]),
    )
    .await;
    assert_eq!(status, 201);
    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/virtual-models",
        vm_payload("vm-a", &[b]),
    )
    .await;
    assert_eq!(status, 400);
    assert!(body["msg"].as_str().unwrap().contains("已存在"));

    // 更新为已有的 display_id 同样冲突。
    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/virtual-models",
        vm_payload("vm-b", &[b]),
    )
    .await;
    assert_eq!(status, 201);
    let vm_b = body["data"]["virtualModelId"].as_i64().unwrap();
    let (status, body) = send_json(
        app,
        "PUT",
        &format!("/api/virtual-models/{vm_b}"),
        json!({"displayId": "vm-a"}),
    )
    .await;
    assert_eq!(status, 400);
    assert!(body["msg"].as_str().unwrap().contains("已存在"));
}

#[tokio::test]
async fn test_model_can_only_belong_to_one_virtual_model() {
    let (app, db) = setup_app().await;
    let p1 = seed_provider(&db, "p1").await;
    let a = seed_provider_model(&db, p1, "a").await;
    let b = seed_provider_model(&db, p1, "b").await;

    let (status, _) = send_json(
        app.clone(),
        "POST",
        "/api/virtual-models",
        vm_payload("vm1", &[a]),
    )
    .await;
    assert_eq!(status, 201);

    // 创建时包含已被 vm1 占用的 a → 400。
    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/virtual-models",
        vm_payload("vm2", &[a]),
    )
    .await;
    assert_eq!(status, 400);
    assert!(
        body["msg"]
            .as_str()
            .unwrap()
            .contains("已被其他虚拟模型使用")
    );

    // 更新其他虚拟模型把 a 加进来 → 400。
    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/virtual-models",
        vm_payload("vm2", &[b]),
    )
    .await;
    assert_eq!(status, 201);
    let vm2 = body["data"]["virtualModelId"].as_i64().unwrap();
    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/virtual-models/{vm2}"),
        vm_payload("vm2", &[b, a]),
    )
    .await;
    assert_eq!(status, 400);
    assert!(
        body["msg"]
            .as_str()
            .unwrap()
            .contains("已被其他虚拟模型使用")
    );

    // 保留自身成员的更新不受影响。
    let (status, _) = send_json(
        app,
        "PUT",
        &format!("/api/virtual-models/{vm2}"),
        vm_payload("vm2", &[b]),
    )
    .await;
    assert_eq!(status, 200);
}

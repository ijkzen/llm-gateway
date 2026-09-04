mod common;

use axum::body::Body;
use axum::http::Request;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde_json::{Value, json};
use tower::ServiceExt;

use llm_gateway::entity::provider;
use llm_gateway::entity::provider_model;
use llm_gateway::entity::virtual_model_item;

/// 建一个测试 Provider（api_key 加密存储），返回其 id。
async fn seed_provider(db: &sea_orm::DatabaseConnection, name: &str) -> i32 {
    let active = provider::ActiveModel {
        name: Set(name.to_string()),
        enable: Set(true),
        base_url: Set("https://api.example.com/v1".to_string()),
        api_key: Set(llm_gateway::crypto::encrypt("sk-test")),
        custom_header: Set("{}".to_string()),
        protocol_type: Set(0),
        billing_mode: Set(0),
        extra: Set("{}".to_string()),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    active.insert(db).await.unwrap().id
}

/// 建一个测试 ProviderModel，返回其 model_id。
async fn seed_provider_model(
    db: &sea_orm::DatabaseConnection,
    provider_id: i32,
    remote_id: &str,
) -> i32 {
    let active = provider_model::ActiveModel {
        provider_id: Set(provider_id),
        provider_model_id: Set(remote_id.to_string()),
        context_length: Set(128000),
        max_output_tokens: Set(4096),
        reasoning: Set(false),
        tool_use: Set(true),
        image_understand: Set(false),
        video_understand: Set(false),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    active.insert(db).await.unwrap().model_id
}

async fn setup_app() -> (axum::Router, sea_orm::DatabaseConnection) {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    scheduler.start().await.unwrap();
    let app = common::build_authed_app(db.clone(), scheduler, log_tx).await;
    (app, db)
}

async fn send_json(app: axum::Router, method: &str, uri: &str, body: Value) -> (u16, Value) {
    let request: Request<Body> = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    let status = response.status().as_u16();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let parsed: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, parsed)
}

/// 创建虚拟模型的请求体（成员默认启用）。
fn vm_payload(display_id: &str, model_ids: &[i32]) -> Value {
    json!({
        "displayId": display_id,
        "loadBalancingStrategy": 3,
        "fallbackStrategy": 1,
        "items": model_ids
            .iter()
            .map(|id| json!({"modelId": id}))
            .collect::<Vec<_>>(),
    })
}

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

#[tokio::test]
async fn test_update_virtual_model_diffs_members_and_preserves_enable() {
    let (app, db) = setup_app().await;
    let p1 = seed_provider(&db, "p1").await;
    let a = seed_provider_model(&db, p1, "a").await;
    let b = seed_provider_model(&db, p1, "b").await;
    let c = seed_provider_model(&db, p1, "c").await;

    // a 启用、b 禁用。
    let payload = json!({
        "displayId": "vm1",
        "loadBalancingStrategy": 0,
        "fallbackStrategy": 0,
        "items": [
            {"modelId": a},
            {"modelId": b, "enable": false},
        ],
    });
    let (status, body) = send_json(app.clone(), "POST", "/api/virtual-models", payload).await;
    assert_eq!(status, 201);
    assert_eq!(body["data"]["items"][1]["enable"], false);
    let vm1 = body["data"]["virtualModelId"].as_i64().unwrap();

    // 更新成员为 [a, c]（b 移除、c 新增），同时修改 displayId 与策略。
    let mut payload = vm_payload("vm-renamed", &[a, c]);
    payload["loadBalancingStrategy"] = json!(2);
    payload["fallbackStrategy"] = json!(1);
    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/virtual-models/{vm1}"),
        payload,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["data"]["displayId"], "vm-renamed");
    assert_eq!(body["data"]["loadBalancingStrategy"], 2);
    assert_eq!(body["data"]["fallbackStrategy"], 1);
    let items = body["data"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 2, "b 应被移除");
    let a_item = items.iter().find(|it| it["modelId"] == a).unwrap();
    assert_eq!(a_item["enable"], true, "保留成员的 enable 不变");
    let c_item = items.iter().find(|it| it["modelId"] == c).unwrap();
    assert_eq!(c_item["enable"], true, "新增成员默认启用");

    // 只传 enable → 成员不变。
    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/virtual-models/{vm1}"),
        json!({"enable": false}),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["data"]["enable"], false);
    assert_eq!(body["data"]["items"].as_array().unwrap().len(), 2);

    // 请求里 items 为空 → 400。
    let (status, _) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/virtual-models/{vm1}"),
        json!({"enable": true, "items": []}),
    )
    .await;
    assert_eq!(status, 400);

    // b 已被移除，应可再映射到新虚拟模型。
    let (status, _) = send_json(app, "POST", "/api/virtual-models", vm_payload("vm2", &[b])).await;
    assert_eq!(status, 201);
}

#[tokio::test]
async fn test_update_missing_virtual_model_returns_404() {
    let (app, _db) = setup_app().await;
    let (status, _) = send_json(
        app.clone(),
        "PUT",
        "/api/virtual-models/999",
        json!({"enable": true}),
    )
    .await;
    assert_eq!(status, 404);
    let (status, _) = send_json(app, "GET", "/api/virtual-models/999", Value::Null).await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn test_delete_virtual_model_releases_members() {
    let (app, db) = setup_app().await;
    let p1 = seed_provider(&db, "p1").await;
    let a = seed_provider_model(&db, p1, "a").await;
    let b = seed_provider_model(&db, p1, "b").await;

    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/virtual-models",
        vm_payload("vm1", &[a]),
    )
    .await;
    assert_eq!(status, 201);
    let vm1 = body["data"]["virtualModelId"].as_i64().unwrap();
    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/virtual-models",
        vm_payload("vm2", &[b]),
    )
    .await;
    assert_eq!(status, 201);
    let vm2 = body["data"]["virtualModelId"].as_i64().unwrap();

    // a、b 均被占用 → 400。
    let (status, _) = send_json(
        app.clone(),
        "POST",
        "/api/virtual-models",
        vm_payload("vm3", &[a, b]),
    )
    .await;
    assert_eq!(status, 400);

    // 删除 vm1 释放 a；b 仍被占用 → 400。
    let (status, _) = send_json(
        app.clone(),
        "DELETE",
        &format!("/api/virtual-models/{vm1}"),
        Value::Null,
    )
    .await;
    assert_eq!(status, 200);
    let (status, _) = send_json(
        app.clone(),
        "POST",
        "/api/virtual-models",
        vm_payload("vm3", &[a, b]),
    )
    .await;
    assert_eq!(status, 400);

    // 删除 vm2 后 a、b 全部释放。
    let (status, _) = send_json(
        app.clone(),
        "DELETE",
        &format!("/api/virtual-models/{vm2}"),
        Value::Null,
    )
    .await;
    assert_eq!(status, 200);
    let items = virtual_model_item::Entity::find().all(&db).await.unwrap();
    assert!(items.is_empty(), "级联删除成员条目");

    let (status, _) = send_json(
        app.clone(),
        "POST",
        "/api/virtual-models",
        vm_payload("vm3", &[a, b]),
    )
    .await;
    assert_eq!(status, 201);

    // 重复删除 → 404。
    let (status, _) = send_json(
        app,
        "DELETE",
        &format!("/api/virtual-models/{vm2}"),
        Value::Null,
    )
    .await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn test_delete_provider_cascades_virtual_model_items() {
    let (app, db) = setup_app().await;
    let p1 = seed_provider(&db, "p1").await;
    let p2 = seed_provider(&db, "p2").await;
    let a = seed_provider_model(&db, p1, "a").await;
    let c = seed_provider_model(&db, p2, "c").await;

    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/virtual-models",
        vm_payload("vm1", &[a]),
    )
    .await;
    assert_eq!(status, 201);
    let vm1 = body["data"]["virtualModelId"].as_i64().unwrap();
    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/virtual-models",
        vm_payload("vm2", &[c]),
    )
    .await;
    assert_eq!(status, 201);
    let vm2 = body["data"]["virtualModelId"].as_i64().unwrap();

    let (status, _) = send_json(
        app.clone(),
        "DELETE",
        &format!("/api/providers/{p1}"),
        Value::Null,
    )
    .await;
    assert_eq!(status, 200);

    // vm1 的成员被级联清理；vm2 不受影响。
    let (status, body) = send_json(
        app.clone(),
        "GET",
        &format!("/api/virtual-models/{vm1}"),
        Value::Null,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["data"]["items"].as_array().unwrap().len(), 0);
    let (status, body) = send_json(
        app.clone(),
        "GET",
        &format!("/api/virtual-models/{vm2}"),
        Value::Null,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["data"]["items"].as_array().unwrap().len(), 1);

    // 已删除供应商的模型不能再被映射（已不存在）。
    let (status, _) = send_json(app, "POST", "/api/virtual-models", vm_payload("vm3", &[a])).await;
    assert_eq!(status, 400);
}

/// 手动禁用供应商应级联停用其名下虚拟模型子模型，且成员排序把停用者沉底；
/// 重新启用后级联恢复（与用量额度门控 apply_usage_gate 语义一致）。
#[tokio::test]
async fn provider_disable_cascades_to_virtual_model_items_and_resorts() {
    let (app, db) = setup_app().await;
    // "a-model" 挂在将被禁用的供应商下，字母序本就排在前；"b-model" 挂在保留启用的供应商下。
    let p_keep = seed_provider(&db, "keep").await;
    let p_disable = seed_provider(&db, "disable").await;
    let m_alpha = seed_provider_model(&db, p_disable, "a-model").await;
    let m_beta = seed_provider_model(&db, p_keep, "b-model").await;

    let (status, _) = send_json(
        app.clone(),
        "POST",
        "/api/virtual-models",
        vm_payload("vm", &[m_alpha, m_beta]),
    )
    .await;
    assert_eq!(status, 201);

    // 手动禁用供应商（供应商详情卡片的启用开关即此接口）。
    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/providers/{p_disable}"),
        json!({ "enable": false }),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["code"], "0");

    // DB 层：该供应商名下全部虚拟模型子模型同步停用（不是只翻 provider.enable）。
    let disabled_item = virtual_model_item::Entity::find()
        .filter(virtual_model_item::Column::ModelId.eq(m_alpha))
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert!(!disabled_item.enable, "禁用供应商后子模型条目应被级联停用");

    // API 层：被禁用的成员沉底排在启用成员之后，且带 providerEnable=false。
    let (status, body) = send_json(app.clone(), "GET", "/api/virtual-models", Value::Null).await;
    assert_eq!(status, 200);
    let vms = body["data"].as_array().unwrap();
    let vm = vms.iter().find(|v| v["displayId"] == "vm").unwrap();
    let members = vm["items"].as_array().unwrap();
    assert_eq!(members.len(), 2);
    assert_eq!(members[0]["providerModelId"], "b-model");
    assert!(members[0]["enable"].as_bool().unwrap());
    assert!(members[0]["providerEnable"].as_bool().unwrap());
    assert_eq!(members[1]["providerModelId"], "a-model");
    assert!(!members[1]["enable"].as_bool().unwrap());
    assert!(!members[1]["providerEnable"].as_bool().unwrap());

    // 重新启用 → 级联恢复子模型。
    let (status, _) = send_json(
        app,
        "PUT",
        &format!("/api/providers/{p_disable}"),
        json!({ "enable": true }),
    )
    .await;
    assert_eq!(status, 200);
    let reenabled = virtual_model_item::Entity::find()
        .filter(virtual_model_item::Column::ModelId.eq(m_alpha))
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert!(reenabled.enable, "重新启用供应商后子模型条目应被级联恢复");
}

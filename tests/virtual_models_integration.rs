mod common;

use axum::body::Body;
use axum::http::Request;
use sea_orm::{ActiveModelTrait, EntityTrait, Set};
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
        status: Set(0),
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
async fn seed_provider_model(db: &sea_orm::DatabaseConnection, provider_id: i32, remote_id: &str) -> i32 {
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
    assert!(body["msg"].as_str().unwrap().contains("至少选择一个成员模型"));

    // 不存在的 model_id。
    let (status, body) = send_json(
        app,
        "POST",
        "/api/virtual-models",
        vm_payload("vm", &[999]),
    )
    .await;
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
    assert!(body["msg"].as_str().unwrap().contains("已被其他虚拟模型使用"));

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
    assert!(body["msg"].as_str().unwrap().contains("已被其他虚拟模型使用"));

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
    let (status, _) = send_json(
        app,
        "POST",
        "/api/virtual-models",
        vm_payload("vm2", &[b]),
    )
    .await;
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
    let items = virtual_model_item::Entity::find()
        .all(&db)
        .await
        .unwrap();
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

    let (status, _) = send_json(app.clone(), "DELETE", &format!("/api/providers/{p1}"), Value::Null)
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
    let (status, _) = send_json(
        app,
        "POST",
        "/api/virtual-models",
        vm_payload("vm3", &[a]),
    )
    .await;
    assert_eq!(status, 400);
}

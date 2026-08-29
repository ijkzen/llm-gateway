mod common;

use axum::body::Body;
use axum::http::Request;
use sea_orm::{ActiveModelTrait, Set};
use serde_json::{Value, json};
use tower::ServiceExt;

use llm_gateway::entity::provider;
use llm_gateway::entity::provider_model;

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
    let app = common::build_app(db.clone(), scheduler, log_tx);
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

fn vm_payload(display_id: &str, model_ids: &[i32], enable: bool) -> Value {
    json!({
        "displayId": display_id,
        "enable": enable,
        "loadBalancingStrategy": 3,
        "fallbackStrategy": 1,
        "items": model_ids
            .iter()
            .map(|id| json!({"modelId": id}))
            .collect::<Vec<_>>(),
    })
}

/// 建两个虚拟模型：启用的 `gpt-4o` 与禁用的 `hidden`，返回 (app)。
async fn seed_two_virtual_models() -> axum::Router {
    let (app, db) = setup_app().await;
    let p1 = seed_provider(&db, "p1").await;
    let a = seed_provider_model(&db, p1, "a").await;
    let b = seed_provider_model(&db, p1, "b").await;

    let (status, _) = send_json(
        app.clone(),
        "POST",
        "/api/virtual-models",
        vm_payload("gpt-4o", &[a], true),
    )
    .await;
    assert_eq!(status, 201);
    let (status, _) = send_json(
        app.clone(),
        "POST",
        "/api/virtual-models",
        vm_payload("hidden", &[b], false),
    )
    .await;
    assert_eq!(status, 201);
    app
}

#[tokio::test]
async fn test_v1_models_list_shape() {
    let app = seed_two_virtual_models().await;

    let (status, body) = send_json(app, "GET", "/v1/models", Value::Null).await;
    assert_eq!(status, 200);
    assert_eq!(body["object"], "list");
    let data = body["data"].as_array().unwrap();
    assert_eq!(data.len(), 1, "禁用的虚拟模型不应出现");
    assert_eq!(data[0]["id"], "gpt-4o");
    assert_eq!(data[0]["object"], "model");
    assert!(data[0]["created"].is_i64());
    assert_eq!(data[0]["owned_by"], "llm-gateway");
    assert!(
        data[0].get("displayId").is_none() && data[0].get("items").is_none(),
        "/v1 不应携带内部管理字段"
    );
}

#[tokio::test]
async fn test_v1_model_detail() {
    let app = seed_two_virtual_models().await;

    let (status, body) = send_json(app.clone(), "GET", "/v1/models/gpt-4o", Value::Null).await;
    assert_eq!(status, 200);
    assert_eq!(body["id"], "gpt-4o");
    assert_eq!(body["object"], "model");
    assert_eq!(body["owned_by"], "llm-gateway");
}

#[tokio::test]
async fn test_v1_model_detail_404_format() {
    let app = seed_two_virtual_models().await;

    // 禁用的虚拟模型按不存在处理。
    let (status, body) = send_json(app.clone(), "GET", "/v1/models/hidden", Value::Null).await;
    assert_eq!(status, 404);
    assert_eq!(body["error"]["code"], "model_not_found");
    assert_eq!(body["error"]["type"], "invalid_request_error");
    assert!(body["error"]["message"].is_string());

    let (status, body) = send_json(app, "GET", "/v1/models/missing", Value::Null).await;
    assert_eq!(status, 404);
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("does not exist")
    );
}

#[tokio::test]
async fn test_v1_models_empty_list() {
    let (app, _db) = setup_app().await;
    let (status, body) = send_json(app, "GET", "/v1/models", Value::Null).await;
    assert_eq!(status, 200);
    assert_eq!(body["object"], "list");
    assert_eq!(body["data"].as_array().unwrap().len(), 0);
}

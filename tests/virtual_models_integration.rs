mod common;

use axum::body::Body;
use axum::http::Request;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde_json::{Value, json};
use tower::ServiceExt;

use llm_gateway::entity::provider;
use llm_gateway::entity::provider_model;
use llm_gateway::entity::virtual_model_item;

#[path = "virtual_models_integration/interface.rs"]
mod interface;
#[path = "virtual_models_integration/members.rs"]
mod members;
#[path = "virtual_models_integration/models.rs"]
mod models;

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

/// 创建带接口类型的虚拟模型请求体。
fn vm_payload_typed(display_id: &str, model_ids: &[i32], interface_type: i32) -> Value {
    let mut payload = vm_payload(display_id, model_ids);
    payload["interfaceType"] = json!(interface_type);
    payload
}

/// 以 Bearer 凭证发送 /v1 请求。
async fn send_v1_json(app: axum::Router, method: &str, uri: &str, body: Value) -> (u16, Value) {
    let request: Request<Body> = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header("authorization", common::TEST_BEARER)
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

/// 建指定协议的 Provider 与无覆盖的 ProviderModel。
async fn seed_provider_with_protocol(
    db: &sea_orm::DatabaseConnection,
    name: &str,
    protocol: i32,
) -> i32 {
    let active = provider::ActiveModel {
        name: Set(name.to_string()),
        enable: Set(true),
        base_url: Set("https://api.example.com/v1".to_string()),
        api_key: Set(llm_gateway::crypto::encrypt("sk-test")),
        custom_header: Set("{}".to_string()),
        protocol_type: Set(protocol),
        billing_mode: Set(0),
        extra: Set("{}".to_string()),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    active.insert(db).await.unwrap().id
}

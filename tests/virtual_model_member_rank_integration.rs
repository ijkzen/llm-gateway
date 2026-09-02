mod common;

use axum::body::Body;
use axum::http::Request;
use sea_orm::{ActiveModelTrait, DatabaseConnection, Set};
use tower::ServiceExt;

use llm_gateway::entity::provider as provider_entity;
use llm_gateway::entity::provider_model as provider_model_entity;
use llm_gateway::entity::request as request_entity;
use llm_gateway::entity::virtual_model as virtual_model_entity;
use llm_gateway::entity::virtual_model_item as virtual_model_item_entity;

/// 窗口起点：固定锚点，避免与「当前时间」的耦合。
const T0: i64 = 1_700_000_000_000;

async fn seed_virtual_model(db: &DatabaseConnection, id: i32, display_id: &str) {
    let now = chrono::Utc::now();
    virtual_model_entity::ActiveModel {
        virtual_model_id: Set(id),
        display_id: Set(display_id.to_string()),
        enable: Set(true),
        load_balancing_strategy: Set(0),
        fallback_strategy: Set(0),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db)
    .await
    .unwrap();
}

async fn seed_provider(db: &DatabaseConnection, id: i32, name: &str) {
    let now = chrono::Utc::now();
    provider_entity::ActiveModel {
        id: Set(id),
        name: Set(name.to_string()),
        enable: Set(true),
        base_url: Set("https://example.com".to_string()),
        api_key: Set("encrypted".to_string()),
        custom_header: Set("{}".to_string()),
        status: Set(0),
        protocol_type: Set(0),
        billing_mode: Set(0),
        extra: Set("{}".to_string()),
        sort_order: Set(0),
        created_at: Set(now),
        updated_at: Set(now),
        proxy_enabled: Set(false),
        proxy_addr: Set(String::new()),
        failure_disabled: Set(false),
    }
    .insert(db)
    .await
    .unwrap();
}

async fn seed_provider_model(
    db: &DatabaseConnection,
    model_id: i32,
    provider_id: i32,
    provider_model_id: &str,
) {
    let now = chrono::Utc::now();
    provider_model_entity::ActiveModel {
        model_id: Set(model_id),
        provider_id: Set(provider_id),
        provider_model_id: Set(provider_model_id.to_string()),
        context_length: Set(128_000),
        max_output_tokens: Set(8_192),
        reasoning: Set(false),
        tool_use: Set(true),
        image_understand: Set(false),
        video_understand: Set(false),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db)
    .await
    .unwrap();
}

async fn seed_member(
    db: &DatabaseConnection,
    item_id: i32,
    virtual_model_id: i32,
    model_id: i32,
    enable: bool,
) {
    let now = chrono::Utc::now();
    virtual_model_item_entity::ActiveModel {
        virtual_model_item_id: Set(item_id),
        virtual_model_id: Set(virtual_model_id),
        model_id: Set(model_id),
        enable: Set(enable),
        cascade_disabled: Set(false),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db)
    .await
    .unwrap();
}

#[allow(clippy::too_many_arguments)]
async fn insert_request(
    db: &DatabaseConnection,
    request_id: &str,
    virtual_model_id: i32,
    provider_id: i32,
    model_id: &str,
    start_time: i64,
    total_tokens: Option<i64>,
    stream: bool,
    ttft: Option<i64>,
    output_tokens: Option<i64>,
    tps: f64,
    input_tokens: Option<i64>,
    input_cache_tokens: i64,
) {
    let end_time = start_time + 500;
    request_entity::ActiveModel {
        request_id: Set(request_id.to_string()),
        virtual_model_id: Set(virtual_model_id),
        provider_id: Set(provider_id),
        model_id: Set(model_id.to_string()),
        stream: Set(stream),
        ttft: Set(ttft),
        input_tokens: Set(input_tokens),
        input_cache_tokens: Set(input_cache_tokens),
        input_cache_rate: Set(0.0),
        output_tokens: Set(output_tokens),
        output_tokens_time: Set(None),
        tps: Set(tps),
        start_time: Set(start_time),
        end_time: Set(end_time),
        request_time: Set(500),
        success: Set(true),
        fail_reason: Set(None),
        total_tokens: Set(total_tokens),
        api_key_name: Set("itest-key".to_string()),
    }
    .insert(db)
    .await
    .unwrap();
}

async fn setup_app() -> (axum::Router, DatabaseConnection) {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    // 虚拟模型 vm-1 配置 3 个成员：A/gpt-4o（启用）、A/deepseek-v3（启用）、
    // B/claude-sonnet（停用）；vm-2 配置 1 个成员。
    seed_virtual_model(&db, 1, "vm-1").await;
    seed_virtual_model(&db, 2, "vm-2").await;
    seed_provider(&db, 1, "供应商A").await;
    seed_provider(&db, 2, "供应商B").await;
    seed_provider_model(&db, 1, 1, "gpt-4o").await;
    seed_provider_model(&db, 2, 1, "deepseek-v3").await;
    seed_provider_model(&db, 3, 2, "claude-sonnet").await;
    seed_provider_model(&db, 4, 2, "kimi-k2").await;
    seed_member(&db, 1, 1, 1, true).await; // vm-1: A/gpt-4o
    seed_member(&db, 2, 1, 2, true).await; // vm-1: A/deepseek-v3
    seed_member(&db, 3, 1, 3, false).await; // vm-1: B/claude-sonnet（停用）
    seed_member(&db, 4, 2, 4, true).await; // vm-2: B/kimi-k2
    scheduler.start().await.unwrap();
    let app = common::build_authed_app(db.clone(), scheduler, log_tx).await;
    (app, db)
}

async fn get_json(app: axum::Router, uri: &str) -> (u16, serde_json::Value) {
    let request: Request<Body> = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    let status = response.status().as_u16();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    (status, json)
}

#[tokio::test]
async fn test_member_rank_returns_all_configured_members() {
    let (app, db) = setup_app().await;
    // vm-1 下三笔请求：A/gpt-4o 两笔（含流式 ttft=100/300、token 150+250）、
    // A/deepseek-v3 一笔（token 100）；B/claude-sonnet 无流量（停用成员）。
    insert_request(
        &db,
        "m1",
        1,
        1,
        "gpt-4o",
        T0,
        Some(150),
        true,
        Some(100),
        Some(100),
        50.0,
        Some(100),
        40,
    )
    .await;
    insert_request(
        &db,
        "m2",
        1,
        1,
        "gpt-4o",
        T0 + 1,
        Some(250),
        true,
        Some(300),
        Some(300),
        100.0,
        Some(100),
        0,
    )
    .await;
    insert_request(
        &db,
        "m3",
        1,
        1,
        "deepseek-v3",
        T0,
        Some(100),
        false,
        None,
        None,
        0.0,
        Some(100),
        0,
    )
    .await;
    // vm-2 的请求不应出现在 vm-1 的结果里。
    insert_request(
        &db,
        "m4",
        2,
        2,
        "kimi-k2",
        T0,
        Some(500),
        true,
        Some(200),
        Some(200),
        80.0,
        Some(100),
        0,
    )
    .await;

    let (status, json) = get_json(
        app,
        &format!(
            "/api/stats/virtual-model-member-rank?virtualModelId=1&startTime={T0}&endTime={}",
            T0 + 2
        ),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(json["code"], "0");

    let items = json["data"]["items"].as_array().unwrap();
    // 三个配置成员全量返回（含停用、无流量成员）。
    assert_eq!(items.len(), 3);

    // 默认 totalTokens 降序：gpt-4o(400) 最前。
    let by_model: Vec<&serde_json::Value> = items.iter().collect();
    let gpt = by_model.iter().find(|i| i["modelId"] == "gpt-4o").unwrap();
    assert_eq!(gpt["providerId"], 1);
    assert_eq!(gpt["providerName"], "供应商A");
    assert_eq!(gpt["memberEnable"], true);
    assert_eq!(gpt["requestCount"], 2);
    assert_eq!(gpt["totalTokens"], 400.0);
    assert_eq!(gpt["ttft"], 200.0); // (100+300)/2
    assert!((gpt["tps"].as_f64().unwrap() - 80.0).abs() < 0.001); // 400/(100/50+300/100)=400/5=80

    let ds = by_model
        .iter()
        .find(|i| i["modelId"] == "deepseek-v3")
        .unwrap();
    assert_eq!(ds["memberEnable"], true);
    assert_eq!(ds["requestCount"], 1);
    assert_eq!(ds["totalTokens"], 100.0);

    // 停用且无流量的成员：指标全 0。
    let claude = by_model
        .iter()
        .find(|i| i["modelId"] == "claude-sonnet")
        .unwrap();
    assert_eq!(claude["memberEnable"], false);
    assert_eq!(claude["requestCount"], 0);
    assert_eq!(claude["totalTokens"], 0.0);

    // vm-2 的 kimi-k2 不应出现。
    assert!(by_model.iter().all(|i| i["modelId"] != "kimi-k2"));
}

#[tokio::test]
async fn test_member_rank_missing_virtual_model_id_rejected() {
    let (app, _db) = setup_app().await;
    let (status, json) = get_json(
        app,
        &format!(
            "/api/stats/virtual-model-member-rank?startTime={T0}&endTime={}",
            T0 + 1
        ),
    )
    .await;
    assert_eq!(status, 400);
    assert_eq!(json["code"], "INVALID_INPUT");
}

#[tokio::test]
async fn test_member_rank_sort_by_and_order() {
    let (app, db) = setup_app().await;
    insert_request(
        &db,
        "s1",
        1,
        1,
        "gpt-4o",
        T0,
        Some(150),
        true,
        Some(100),
        Some(100),
        50.0,
        Some(100),
        40,
    )
    .await;
    insert_request(
        &db,
        "s2",
        1,
        1,
        "deepseek-v3",
        T0,
        Some(250),
        true,
        Some(500),
        Some(200),
        100.0,
        Some(100),
        0,
    )
    .await;

    // ttft 升序（默认）：gpt-4o(100) 在前；无流量的 claude-sonnet 排最后。
    let (status, json) = get_json(
        app.clone(),
        &format!("/api/stats/virtual-model-member-rank?virtualModelId=1&sortBy=ttft&startTime={T0}&endTime={}", T0 + 1),
    )
    .await;
    assert_eq!(status, 200);
    let items = json["data"]["items"].as_array().unwrap();
    assert_eq!(items[0]["modelId"], "gpt-4o");
    assert_eq!(items.last().unwrap()["modelId"], "claude-sonnet");

    // ttft 降序：deepseek-v3(500) 在前；无流量成员仍排最后。
    let (status, json) = get_json(
        app,
        &format!("/api/stats/virtual-model-member-rank?virtualModelId=1&sortBy=ttft&sortOrder=desc&startTime={T0}&endTime={}", T0 + 1),
    )
    .await;
    assert_eq!(status, 200);
    let items = json["data"]["items"].as_array().unwrap();
    assert_eq!(items[0]["modelId"], "deepseek-v3");
    assert_eq!(items.last().unwrap()["modelId"], "claude-sonnet");
}

#[tokio::test]
async fn test_member_rank_requires_auth() {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    scheduler.start().await.unwrap();
    let app = common::build_app(db, scheduler, log_tx);
    let (status, _json) = get_json(
        app,
        &format!(
            "/api/stats/virtual-model-member-rank?virtualModelId=1&startTime={T0}&endTime={}",
            T0 + 1
        ),
    )
    .await;
    assert_eq!(status, 401);
}

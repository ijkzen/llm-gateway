mod common;

use axum::body::Body;
use axum::http::Request;
use sea_orm::{ActiveModelTrait, DatabaseConnection, Set};
use tower::ServiceExt;

use llm_gateway::entity::provider as provider_entity;
use llm_gateway::entity::request as request_entity;

/// 窗口起点：固定锚点，避免与「当前时间」的耦合。
const T0: i64 = 1_700_000_000_000;

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
    }
    .insert(db)
    .await
    .unwrap();
}

struct SeedRow {
    request_id: String,
    provider_id: i32,
    model_id: String,
    start_time: i64,
    stream: bool,
    ttft: Option<i64>,
    input_tokens: Option<i64>,
    input_cache_tokens: i64,
    output_tokens: Option<i64>,
    tps: f64,
    total_tokens: Option<i64>,
    request_time: i64,
    success: bool,
}

async fn insert_request(db: &DatabaseConnection, row: SeedRow) {
    let end_time = row.start_time + row.request_time;
    request_entity::ActiveModel {
        request_id: Set(row.request_id),
        virtual_model_id: Set(1),
        provider_id: Set(row.provider_id),
        model_id: Set(row.model_id),
        stream: Set(row.stream),
        ttft: Set(row.ttft),
        input_tokens: Set(row.input_tokens),
        input_cache_tokens: Set(row.input_cache_tokens),
        input_cache_rate: Set(0.0),
        output_tokens: Set(row.output_tokens),
        output_tokens_time: Set(None),
        tps: Set(row.tps),
        start_time: Set(row.start_time),
        end_time: Set(end_time),
        request_time: Set(row.request_time),
        success: Set(row.success),
        fail_reason: Set(None),
        total_tokens: Set(row.total_tokens),
        api_key_name: Set("itest-key".to_string()),
    }
    .insert(db)
    .await
    .unwrap();
}

async fn setup_app() -> (axum::Router, DatabaseConnection) {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    seed_provider(&db, 1, "供应商A").await;
    seed_provider(&db, 2, "供应商B").await;
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
async fn test_model_metrics_aggregates_single_model() {
    let (app, db) = setup_app().await;
    // 供应商 A/gpt-4o：两笔成功（流式 ttft=100/300、token 150+250、output 100/300 tps 50/100、
    // 输入 100/100 缓存 40/0）+ 一笔失败（排除）；供应商 A/other 一笔（不应计入）。
    for row in [
        SeedRow {
            request_id: "mm1".into(),
            provider_id: 1,
            model_id: "gpt-4o".into(),
            start_time: T0,
            stream: true,
            ttft: Some(100),
            input_tokens: Some(100),
            input_cache_tokens: 40,
            output_tokens: Some(100),
            tps: 50.0,
            total_tokens: Some(150),
            request_time: 1000,
            success: true,
        },
        SeedRow {
            request_id: "mm2".into(),
            provider_id: 1,
            model_id: "gpt-4o".into(),
            start_time: T0 + 1,
            stream: true,
            ttft: Some(300),
            input_tokens: Some(100),
            input_cache_tokens: 0,
            output_tokens: Some(300),
            tps: 100.0,
            total_tokens: Some(250),
            request_time: 2000,
            success: true,
        },
        SeedRow {
            request_id: "mm3".into(),
            provider_id: 1,
            model_id: "gpt-4o".into(),
            start_time: T0 + 2,
            stream: false,
            ttft: None,
            input_tokens: None,
            input_cache_tokens: 0,
            output_tokens: None,
            tps: 0.0,
            total_tokens: Some(999),
            request_time: 500,
            success: false,
        },
        SeedRow {
            request_id: "mm4".into(),
            provider_id: 1,
            model_id: "other-model".into(),
            start_time: T0,
            stream: false,
            ttft: None,
            input_tokens: None,
            input_cache_tokens: 0,
            output_tokens: None,
            tps: 0.0,
            total_tokens: Some(500),
            request_time: 500,
            success: true,
        },
    ] {
        insert_request(&db, row).await;
    }

    let (status, json) = get_json(
        app,
        &format!(
            "/api/stats/model-metrics?providerId=1&modelId=gpt-4o&startTime={T0}&endTime={}",
            T0 + 3
        ),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(json["code"], "0");

    let data = &json["data"];
    assert_eq!(data["providerId"], 1);
    assert_eq!(data["providerName"], "供应商A");
    assert_eq!(data["modelId"], "gpt-4o");
    assert_eq!(data["requestCount"], 2); // 失败行排除
    assert_eq!(data["totalTokens"], 400.0);
    assert_eq!(data["ttft"], 200.0); // (100+300)/2
    assert_eq!(data["requestTime"], 1500.0); // (1000+2000)/2
    assert!((data["tps"].as_f64().unwrap() - 80.0).abs() < 0.001); // 400/(100/50+300/100)=400/5=80
    assert!((data["cacheHitRate"].as_f64().unwrap() - 0.2).abs() < 0.001); // 40/200
}

#[tokio::test]
async fn test_model_metrics_missing_params_rejected() {
    let (app, _db) = setup_app().await;
    // 缺 modelId。
    let (status, json) = get_json(
        app.clone(),
        &format!(
            "/api/stats/model-metrics?providerId=1&startTime={T0}&endTime={}",
            T0 + 1
        ),
    )
    .await;
    assert_eq!(status, 400);
    assert_eq!(json["code"], "INVALID_INPUT");
    // 缺 providerId。
    let (status, _json) = get_json(
        app.clone(),
        &format!(
            "/api/stats/model-metrics?modelId=gpt-4o&startTime={T0}&endTime={}",
            T0 + 1
        ),
    )
    .await;
    assert_eq!(status, 400);
    // 缺时间。
    let (status, _json) =
        get_json(app, "/api/stats/model-metrics?providerId=1&modelId=gpt-4o").await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn test_model_metrics_requires_auth() {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    scheduler.start().await.unwrap();
    let app = common::build_app(db, scheduler, log_tx);
    let (status, _json) = get_json(
        app,
        &format!(
            "/api/stats/model-metrics?providerId=1&modelId=gpt-4o&startTime={T0}&endTime={}",
            T0 + 1
        ),
    )
    .await;
    assert_eq!(status, 401);
}

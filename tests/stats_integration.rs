mod common;

use axum::body::Body;
use axum::http::Request;
use sea_orm::{ActiveModelTrait, DatabaseConnection, Set};
use tower::ServiceExt;

use llm_gateway::entity::provider as provider_entity;
use llm_gateway::entity::request as request_entity;

const HOUR_MS: i64 = 3_600_000;

/// 默认种子供应商（name 唯一，request 记录关联的 provider_id）。
const DEFAULT_PROVIDER_ID: i32 = 1;
const DEFAULT_PROVIDER_NAME: &str = "测试供应商";

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
    }
    .insert(db)
    .await
    .unwrap();
}

struct SeedRow {
    request_id: String,
    provider_id: i32,
    model_id: String,
    success: bool,
    start_time: i64,
    input_tokens: Option<i64>,
    input_cache_tokens: i64,
    total_tokens: Option<i64>,
}

async fn insert_request(db: &DatabaseConnection, row: SeedRow) {
    let end_time = row.start_time + 500;
    request_entity::ActiveModel {
        request_id: Set(row.request_id),
        virtual_model_id: Set(1),
        provider_id: Set(row.provider_id),
        model_id: Set(row.model_id),
        stream: Set(false),
        ttft: Set(None),
        input_tokens: Set(row.input_tokens),
        input_cache_tokens: Set(row.input_cache_tokens),
        input_cache_rate: Set(0.0),
        output_tokens: Set(None),
        output_tokens_time: Set(None),
        tps: Set(0.0),
        start_time: Set(row.start_time),
        end_time: Set(end_time),
        request_time: Set(500),
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
    seed_provider(&db, DEFAULT_PROVIDER_ID, DEFAULT_PROVIDER_NAME).await;
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
async fn test_summary_empty_table_returns_zeros() {
    let (app, _db) = setup_app().await;

    let (status, json) = get_json(app, "/api/stats/summary").await;
    assert_eq!(status, 200);

    let data = &json["data"];
    assert_eq!(data["totalRequests"], 0);
    assert_eq!(data["successRate"], 0.0);
    assert_eq!(data["totalTokens"], 0);
    assert_eq!(data["cacheHitRate"], 0.0);
}

#[tokio::test]
async fn test_summary_aggregates_all_history() {
    let (app, db) = setup_app().await;
    let now = chrono::Utc::now().timestamp_millis();
    // 一条超出 24h 窗口的旧数据，summary 仍应计入。
    let old = now - 48 * HOUR_MS;

    for (i, row) in [
        SeedRow {
            request_id: "r1".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            success: true,
            start_time: now,
            input_tokens: Some(100),
            input_cache_tokens: 40,
            total_tokens: Some(150),
        },
        SeedRow {
            request_id: "r2".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            success: true,
            start_time: now,
            input_tokens: Some(100),
            input_cache_tokens: 20,
            total_tokens: Some(150),
        },
        SeedRow {
            request_id: "r3".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "claude-sonnet".into(),
            success: false,
            start_time: old,
            input_tokens: Some(200),
            input_cache_tokens: 60,
            total_tokens: Some(300),
        },
        // usage 缺失的一行：token 统计应忽略 NULL。
        SeedRow {
            request_id: "r4".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gemini-pro".into(),
            success: true,
            start_time: now,
            input_tokens: None,
            input_cache_tokens: 0,
            total_tokens: None,
        },
    ]
    .into_iter()
    .enumerate()
    {
        let _ = i;
        insert_request(&db, row).await;
    }

    let (status, json) = get_json(app, "/api/stats/summary").await;
    assert_eq!(status, 200);

    let data = &json["data"];
    assert_eq!(data["totalRequests"], 4);
    assert_eq!(data["successRate"], 0.75);
    assert_eq!(data["totalTokens"], 600);
    // 加权缓存命中率：(40+20+60) / (100+100+200) = 0.3
    assert_eq!(data["cacheHitRate"], 0.3);
}

#[tokio::test]
async fn test_charts_returns_24_zero_filled_buckets() {
    let (app, _db) = setup_app().await;

    let (status, json) = get_json(app, "/api/stats/charts").await;
    assert_eq!(status, 200);

    let data = &json["data"];
    let call_trend = data["callTrend"].as_array().unwrap();
    let token_trend = data["tokenTrend"].as_array().unwrap();
    assert_eq!(call_trend.len(), 24);
    assert_eq!(token_trend.len(), 24);
    for point in call_trend.iter().chain(token_trend.iter()) {
        assert_eq!(point["value"], 0);
    }
    // 桶按时间升序、相邻间隔一小时。
    let starts: Vec<i64> = call_trend
        .iter()
        .map(|p| p["bucketStart"].as_i64().unwrap())
        .collect();
    for w in starts.windows(2) {
        assert_eq!(w[1] - w[0], HOUR_MS);
    }
    assert_eq!(data["callByModel"].as_array().unwrap().len(), 0);
    assert_eq!(data["tokenByModel"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn test_charts_aggregates_by_hour_and_model() {
    let (app, db) = setup_app().await;
    let now = chrono::Utc::now().timestamp_millis();
    let current_bucket_start = (now / HOUR_MS) * HOUR_MS;
    let prev_bucket_start = current_bucket_start - HOUR_MS;
    let outside_window = current_bucket_start - 24 * HOUR_MS;

    let rows = vec![
        // 当前小时：gpt-4o 两笔（含一笔失败，仍计入调用数）。
        SeedRow {
            request_id: "c1".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            success: true,
            start_time: current_bucket_start + 1,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(100),
        },
        SeedRow {
            request_id: "c2".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            success: false,
            start_time: current_bucket_start + 2,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: None,
        },
        // 上一小时：gpt-4o 一笔 + claude 一笔。
        SeedRow {
            request_id: "c3".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            success: true,
            start_time: prev_bucket_start + 1,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(50),
        },
        SeedRow {
            request_id: "c4".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "claude-sonnet".into(),
            success: true,
            start_time: prev_bucket_start + 2,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(30),
        },
        // 窗口外：不应出现在任何图表数据中。
        SeedRow {
            request_id: "c5".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "old-model".into(),
            success: true,
            start_time: outside_window,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(999),
        },
    ];
    for row in rows {
        insert_request(&db, row).await;
    }

    let (status, json) = get_json(app, "/api/stats/charts").await;
    assert_eq!(status, 200);
    let data = &json["data"];

    let call_trend = data["callTrend"].as_array().unwrap();
    assert_eq!(call_trend.len(), 24);
    let current = &call_trend[23];
    let prev = &call_trend[22];
    assert_eq!(current["bucketStart"], current_bucket_start);
    assert_eq!(current["value"], 2);
    assert_eq!(prev["value"], 2);
    assert!(call_trend[..22].iter().all(|p| p["value"] == 0));

    let token_trend = data["tokenTrend"].as_array().unwrap();
    assert_eq!(token_trend[23]["value"], 100);
    assert_eq!(token_trend[22]["value"], 80);

    let call_by_model = data["callByModel"].as_array().unwrap();
    assert_eq!(call_by_model.len(), 2);
    let gpt = call_by_model
        .iter()
        .find(|m| m["modelId"] == "gpt-4o")
        .unwrap();
    assert_eq!(gpt["value"], 3);
    assert_eq!(gpt["providerName"], DEFAULT_PROVIDER_NAME);
    let claude = call_by_model
        .iter()
        .find(|m| m["modelId"] == "claude-sonnet")
        .unwrap();
    assert_eq!(claude["value"], 1);
    assert_eq!(claude["providerName"], DEFAULT_PROVIDER_NAME);

    let token_by_model = data["tokenByModel"].as_array().unwrap();
    let gpt_tokens = token_by_model
        .iter()
        .find(|m| m["modelId"] == "gpt-4o")
        .unwrap();
    assert_eq!(gpt_tokens["value"], 150);
    assert_eq!(gpt_tokens["providerName"], DEFAULT_PROVIDER_NAME);
}

#[tokio::test]
async fn test_stats_requires_auth() {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    scheduler.start().await.unwrap();
    // 未注入凭证的 app：/api/stats 应被会话中间件拦截。
    let app = common::build_app(db, scheduler, log_tx);

    let request: Request<Body> = Request::builder()
        .method("GET")
        .uri("/api/stats/summary")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), 401);
}

#[tokio::test]
async fn test_charts_splits_same_model_across_providers() {
    let (app, db) = setup_app().await;
    let now = chrono::Utc::now().timestamp_millis();
    // 第二个供应商注册同名模型：分布应按 (provider, model) 拆成两行。
    seed_provider(&db, 2, "第二供应商").await;

    for (i, row) in [
        SeedRow {
            request_id: "p1".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            success: true,
            start_time: now,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(50),
        },
        SeedRow {
            request_id: "p2".into(),
            provider_id: 2,
            model_id: "gpt-4o".into(),
            success: true,
            start_time: now,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(30),
        },
    ]
    .into_iter()
    .enumerate()
    {
        let _ = i;
        insert_request(&db, row).await;
    }

    let (status, json) = get_json(app, "/api/stats/charts").await;
    assert_eq!(status, 200);
    let data = &json["data"];

    let call_by_model = data["callByModel"].as_array().unwrap();
    assert_eq!(call_by_model.len(), 2);
    let first = call_by_model
        .iter()
        .find(|m| m["providerName"] == DEFAULT_PROVIDER_NAME && m["modelId"] == "gpt-4o")
        .unwrap();
    assert_eq!(first["value"], 1);
    let second = call_by_model
        .iter()
        .find(|m| m["providerName"] == "第二供应商" && m["modelId"] == "gpt-4o")
        .unwrap();
    assert_eq!(second["value"], 1);

    let token_by_model = data["tokenByModel"].as_array().unwrap();
    assert_eq!(token_by_model.len(), 2);
    let first_tokens = token_by_model
        .iter()
        .find(|m| m["providerName"] == DEFAULT_PROVIDER_NAME && m["modelId"] == "gpt-4o")
        .unwrap();
    assert_eq!(first_tokens["value"], 50);
    let second_tokens = token_by_model
        .iter()
        .find(|m| m["providerName"] == "第二供应商" && m["modelId"] == "gpt-4o")
        .unwrap();
    assert_eq!(second_tokens["value"], 30);
}

#[tokio::test]
async fn test_charts_provider_deleted_falls_back_to_empty_name() {
    let (app, db) = setup_app().await;
    let now = chrono::Utc::now().timestamp_millis();
    // 供应商已删除（provider 表无该行）：providerName 应为空串，仍按 model_id 聚合。
    for (i, row) in [
        SeedRow {
            request_id: "d1".into(),
            provider_id: 99,
            model_id: "ghost-model".into(),
            success: true,
            start_time: now,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(10),
        },
        SeedRow {
            request_id: "d2".into(),
            provider_id: 99,
            model_id: "ghost-model".into(),
            success: true,
            start_time: now,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(20),
        },
    ]
    .into_iter()
    .enumerate()
    {
        let _ = i;
        insert_request(&db, row).await;
    }

    let (status, json) = get_json(app, "/api/stats/charts").await;
    assert_eq!(status, 200);
    let data = &json["data"];

    let call_by_model = data["callByModel"].as_array().unwrap();
    assert_eq!(call_by_model.len(), 1);
    let ghost = &call_by_model[0];
    assert_eq!(ghost["providerName"], "");
    assert_eq!(ghost["modelId"], "ghost-model");
    assert_eq!(ghost["value"], 2);
}

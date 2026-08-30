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
    stream: bool,
    ttft: Option<i64>,
    input_tokens: Option<i64>,
    input_cache_tokens: i64,
    output_tokens: Option<i64>,
    output_tokens_time: Option<i64>,
    tps: f64,
    total_tokens: Option<i64>,
}

impl SeedRow {
    fn new(request_id: &str, provider_id: i32, model_id: &str, start_time: i64) -> Self {
        Self {
            request_id: request_id.to_string(),
            provider_id,
            model_id: model_id.to_string(),
            success: true,
            start_time,
            stream: false,
            ttft: None,
            input_tokens: None,
            input_cache_tokens: 0,
            output_tokens: None,
            output_tokens_time: None,
            tps: 0.0,
            total_tokens: None,
        }
    }
}

async fn insert_request(db: &DatabaseConnection, row: SeedRow) {
    let end_time = row.start_time + 500;
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
        output_tokens_time: Set(row.output_tokens_time),
        tps: Set(row.tps),
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
    // 三个供应商：A/B 有关联请求，C 无请求（不应出现在排行中）。
    for (id, name) in [(1, "供应商A"), (2, "供应商B"), (3, "供应商C")] {
        seed_provider(&db, id, name).await;
    }
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
async fn test_token_rank_aggregates_and_sorts() {
    let (app, db) = setup_app().await;
    // A 两笔成功（150+250）、一笔失败；B 一笔成功；失败行不应计入。
    for row in [
        SeedRow {
            request_id: "r1".into(),
            total_tokens: Some(150),
            ..SeedRow::new("r1", 1, "gpt-4o", T0)
        },
        SeedRow {
            request_id: "r2".into(),
            total_tokens: Some(250),
            ..SeedRow::new("r2", 1, "gpt-4o", T0 + 1)
        },
        SeedRow {
            request_id: "r3".into(),
            success: false,
            total_tokens: Some(999),
            ..SeedRow::new("r3", 1, "gpt-4o", T0 + 2)
        },
        SeedRow {
            request_id: "r4".into(),
            total_tokens: Some(100),
            ..SeedRow::new("r4", 2, "claude-sonnet", T0)
        },
        // usage 缺失（total_tokens NULL）的一行：不应计入 SUM（COALESCE 忽略）。
        SeedRow {
            request_id: "r5".into(),
            total_tokens: None,
            ..SeedRow::new("r5", 2, "claude-sonnet", T0 + 1)
        },
    ] {
        insert_request(&db, row).await;
    }

    let (status, json) = get_json(
        app,
        &format!("/api/stats/provider-rank?metric=token&startTime={T0}&endTime={}", T0 + 3),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(json["code"], "0");

    let data = &json["data"];
    assert_eq!(data["metric"], "token");
    assert_eq!(data["startTime"], T0);
    assert_eq!(data["endTime"], T0 + 3);

    let items = data["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    // 降序：A(400) 在前，B(100) 在后。
    assert_eq!(items[0]["providerName"], "供应商A");
    assert_eq!(items[0]["value"], 400.0);
    // request_count 只统计成功请求（失败行被 success=1 过滤）。
    assert_eq!(items[0]["requestCount"], 2);
    assert_eq!(items[1]["providerName"], "供应商B");
    assert_eq!(items[1]["value"], 100.0);
}

#[tokio::test]
async fn test_token_rank_respects_half_open_window() {
    let (app, db) = setup_app().await;
    insert_request(
        &db,
        SeedRow {
            request_id: "r1".into(),
            total_tokens: Some(100),
            ..SeedRow::new("r1", 1, "gpt-4o", T0 - 1)
        },
    )
    .await;
    // endTime 边界：start_time == endTime 的行不属于窗口。
    insert_request(
        &db,
        SeedRow {
            request_id: "r2".into(),
            total_tokens: Some(100),
            ..SeedRow::new("r2", 1, "gpt-4o", T0 + 1_000)
        },
    )
    .await;
    // 供应商 B 的请求在窗口内。
    insert_request(
        &db,
        SeedRow {
            request_id: "r3".into(),
            total_tokens: Some(50),
            ..SeedRow::new("r3", 2, "claude-sonnet", T0 + 500)
        },
    )
    .await;

    // 窗口 [T0, T0+1000)：r1（T0-1）与 r2（T0+1000）都应被排除。
    let (status, json) = get_json(
        app,
        &format!("/api/stats/provider-rank?metric=token&startTime={T0}&endTime={}", T0 + 1_000),
    )
    .await;
    assert_eq!(status, 200);
    let items = json["data"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["providerName"], "供应商B");
    assert_eq!(items[0]["value"], 50.0);
}

#[tokio::test]
async fn test_token_rank_limits_to_ten() {
    let (app, db) = setup_app().await;
    // 11 个供应商各一笔请求，token 递增保证排序稳定。
    for i in 1..=11 {
        seed_provider(&db, 100 + i, &format!("供应商{i}")).await;
        insert_request(
            &db,
            SeedRow {
                request_id: format!("r{i}"),
                provider_id: 100 + i,
                total_tokens: Some(i as i64 * 10),
                ..SeedRow::new(&format!("r{i}"), 100 + i, "gpt-4o", T0)
            },
        )
        .await;
    }

    let (status, json) = get_json(
        app,
        &format!("/api/stats/provider-rank?metric=token&startTime={T0}&endTime={}", T0 + 1),
    )
    .await;
    assert_eq!(status, 200);
    let items = json["data"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 10);
    // 第一名是 token 最高的 供应商11。
    assert_eq!(items[0]["providerName"], "供应商11");
}

#[tokio::test]
async fn test_ttft_rank_only_streaming_with_ttft() {
    let (app, db) = setup_app().await;
    // A：两条流式（100ms、300ms）→ 均值 200。
    // B：一条流式（500ms）、一条非流式（ttft NULL 应排除）。
    // C：一条流式 ttft NULL（应排除）。
    for row in [
        SeedRow {
            request_id: "r1".into(),
            stream: true,
            ttft: Some(100),
            ..SeedRow::new("r1", 1, "gpt-4o", T0)
        },
        SeedRow {
            request_id: "r2".into(),
            stream: true,
            ttft: Some(300),
            ..SeedRow::new("r2", 1, "gpt-4o", T0 + 1)
        },
        SeedRow {
            request_id: "r3".into(),
            stream: true,
            ttft: Some(500),
            ..SeedRow::new("r3", 2, "claude-sonnet", T0)
        },
        SeedRow {
            request_id: "r4".into(),
            stream: false,
            ttft: None,
            ..SeedRow::new("r4", 2, "claude-sonnet", T0 + 1)
        },
        SeedRow {
            request_id: "r5".into(),
            stream: true,
            ttft: None,
            ..SeedRow::new("r5", 3, "gemini-pro", T0)
        },
    ] {
        insert_request(&db, row).await;
    }

    let (status, json) = get_json(
        app,
        &format!("/api/stats/provider-rank?metric=ttft&startTime={T0}&endTime={}", T0 + 2),
    )
    .await;
    assert_eq!(status, 200);

    let items = json["data"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    // 升序：A(200) 在前，B(500) 在后；C 因全部被排除而不出现。
    assert_eq!(items[0]["providerName"], "供应商A");
    assert_eq!(items[0]["value"], 200.0);
    assert_eq!(items[1]["providerName"], "供应商B");
    assert_eq!(items[1]["value"], 500.0);
}

#[tokio::test]
async fn test_tps_rank_weighted_average() {
    let (app, db) = setup_app().await;
    // A：两笔（output=100, tps=50 → 耗时 2s；output=300, tps=100 → 耗时 3s）
    //    Σ输出=400，Σ耗时=5 → tps=80。
    // B：一笔（output=200, tps=200 → 耗时 1s）→ tps=200，应排第一。
    // C：一笔 output_tokens NULL、一笔 tps=0（应被排除出分母分子，值记 0）。
    for row in [
        SeedRow {
            request_id: "r1".into(),
            output_tokens: Some(100),
            tps: 50.0,
            ..SeedRow::new("r1", 1, "gpt-4o", T0)
        },
        SeedRow {
            request_id: "r2".into(),
            output_tokens: Some(300),
            tps: 100.0,
            ..SeedRow::new("r2", 1, "gpt-4o", T0 + 1)
        },
        SeedRow {
            request_id: "r3".into(),
            output_tokens: Some(200),
            tps: 200.0,
            ..SeedRow::new("r3", 2, "claude-sonnet", T0)
        },
        SeedRow {
            request_id: "r4".into(),
            output_tokens: None,
            tps: 10.0,
            ..SeedRow::new("r4", 3, "gemini-pro", T0)
        },
        SeedRow {
            request_id: "r5".into(),
            output_tokens: Some(100),
            tps: 0.0,
            ..SeedRow::new("r5", 3, "gemini-pro", T0 + 1)
        },
    ] {
        insert_request(&db, row).await;
    }

    let (status, json) = get_json(
        app,
        &format!("/api/stats/provider-rank?metric=tps&startTime={T0}&endTime={}", T0 + 2),
    )
    .await;
    assert_eq!(status, 200);

    let items = json["data"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 3);
    assert_eq!(items[0]["providerName"], "供应商B");
    assert_eq!(items[0]["value"], 200.0);
    assert_eq!(items[1]["providerName"], "供应商A");
    assert_eq!(items[1]["value"], 80.0);
    // C 无可用的耗时/输出，值记 0，排最后。
    assert_eq!(items[2]["providerName"], "供应商C");
    assert_eq!(items[2]["value"], 0.0);
}

#[tokio::test]
async fn test_provider_deleted_shows_empty_name() {
    let (app, db) = setup_app().await;
    insert_request(
        &db,
        SeedRow {
            request_id: "r1".into(),
            total_tokens: Some(100),
            ..SeedRow::new("r1", 999, "gpt-4o", T0)
        },
    )
    .await;

    let (status, json) = get_json(
        app,
        &format!("/api/stats/provider-rank?metric=token&startTime={T0}&endTime={}", T0 + 1),
    )
    .await;
    assert_eq!(status, 200);
    let items = json["data"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["providerName"], "");
}

#[tokio::test]
async fn test_invalid_metric_rejected() {
    let (app, _db) = setup_app().await;
    let (status, json) = get_json(
        app,
        &format!("/api/stats/provider-rank?metric=foo&startTime={T0}&endTime={}", T0 + 1),
    )
    .await;
    assert_eq!(status, 400);
    assert_eq!(json["code"], "INVALID_INPUT");
}

#[tokio::test]
async fn test_missing_params_rejected() {
    let (app, _db) = setup_app().await;
    // 缺 metric。
    let (status, _json) = get_json(app.clone(), "/api/stats/provider-rank?startTime=1&endTime=2").await;
    assert_eq!(status, 400);
    // 缺 startTime / endTime。
    let (status, _json) = get_json(app.clone(), "/api/stats/provider-rank?metric=token").await;
    assert_eq!(status, 400);
    // endTime <= startTime。
    let (status, _json) = get_json(
        app,
        "/api/stats/provider-rank?metric=token&startTime=200&endTime=200",
    )
    .await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn test_requires_auth() {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    scheduler.start().await.unwrap();
    // 未注入会话/Bearer 的应用，/api/* 应 401。
    let app = common::build_app(db, scheduler, log_tx);
    let (status, _json) = get_json(
        app,
        &format!("/api/stats/provider-rank?metric=token&startTime={T0}&endTime={}", T0 + 1),
    )
    .await;
    assert_eq!(status, 401);
}

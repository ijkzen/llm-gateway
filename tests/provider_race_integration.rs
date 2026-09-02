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
        failure_disabled: Set(false),
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
    request_time: i64,
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
            request_time: 500,
        }
    }
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
        output_tokens_time: Set(row.output_tokens_time),
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

/// 种入 A/B 两供应商的各指标种子数据，返回 (app, db)：
/// - A：r1(成功, 流式 ttft=100, 输入100/缓存40, 输出100 tps=50, token=150, rt=1000)
///   r2(成功, 流式 ttft=300, 输入100/缓存0, 输出300 tps=100, token=250, rt=2000)
///   r3(失败, token=999 —— 应被 success=1 排除)
/// - B：r4(成功, 流式 ttft=500, 输入200/缓存100, 输出200 tps=200, token=100, rt=3000)
///   r5(成功, 非流式 ttft NULL —— 只影响 ttft 分母，其余指标计入)
async fn seed_rank_data(db: &DatabaseConnection) {
    for row in [
        SeedRow {
            request_id: "r1".into(),
            stream: true,
            ttft: Some(100),
            input_tokens: Some(100),
            input_cache_tokens: 40,
            output_tokens: Some(100),
            tps: 50.0,
            total_tokens: Some(150),
            request_time: 1000,
            ..SeedRow::new("r1", 1, "gpt-4o", T0)
        },
        SeedRow {
            request_id: "r2".into(),
            stream: true,
            ttft: Some(300),
            input_tokens: Some(100),
            input_cache_tokens: 0,
            output_tokens: Some(300),
            tps: 100.0,
            total_tokens: Some(250),
            request_time: 2000,
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
            stream: true,
            ttft: Some(500),
            input_tokens: Some(200),
            input_cache_tokens: 100,
            output_tokens: Some(200),
            tps: 200.0,
            total_tokens: Some(100),
            request_time: 3000,
            ..SeedRow::new("r4", 2, "claude-sonnet", T0)
        },
        SeedRow {
            request_id: "r5".into(),
            stream: false,
            ttft: None,
            input_tokens: Some(200),
            input_cache_tokens: 0,
            output_tokens: None,
            tps: 0.0,
            total_tokens: Some(100),
            request_time: 3000,
            ..SeedRow::new("r5", 2, "claude-sonnet", T0 + 1)
        },
    ] {
        insert_request(db, row).await;
    }
}

#[tokio::test]
async fn test_rank_aggregates_all_six_metrics() {
    let (app, db) = setup_app().await;
    seed_rank_data(&db).await;

    let (status, json) = get_json(
        app,
        &format!("/api/stats/provider-rank?startTime={T0}&endTime={}", T0 + 3),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(json["code"], "0");
    assert_eq!(json["data"]["startTime"], T0);
    assert_eq!(json["data"]["endTime"], T0 + 3);

    let items = json["data"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    // 默认排序：totalTokens 降序 → A(400) 在前。
    let a = &items[0];
    let b = &items[1];
    assert_eq!(a["providerId"], 1);
    assert_eq!(a["providerName"], "供应商A");
    assert_eq!(a["requestCount"], 2); // 失败行排除
    assert_eq!(a["totalTokens"], 400.0);
    assert_eq!(a["ttft"], 200.0); // (100+300)/2
    assert_eq!(a["requestTime"], 1500.0); // (1000+2000)/2
    assert!((a["tps"].as_f64().unwrap() - 80.0).abs() < 0.001); // (100+300)/(100/50+300/100)
    assert!((a["cacheHitRate"].as_f64().unwrap() - 0.2).abs() < 0.001); // 40/200
    assert_eq!(b["providerId"], 2);
    assert_eq!(b["providerName"], "供应商B");
    assert_eq!(b["requestCount"], 2);
    assert_eq!(b["totalTokens"], 200.0);
    assert_eq!(b["ttft"], 500.0); // r5 非流式被排除
    assert_eq!(b["requestTime"], 3000.0);
    assert!((b["tps"].as_f64().unwrap() - 200.0).abs() < 0.001);
    assert!((b["cacheHitRate"].as_f64().unwrap() - 0.25).abs() < 0.001); // 100/400
}

#[tokio::test]
async fn test_rank_sort_by_and_order() {
    let (app, db) = setup_app().await;
    seed_rank_data(&db).await;

    // ttft 升序（默认方向）→ A(200) 在前。
    let (status, json) = get_json(
        app.clone(),
        &format!(
            "/api/stats/provider-rank?sortBy=ttft&startTime={T0}&endTime={}",
            T0 + 3
        ),
    )
    .await;
    assert_eq!(status, 200);
    let items = json["data"]["items"].as_array().unwrap();
    assert_eq!(items[0]["providerName"], "供应商A");
    assert_eq!(items[0]["ttft"], 200.0);

    // ttft 降序 → B(500) 在前。
    let (_status, json) = get_json(
        app.clone(),
        &format!(
            "/api/stats/provider-rank?sortBy=ttft&sortOrder=desc&startTime={T0}&endTime={}",
            T0 + 3
        ),
    )
    .await;
    let items = json["data"]["items"].as_array().unwrap();
    assert_eq!(items[0]["providerName"], "供应商B");

    // cacheHitRate 降序 → B(0.25) 在前。
    let (_status, json) = get_json(
        app.clone(),
        &format!(
            "/api/stats/provider-rank?sortBy=cacheHitRate&startTime={T0}&endTime={}",
            T0 + 3
        ),
    )
    .await;
    let items = json["data"]["items"].as_array().unwrap();
    assert_eq!(items[0]["providerName"], "供应商B");
    assert_eq!(items[0]["cacheHitRate"], 0.25);

    // requestCount 升序 → 都是 2，保持稳定（A 在前，default 排序稳定）。
    let (_status, json) = get_json(
        app,
        &format!(
            "/api/stats/provider-rank?sortBy=requestCount&sortOrder=asc&startTime={T0}&endTime={}",
            T0 + 3
        ),
    )
    .await;
    let items = json["data"]["items"].as_array().unwrap();
    assert_eq!(items[0]["providerName"], "供应商A");
    assert_eq!(items[1]["providerName"], "供应商B");
}

#[tokio::test]
async fn test_rank_respects_half_open_window() {
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
        &format!(
            "/api/stats/provider-rank?startTime={T0}&endTime={}",
            T0 + 1_000
        ),
    )
    .await;
    assert_eq!(status, 200);
    let items = json["data"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["providerName"], "供应商B");
    assert_eq!(items[0]["totalTokens"], 50.0);
}

#[tokio::test]
async fn test_rank_all_providers_returned_no_limit() {
    let (app, db) = setup_app().await;
    // 11 个供应商各一笔请求，验证不再截断到 Top 10。
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
        &format!("/api/stats/provider-rank?startTime={T0}&endTime={}", T0 + 1),
    )
    .await;
    assert_eq!(status, 200);
    let items = json["data"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 11);
    // 默认 totalTokens 降序：第一名是 token 最高的 供应商11。
    assert_eq!(items[0]["providerName"], "供应商11");
}

#[tokio::test]
async fn test_rank_provider_deleted_shows_empty_name() {
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
        &format!("/api/stats/provider-rank?startTime={T0}&endTime={}", T0 + 1),
    )
    .await;
    assert_eq!(status, 200);
    let items = json["data"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["providerName"], "");
}

#[tokio::test]
async fn test_rank_invalid_sort_by_rejected() {
    let (app, _db) = setup_app().await;
    let (status, json) = get_json(
        app,
        &format!(
            "/api/stats/provider-rank?sortBy=foo&startTime={T0}&endTime={}",
            T0 + 1
        ),
    )
    .await;
    assert_eq!(status, 400);
    assert_eq!(json["code"], "INVALID_INPUT");
}

#[tokio::test]
async fn test_rank_missing_params_rejected() {
    let (app, _db) = setup_app().await;
    // 缺 startTime / endTime。
    let (status, _json) = get_json(app.clone(), "/api/stats/provider-rank?sortBy=ttft").await;
    assert_eq!(status, 400);
    // endTime <= startTime。
    let (status, _json) = get_json(
        app,
        "/api/stats/provider-rank?sortBy=ttft&startTime=200&endTime=200",
    )
    .await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn test_rank_requires_auth() {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    scheduler.start().await.unwrap();
    // 未注入会话/Bearer 的应用，/api/* 应 401。
    let app = common::build_app(db, scheduler, log_tx);
    let (status, _json) = get_json(
        app,
        &format!("/api/stats/provider-rank?startTime={T0}&endTime={}", T0 + 1),
    )
    .await;
    assert_eq!(status, 401);
}

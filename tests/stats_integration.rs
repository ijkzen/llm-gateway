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

#[tokio::test]
async fn test_charts_with_window_and_provider_filter() {
    let (app, db) = setup_app().await;
    // 固定窗口 [T0, T0+3h)，两个供应商各一笔；带 providerId 应只返回该供应商。
    let t0 = (1_700_000_000_000i64 / HOUR_MS) * HOUR_MS;
    for row in [
        SeedRow {
            request_id: "w1".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            success: true,
            start_time: t0 + 1,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(100),
        },
        // 供应商 2（另一个 provider）的请求，不应出现在 providerId=1 过滤结果中。
        SeedRow {
            request_id: "w2".into(),
            provider_id: 2,
            model_id: "claude-sonnet".into(),
            success: true,
            start_time: t0 + 1,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(200),
        },
        // 窗口外请求，不应出现。
        SeedRow {
            request_id: "w3".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            success: true,
            start_time: t0 - 1,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(999),
        },
    ] {
        insert_request(&db, row).await;
    }

    // 带 startTime/endTime + providerId 过滤。
    let (status, json) = get_json(
        app.clone(),
        &format!(
            "/api/stats/charts?startTime={t0}&endTime={}&providerId={}",
            t0 + 3 * HOUR_MS,
            DEFAULT_PROVIDER_ID
        ),
    )
    .await;
    assert_eq!(status, 200);
    let data = &json["data"];
    let call_trend = data["callTrend"].as_array().unwrap();
    // 3 小时窗口 → 3 个桶（小时粒度）。
    assert_eq!(call_trend.len(), 3);
    let total_calls: i64 = call_trend
        .iter()
        .map(|p| p["value"].as_i64().unwrap())
        .sum();
    assert_eq!(total_calls, 1); // 只有 w1
    let call_by_model = data["callByModel"].as_array().unwrap();
    assert_eq!(call_by_model.len(), 1);
    assert_eq!(call_by_model[0]["modelId"], "gpt-4o");
    assert_eq!(call_by_model[0]["value"], 1);

    // 不带 providerId：两个供应商都出现。
    let (status, json) = get_json(
        app,
        &format!(
            "/api/stats/charts?startTime={t0}&endTime={}",
            t0 + 3 * HOUR_MS
        ),
    )
    .await;
    assert_eq!(status, 200);
    let data = &json["data"];
    let call_by_model = data["callByModel"].as_array().unwrap();
    assert_eq!(call_by_model.len(), 2);
}

#[tokio::test]
async fn test_charts_day_granularity_for_long_window() {
    let (app, db) = setup_app().await;
    // 5 天窗口 → 天桶粒度（5 个桶），验证 >48h 用天桶。
    // t0 对齐到整天起点（小时对齐后再按 24h 对齐），保证桶边界整齐。
    let t0 = ((1_700_000_000_000i64 / HOUR_MS) / 24) * 24 * HOUR_MS;
    let day_ms = 24 * HOUR_MS;
    for row in [
        SeedRow {
            request_id: "d1".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            success: true,
            start_time: t0 + 1,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(100),
        },
        SeedRow {
            request_id: "d2".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            success: true,
            start_time: t0 + 2 * day_ms + 1,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(200),
        },
    ] {
        insert_request(&db, row).await;
    }

    let (status, json) = get_json(
        app,
        &format!(
            "/api/stats/charts?startTime={t0}&endTime={}",
            t0 + 5 * day_ms
        ),
    )
    .await;
    assert_eq!(status, 200);
    let data = &json["data"];
    let call_trend = data["callTrend"].as_array().unwrap();
    assert_eq!(call_trend.len(), 5);
    // 相邻桶间隔一天。
    let starts: Vec<i64> = call_trend
        .iter()
        .map(|p| p["bucketStart"].as_i64().unwrap())
        .collect();
    for w in starts.windows(2) {
        assert_eq!(w[1] - w[0], day_ms);
    }
    let values: Vec<i64> = call_trend
        .iter()
        .map(|p| p["value"].as_i64().unwrap())
        .collect();
    assert_eq!(values, vec![1, 0, 1, 0, 0]);
}

#[tokio::test]
async fn test_charts_virtual_model_filter() {
    let (app, db) = setup_app().await;
    // 固定窗口内两个虚拟模型各一笔；带 virtualModelId 只返回该虚拟模型。
    let t0 = (1_700_000_000_000i64 / HOUR_MS) * HOUR_MS;
    // 直接插 request（virtual_model_id=2 的行需要绕过 insert_request 的硬编码 1）。
    let end_time = t0 + 500;
    for (rid, vm_id, model) in [("vmf1", 1, "gpt-4o"), ("vmf2", 2, "claude-sonnet")] {
        request_entity::ActiveModel {
            request_id: Set(rid.to_string()),
            virtual_model_id: Set(vm_id),
            provider_id: Set(DEFAULT_PROVIDER_ID),
            model_id: Set(model.to_string()),
            stream: Set(false),
            ttft: Set(None),
            input_tokens: Set(Some(10)),
            input_cache_tokens: Set(0),
            input_cache_rate: Set(0.0),
            output_tokens: Set(None),
            output_tokens_time: Set(None),
            tps: Set(0.0),
            start_time: Set(t0 + 1),
            end_time: Set(end_time),
            request_time: Set(500),
            success: Set(true),
            fail_reason: Set(None),
            total_tokens: Set(Some(100)),
            api_key_name: Set("itest-key".to_string()),
        }
        .insert(&db)
        .await
        .unwrap();
    }

    // 带 virtualModelId=1：只返回 gpt-4o。
    let (status, json) = get_json(
        app.clone(),
        &format!(
            "/api/stats/charts?startTime={t0}&endTime={}&virtualModelId=1",
            t0 + 2 * HOUR_MS
        ),
    )
    .await;
    assert_eq!(status, 200);
    let data = &json["data"];
    let call_by_model = data["callByModel"].as_array().unwrap();
    assert_eq!(call_by_model.len(), 1);
    assert_eq!(call_by_model[0]["modelId"], "gpt-4o");

    // 不带过滤：两个虚拟模型的模型都出现。
    let (status, json) = get_json(
        app,
        &format!(
            "/api/stats/charts?startTime={t0}&endTime={}",
            t0 + 2 * HOUR_MS
        ),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(json["data"]["callByModel"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn test_charts_provider_and_model_filter() {
    let (app, db) = setup_app().await;
    // 固定窗口内：供应商 A 两个模型、B 一个模型；带 providerId+modelId 只返回该模型。
    let t0 = (1_700_000_000_000i64 / HOUR_MS) * HOUR_MS;
    let end_time = t0 + 500;
    for (rid, pid, model) in [
        ("pmf1", DEFAULT_PROVIDER_ID, "gpt-4o"),
        ("pmf2", DEFAULT_PROVIDER_ID, "deepseek-v3"),
        ("pmf3", 2, "claude-sonnet"),
    ] {
        request_entity::ActiveModel {
            request_id: Set(rid.to_string()),
            virtual_model_id: Set(1),
            provider_id: Set(pid),
            model_id: Set(model.to_string()),
            stream: Set(false),
            ttft: Set(None),
            input_tokens: Set(Some(10)),
            input_cache_tokens: Set(0),
            input_cache_rate: Set(0.0),
            output_tokens: Set(None),
            output_tokens_time: Set(None),
            tps: Set(0.0),
            start_time: Set(t0 + 1),
            end_time: Set(end_time),
            request_time: Set(500),
            success: Set(true),
            fail_reason: Set(None),
            total_tokens: Set(Some(100)),
            api_key_name: Set("itest-key".to_string()),
        }
        .insert(&db)
        .await
        .unwrap();
    }

    // providerId + modelId 组合过滤：只返回 gpt-4o。
    let (status, json) = get_json(
        app,
        &format!(
            "/api/stats/charts?startTime={t0}&endTime={}&providerId={}&modelId=gpt-4o",
            t0 + 2 * HOUR_MS,
            DEFAULT_PROVIDER_ID
        ),
    )
    .await;
    assert_eq!(status, 200);
    let data = &json["data"];
    let call_by_model = data["callByModel"].as_array().unwrap();
    assert_eq!(call_by_model.len(), 1);
    assert_eq!(call_by_model[0]["modelId"], "gpt-4o");
    let total_calls: i64 = data["callTrend"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["value"].as_i64().unwrap())
        .sum();
    assert_eq!(total_calls, 1);
}

/// provider-metrics：供应商级 6 指标聚合（成功行 + 窗口过滤）。
#[tokio::test]
async fn test_provider_metrics_aggregates_success_rows_in_window() {
    let (app, db) = setup_app().await;
    let now = chrono::Utc::now().timestamp_millis();
    let t0 = now - 24 * HOUR_MS;

    // 成功 2 条 + 失败 1 条（失败不计入指标）。
    for row in [
        SeedRow {
            request_id: "pm-ok1".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            success: true,
            start_time: t0,
            input_tokens: Some(100),
            input_cache_tokens: 40,
            total_tokens: Some(150),
        },
        SeedRow {
            request_id: "pm-ok2".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "claude-3".into(),
            success: true,
            start_time: t0 + HOUR_MS,
            input_tokens: Some(200),
            input_cache_tokens: 0,
            total_tokens: Some(300),
        },
        SeedRow {
            request_id: "pm-fail".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            success: false,
            start_time: t0 + 2 * HOUR_MS,
            input_tokens: Some(999),
            input_cache_tokens: 0,
            total_tokens: Some(999),
        },
    ] {
        insert_request(&db, row).await;
    }
    // 窗口外的成功行不计入。
    insert_request(
        &db,
        SeedRow {
            request_id: "pm-outside".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            success: true,
            start_time: now,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(20),
        },
    )
    .await;

    let (status, json) = get_json(
        app,
        &format!(
            "/api/stats/provider-metrics?providerId={}&startTime={t0}&endTime={}",
            DEFAULT_PROVIDER_ID, now
        ),
    )
    .await;
    assert_eq!(status, 200);
    let data = &json["data"];
    assert_eq!(data["providerId"], DEFAULT_PROVIDER_ID);
    assert_eq!(data["providerName"], DEFAULT_PROVIDER_NAME);
    assert_eq!(data["requestCount"], 2);
    assert_eq!(data["totalTokens"], 450);
    // 缓存命中率 = 40 / (100+200) = 0.13333…，后端 ROUND(...,5) 保留 5 位。
    let cache_rate = data["cacheHitRate"].as_f64().unwrap();
    assert!(
        (cache_rate - 0.13333).abs() < 1e-9,
        "cacheHitRate={cache_rate}"
    );
}

/// provider-metrics：缺参 / 窗口非法返回 400。
#[tokio::test]
async fn test_provider_metrics_validation() {
    let (app, _db) = setup_app().await;

    let (status, _) = get_json(app.clone(), "/api/stats/provider-metrics").await;
    assert_eq!(status, 400);
    let (status, _) = get_json(
        app.clone(),
        "/api/stats/provider-metrics?providerId=1&startTime=100&endTime=100",
    )
    .await;
    assert_eq!(status, 400);
}

/// virtual-model-metrics：按虚拟模型过滤聚合。
#[tokio::test]
async fn test_virtual_model_metrics_aggregates() {
    let (app, db) = setup_app().await;
    let now = chrono::Utc::now().timestamp_millis();
    let t0 = now - HOUR_MS;

    for row in [
        SeedRow {
            request_id: "vm-m1".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            success: true,
            start_time: t0,
            input_tokens: Some(100),
            input_cache_tokens: 50,
            total_tokens: Some(200),
        },
        SeedRow {
            request_id: "vm-m2".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            success: true,
            start_time: t0 + 100,
            input_tokens: Some(100),
            input_cache_tokens: 0,
            total_tokens: Some(200),
        },
    ] {
        insert_request(&db, row).await;
    }

    let (status, json) = get_json(
        app,
        &format!("/api/stats/virtual-model-metrics?virtualModelId=1&startTime={t0}&endTime={now}"),
    )
    .await;
    assert_eq!(status, 200);
    let data = &json["data"];
    assert_eq!(data["virtualModelId"], 1);
    assert_eq!(data["requestCount"], 2);
    assert_eq!(data["totalTokens"], 400);
    // 缓存命中率 = 50 / 200 = 0.25。
    let cache_rate = data["cacheHitRate"].as_f64().unwrap();
    assert!(
        (cache_rate - 0.25).abs() < 1e-9,
        "cacheHitRate={cache_rate}"
    );
}

// ---------- 显式 granularity + tzOffsetMinutes 分桶 ----------

/// 东八区本地日期 → UTC 毫秒时间戳。
fn local_ms_cn(y: i32, m: u32, d: u32, h: u32, min: u32) -> i64 {
    use chrono::TimeZone;
    chrono::FixedOffset::east_opt(480 * 60)
        .unwrap()
        .with_ymd_and_hms(y, m, d, h, min, 0)
        .single()
        .unwrap()
        .timestamp_millis()
}

#[tokio::test]
async fn test_charts_hour_granularity_local_alignment() {
    let (app, db) = setup_app().await;
    // 窗口：本地 2026-08-31 00:00 ~ 03:30（东八区）。tzOffsetMinutes=480。
    let start = local_ms_cn(2026, 8, 31, 0, 0);
    let end = local_ms_cn(2026, 8, 31, 3, 30);
    for (i, (offset_ms, tokens)) in [(0, 10), (HOUR_MS, 20), (2 * HOUR_MS + 1000, 30)]
        .into_iter()
        .enumerate()
    {
        insert_request(
            &db,
            SeedRow {
                request_id: format!("h-{i}"),
                provider_id: DEFAULT_PROVIDER_ID,
                model_id: "gpt-4o".into(),
                success: true,
                start_time: start + offset_ms,
                input_tokens: Some(1),
                input_cache_tokens: 0,
                total_tokens: Some(tokens),
            },
        )
        .await;
    }

    let (status, json) = get_json(
        app,
        &format!(
            "/api/stats/charts?startTime={start}&endTime={end}&granularity=hour&tzOffsetMinutes=480"
        ),
    )
    .await;
    assert_eq!(status, 200);
    let data = &json["data"];
    let call_trend = data["callTrend"].as_array().unwrap();
    // 0:00~3:30 → 0、1、2、3 共 4 个整点桶。
    assert_eq!(call_trend.len(), 4);
    let starts: Vec<i64> = call_trend
        .iter()
        .map(|p| p["bucketStart"].as_i64().unwrap())
        .collect();
    assert_eq!(starts[0], start);
    assert_eq!(starts[1], start + HOUR_MS);
    assert_eq!(starts[2], start + 2 * HOUR_MS);
    assert_eq!(starts[3], start + 3 * HOUR_MS);
    let values: Vec<i64> = call_trend
        .iter()
        .map(|p| p["value"].as_i64().unwrap())
        .collect();
    assert_eq!(values, vec![1, 1, 1, 0]);
    let token_trend = data["tokenTrend"].as_array().unwrap();
    let token_values: Vec<i64> = token_trend
        .iter()
        .map(|p| p["value"].as_i64().unwrap())
        .collect();
    assert_eq!(token_values, vec![10, 20, 30, 0]);
}

#[tokio::test]
async fn test_charts_day_granularity_week_has_seven_points() {
    let (app, db) = setup_app().await;
    // 窗口：本地 2026-08-24（周一）00:00 ~ 2026-08-31（下周一）00:00（东八区）。
    // UTC 视角该窗口横跨 8 个 UTC 日，但按本地日对齐必须只出 7 个桶。
    let start = local_ms_cn(2026, 8, 24, 0, 0);
    let end = local_ms_cn(2026, 8, 31, 0, 0);
    for (i, day) in [0, 2, 5].into_iter().enumerate() {
        insert_request(
            &db,
            SeedRow {
                request_id: format!("d-{i}"),
                provider_id: DEFAULT_PROVIDER_ID,
                model_id: "gpt-4o".into(),
                success: true,
                start_time: start + day * 24 * HOUR_MS,
                input_tokens: Some(1),
                input_cache_tokens: 0,
                total_tokens: Some(100 + i as i64),
            },
        )
        .await;
    }

    let (status, json) = get_json(
        app,
        &format!(
            "/api/stats/charts?startTime={start}&endTime={end}&granularity=day&tzOffsetMinutes=480"
        ),
    )
    .await;
    assert_eq!(status, 200);
    let data = &json["data"];
    let call_trend = data["callTrend"].as_array().unwrap();
    assert_eq!(call_trend.len(), 7);
    let starts: Vec<i64> = call_trend
        .iter()
        .map(|p| p["bucketStart"].as_i64().unwrap())
        .collect();
    assert_eq!(starts[0], start);
    for w in starts.windows(2) {
        assert_eq!(w[1] - w[0], 24 * HOUR_MS);
    }
    let values: Vec<i64> = call_trend
        .iter()
        .map(|p| p["value"].as_i64().unwrap())
        .collect();
    assert_eq!(values, vec![1, 0, 1, 0, 0, 1, 0]);
}

#[tokio::test]
async fn test_charts_month_granularity_natural_months() {
    let (app, db) = setup_app().await;
    // 窗口：本地 2026-06-25 ~ 2026-08-27（东八区）。数据落在 6/25、7/15、7/16、8/26。
    let start = local_ms_cn(2026, 6, 25, 0, 0);
    let end = local_ms_cn(2026, 8, 27, 0, 0);
    let rows = [
        (start, 1, 10),
        (local_ms_cn(2026, 7, 15, 0, 0), 1, 20),
        (local_ms_cn(2026, 7, 16, 0, 0), 1, 30),
        (local_ms_cn(2026, 8, 26, 0, 0), 1, 40),
    ];
    for (i, (ts, _, _)) in rows.iter().enumerate() {
        insert_request(
            &db,
            SeedRow {
                request_id: format!("m-{i}"),
                provider_id: DEFAULT_PROVIDER_ID,
                model_id: "gpt-4o".into(),
                success: true,
                start_time: *ts,
                input_tokens: Some(1),
                input_cache_tokens: 0,
                total_tokens: Some(rows[i].2),
            },
        )
        .await;
    }

    let (status, json) = get_json(
        app,
        &format!("/api/stats/charts?startTime={start}&endTime={end}&granularity=month&tzOffsetMinutes=480"),
    )
    .await;
    assert_eq!(status, 200);
    let data = &json["data"];
    let call_trend = data["callTrend"].as_array().unwrap();
    assert_eq!(call_trend.len(), 3);
    let starts: Vec<i64> = call_trend
        .iter()
        .map(|p| p["bucketStart"].as_i64().unwrap())
        .collect();
    assert_eq!(starts[0], local_ms_cn(2026, 6, 1, 0, 0));
    assert_eq!(starts[1], local_ms_cn(2026, 7, 1, 0, 0));
    assert_eq!(starts[2], local_ms_cn(2026, 8, 1, 0, 0));
    let values: Vec<i64> = call_trend
        .iter()
        .map(|p| p["value"].as_i64().unwrap())
        .collect();
    assert_eq!(values, vec![1, 2, 1]);
    let token_values: Vec<i64> = data["tokenTrend"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["value"].as_i64().unwrap())
        .collect();
    assert_eq!(token_values, vec![10, 50, 40]);
}

#[tokio::test]
async fn test_charts_year_granularity_natural_years() {
    let (app, db) = setup_app().await;
    // 窗口：本地 2025-07-01 ~ 2026-09-01（东八区）。数据落在 2025 与 2026。
    let start = local_ms_cn(2025, 7, 1, 0, 0);
    let end = local_ms_cn(2026, 9, 1, 0, 0);
    for (i, ts) in [start, local_ms_cn(2026, 3, 5, 0, 0)]
        .into_iter()
        .enumerate()
    {
        insert_request(
            &db,
            SeedRow {
                request_id: format!("y-{i}"),
                provider_id: DEFAULT_PROVIDER_ID,
                model_id: "gpt-4o".into(),
                success: true,
                start_time: ts,
                input_tokens: Some(1),
                input_cache_tokens: 0,
                total_tokens: Some(100 + i as i64),
            },
        )
        .await;
    }

    let (status, json) = get_json(
        app,
        &format!(
            "/api/stats/charts?startTime={start}&endTime={end}&granularity=year&tzOffsetMinutes=480"
        ),
    )
    .await;
    assert_eq!(status, 200);
    let data = &json["data"];
    let call_trend = data["callTrend"].as_array().unwrap();
    assert_eq!(call_trend.len(), 2);
    let starts: Vec<i64> = call_trend
        .iter()
        .map(|p| p["bucketStart"].as_i64().unwrap())
        .collect();
    assert_eq!(starts[0], local_ms_cn(2025, 1, 1, 0, 0));
    assert_eq!(starts[1], local_ms_cn(2026, 1, 1, 0, 0));
    let values: Vec<i64> = call_trend
        .iter()
        .map(|p| p["value"].as_i64().unwrap())
        .collect();
    assert_eq!(values, vec![1, 1]);
}

#[tokio::test]
async fn test_charts_invalid_granularity_rejected() {
    let (app, _db) = setup_app().await;
    let now = chrono::Utc::now().timestamp_millis();
    let (status, json) = get_json(
        app,
        &format!(
            "/api/stats/charts?startTime={}&endTime={}&granularity=week",
            now - HOUR_MS,
            now
        ),
    )
    .await;
    assert_eq!(status, 400);
    assert_eq!(json["code"], "INVALID_INPUT");
}

#[tokio::test]
async fn test_charts_granularity_without_window_defaults_to_past_24h() {
    let (app, db) = setup_app().await;
    // 无 startTime/endTime + 显式 granularity=hour：回退过去 24 小时，小时桶。
    let now = chrono::Utc::now().timestamp_millis();
    insert_request(
        &db,
        SeedRow {
            request_id: "df-1".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            success: true,
            start_time: now - HOUR_MS / 2,
            input_tokens: Some(1),
            input_cache_tokens: 0,
            total_tokens: Some(50),
        },
    )
    .await;

    let (status, json) = get_json(
        app,
        "/api/stats/charts?granularity=hour&tzOffsetMinutes=480",
    )
    .await;
    assert_eq!(status, 200);
    let data = &json["data"];
    let call_trend = data["callTrend"].as_array().unwrap();
    assert_eq!(call_trend.len(), 24);
    let total: i64 = call_trend
        .iter()
        .map(|p| p["value"].as_i64().unwrap())
        .sum();
    assert_eq!(total, 1);
}

#[tokio::test]
async fn test_summary_success_rate_rounded_to_five_decimals() {
    let (app, db) = setup_app().await;
    // 2 成功 1 失败：成功率 = 2/3 = 0.66666...，应四舍五入保留 5 位 = 0.66667。
    for (i, success) in [true, true, false].into_iter().enumerate() {
        insert_request(
            &db,
            SeedRow {
                request_id: format!("sr-{i}"),
                provider_id: DEFAULT_PROVIDER_ID,
                model_id: "gpt-4o".into(),
                success,
                start_time: chrono::Utc::now().timestamp_millis(),
                input_tokens: Some(1),
                input_cache_tokens: 0,
                total_tokens: Some(10),
            },
        )
        .await;
    }

    let (status, json) = get_json(app, "/api/stats/summary").await;
    assert_eq!(status, 200);
    let rate = json["data"]["successRate"].as_f64().unwrap();
    assert!((rate - 0.66667).abs() < 1e-9, "successRate={rate}");
}

// ---------- /api/stats/insight 性能与可靠性分析 ----------

/// 全字段请求种子（insight 需要 stream/ttft/output_tokens/output_tokens_time/fail_reason）。
struct FullRow {
    request_id: String,
    provider_id: i32,
    model_id: String,
    stream: bool,
    ttft: Option<i64>,
    input_tokens: Option<i64>,
    input_cache_tokens: i64,
    output_tokens: Option<i64>,
    output_tokens_time: Option<i64>,
    request_time: i64,
    success: bool,
    fail_reason: Option<String>,
    total_tokens: Option<i64>,
    start_time: i64,
}

async fn insert_full(db: &DatabaseConnection, row: FullRow) {
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
        tps: Set(0.0),
        start_time: Set(row.start_time),
        end_time: Set(end_time),
        request_time: Set(row.request_time),
        success: Set(row.success),
        fail_reason: Set(row.fail_reason),
        total_tokens: Set(row.total_tokens),
        api_key_name: Set("itest-key".to_string()),
    }
    .insert(db)
    .await
    .unwrap();
}

#[tokio::test]
async fn test_insight_empty_table_returns_zeros() {
    let (app, _db) = setup_app().await;

    let (status, json) = get_json(app, "/api/stats/insight").await;
    assert_eq!(status, 200);
    let data = &json["data"];

    // 缺省窗口=过去 24 小时（小时桶）→ 24 个桶。
    assert_eq!(data["callTrend"].as_array().unwrap().len(), 24);
    assert_eq!(data["failureTrend"].as_array().unwrap().len(), 24);
    assert_eq!(data["failureRateTrend"].as_array().unwrap().len(), 24);
    assert_eq!(data["inputTokenTrend"].as_array().unwrap().len(), 24);
    assert_eq!(data["outputTokenTrend"].as_array().unwrap().len(), 24);
    assert_eq!(data["cacheHitRateTrend"].as_array().unwrap().len(), 24);
    assert_eq!(
        data["outputTokensPerSecTrend"].as_array().unwrap().len(),
        24
    );
    assert_eq!(data["streamRatioTrend"].as_array().unwrap().len(), 24);
    assert_eq!(data["ttftPercentiles"].as_array().unwrap().len(), 24);
    assert_eq!(data["latencyPercentiles"].as_array().unwrap().len(), 24);
    assert!(data["failureReasons"].as_array().unwrap().is_empty());
    assert!(data["apiKeyRank"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn test_insight_failure_diagnostics() {
    let (app, db) = setup_app().await;
    let now = chrono::Utc::now().timestamp_millis();
    let bucket_start = (now / HOUR_MS) * HOUR_MS;

    // 同桶：2 成功（无原因）+ 1 失败（429 限流）。
    for row in [
        FullRow {
            request_id: "i-f1".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            stream: false,
            ttft: None,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            output_tokens: Some(20),
            output_tokens_time: None,
            request_time: 500,
            success: true,
            fail_reason: None,
            total_tokens: Some(30),
            start_time: bucket_start + 1,
        },
        FullRow {
            request_id: "i-f2".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            stream: false,
            ttft: None,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            output_tokens: Some(20),
            output_tokens_time: None,
            request_time: 600,
            success: true,
            fail_reason: None,
            total_tokens: Some(30),
            start_time: bucket_start + 2,
        },
        FullRow {
            request_id: "i-f3".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            stream: false,
            ttft: None,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            output_tokens: None,
            output_tokens_time: None,
            request_time: 100,
            success: false,
            fail_reason: Some("上游 429 限流".to_string()),
            total_tokens: None,
            start_time: bucket_start + 3,
        },
    ] {
        insert_full(&db, row).await;
    }

    let (status, json) = get_json(
        app,
        &format!(
            "/api/stats/insight?startTime={}&endTime={}&tzOffsetMinutes=0",
            bucket_start,
            bucket_start + HOUR_MS
        ),
    )
    .await;
    assert_eq!(status, 200);
    let data = &json["data"];

    let failure_trend = data["failureTrend"].as_array().unwrap();
    assert_eq!(failure_trend.len(), 1);
    assert_eq!(failure_trend[0]["value"], 1);

    // callTrend = 全部调用（2 成功 + 1 失败）= 3；堆叠面积基准应含成功与失败。
    let call_trend = data["callTrend"].as_array().unwrap();
    assert_eq!(call_trend.len(), 1);
    assert_eq!(call_trend[0]["value"], 3);

    let failure_rate_trend = data["failureRateTrend"].as_array().unwrap();
    assert_eq!(failure_rate_trend.len(), 1);
    let rate = failure_rate_trend[0]["value"].as_f64().unwrap();
    assert!((rate - 1.0 / 3.0).abs() < 1e-9, "failureRate={rate}");

    let failure_reasons = data["failureReasons"].as_array().unwrap();
    assert_eq!(failure_reasons.len(), 1);
    assert_eq!(failure_reasons[0]["reason"], "上游 429 限流");
    assert_eq!(failure_reasons[0]["count"], 1);

    // 2 成功 1 失败 → apiKeyRank 按调用数（全量）聚合 = 3。
    let api_key_rank = data["apiKeyRank"].as_array().unwrap();
    assert_eq!(api_key_rank.len(), 1);
    assert_eq!(api_key_rank[0]["apiKeyName"], "itest-key");
    assert_eq!(api_key_rank[0]["value"], 3);
}

#[tokio::test]
async fn test_insight_failure_reason_empty_maps_to_no_reason() {
    let (app, db) = setup_app().await;
    let now = chrono::Utc::now().timestamp_millis();
    let t0 = (now / HOUR_MS) * HOUR_MS;

    // 一条失败且 fail_reason 为空串，一条失败且 NULL：都应归「空串原因」（前端显示无原因）。
    for (rid, reason) in [("nr1", Some("".to_string())), ("nr2", None)] {
        insert_full(
            &db,
            FullRow {
                request_id: rid.into(),
                provider_id: DEFAULT_PROVIDER_ID,
                model_id: "gpt-4o".into(),
                stream: false,
                ttft: None,
                input_tokens: Some(1),
                input_cache_tokens: 0,
                output_tokens: None,
                output_tokens_time: None,
                request_time: 100,
                success: false,
                fail_reason: reason,
                total_tokens: None,
                start_time: t0 + 1,
            },
        )
        .await;
    }

    let (status, json) = get_json(
        app,
        &format!(
            "/api/stats/insight?startTime={}&endTime={}&tzOffsetMinutes=0",
            t0,
            t0 + HOUR_MS
        ),
    )
    .await;
    assert_eq!(status, 200);
    let failure_reasons = json["data"]["failureReasons"].as_array().unwrap();
    assert_eq!(failure_reasons.len(), 1);
    assert_eq!(failure_reasons[0]["reason"], "");
    assert_eq!(failure_reasons[0]["count"], 2);
}

#[tokio::test]
async fn test_insight_latency_percentiles() {
    let (app, db) = setup_app().await;
    let now = chrono::Utc::now().timestamp_millis();
    let t0 = (now / HOUR_MS) * HOUR_MS;

    // 同一小时内 3 个成功流式请求：ttft=[100, 200, 300]，request_time=[500, 600, 700]。
    for (i, (ttft, rt)) in [(100i64, 500i64), (200, 600), (300, 700)]
        .into_iter()
        .enumerate()
    {
        insert_full(
            &db,
            FullRow {
                request_id: format!("lp-{i}"),
                provider_id: DEFAULT_PROVIDER_ID,
                model_id: "gpt-4o".into(),
                stream: true,
                ttft: Some(ttft),
                input_tokens: Some(10),
                input_cache_tokens: 0,
                output_tokens: Some(20),
                output_tokens_time: Some(400),
                request_time: rt,
                success: true,
                fail_reason: None,
                total_tokens: Some(30),
                start_time: t0 + i as i64 + 1,
            },
        )
        .await;
    }

    let (status, json) = get_json(
        app,
        &format!(
            "/api/stats/insight?startTime={}&endTime={}&tzOffsetMinutes=0",
            t0,
            t0 + HOUR_MS
        ),
    )
    .await;
    assert_eq!(status, 200);
    let data = &json["data"];

    let ttft_p = data["ttftPercentiles"].as_array().unwrap();
    assert_eq!(ttft_p.len(), 1);
    // 线性插值：n=3 → p50=200, p90=280, p95=290, p99=298。
    assert_eq!(ttft_p[0]["p50"], 200.0);
    assert_eq!(ttft_p[0]["p90"], 280.0);
    assert_eq!(ttft_p[0]["p95"], 290.0);
    assert_eq!(ttft_p[0]["p99"], 298.0);

    let latency_p = data["latencyPercentiles"].as_array().unwrap();
    assert_eq!(latency_p.len(), 1);
    assert_eq!(latency_p[0]["p50"], 600.0);
    assert_eq!(latency_p[0]["p90"], 680.0);
    assert_eq!(latency_p[0]["p95"], 690.0);
    assert_eq!(latency_p[0]["p99"], 698.0);
}

#[tokio::test]
async fn test_insight_token_structure_and_throughput() {
    let (app, db) = setup_app().await;
    let now = chrono::Utc::now().timestamp_millis();
    let t0 = (now / HOUR_MS) * HOUR_MS;

    // 同一小时内 2 个成功请求：输入 [100, 100]、输出 [200, 300]、缓存 [40, 0]、
    // 输出耗时 [2000ms, 3000ms] → 每秒输出 token = 200/2 + 300/3 = 200。
    for (i, (input, output, cache, out_ms)) in
        [(100i64, 200i64, 40i64, 2000i64), (100, 300, 0, 3000)]
            .into_iter()
            .enumerate()
    {
        insert_full(
            &db,
            FullRow {
                request_id: format!("ts-{i}"),
                provider_id: DEFAULT_PROVIDER_ID,
                model_id: "gpt-4o".into(),
                stream: true,
                ttft: Some(100),
                input_tokens: Some(input),
                input_cache_tokens: cache,
                output_tokens: Some(output),
                output_tokens_time: Some(out_ms),
                request_time: 1000,
                success: true,
                fail_reason: None,
                total_tokens: Some(input + output),
                start_time: t0 + i as i64 + 1,
            },
        )
        .await;
    }

    let (status, json) = get_json(
        app,
        &format!(
            "/api/stats/insight?startTime={}&endTime={}&tzOffsetMinutes=0",
            t0,
            t0 + HOUR_MS
        ),
    )
    .await;
    assert_eq!(status, 200);
    let data = &json["data"];

    let input_trend = data["inputTokenTrend"].as_array().unwrap();
    assert_eq!(input_trend[0]["value"], 200);
    let output_trend = data["outputTokenTrend"].as_array().unwrap();
    assert_eq!(output_trend[0]["value"], 500);

    let cache_rate = data["cacheHitRateTrend"].as_array().unwrap();
    assert_eq!(cache_rate[0]["value"].as_f64().unwrap(), 0.2);

    let out_per_sec = data["outputTokensPerSecTrend"].as_array().unwrap();
    let tps_val = out_per_sec[0]["value"].as_f64().unwrap();
    assert!((tps_val - 200.0).abs() < 1e-9, "outputPerSec={tps_val}");

    // 流式占比：2 个都流式 → 1.0。
    let stream_ratio = data["streamRatioTrend"].as_array().unwrap();
    assert_eq!(stream_ratio[0]["value"].as_f64().unwrap(), 1.0);

    // 小时桶 → RPM/TPM 有值（窗口 1 小时 → RPM=2；total_tokens=700 → TPM=700/60≈11.67 每分钟）。
    let rpm = data["rpmTrend"].as_array().unwrap();
    assert_eq!(rpm[0]["value"], 2);
    let tpm = data["tpmTrend"].as_array().unwrap();
    let tpm_val = tpm[0]["value"].as_f64().unwrap();
    assert!((tpm_val - 700.0 / 60.0).abs() < 1e-9, "tpm={tpm_val}");
}

#[tokio::test]
async fn test_insight_filters_by_provider_vm_model() {
    let (app, db) = setup_app().await;
    let now = chrono::Utc::now().timestamp_millis();
    let t0 = (now / HOUR_MS) * HOUR_MS;

    // 供应商 1：gpt-4o（1 次）；供应商 2：claude-sonnet（1 次）。
    for (rid, pid, model) in [
        ("flt1", DEFAULT_PROVIDER_ID, "gpt-4o"),
        ("flt2", 2, "claude-sonnet"),
    ] {
        insert_full(
            &db,
            FullRow {
                request_id: rid.into(),
                provider_id: pid,
                model_id: model.into(),
                stream: true,
                ttft: Some(100),
                input_tokens: Some(10),
                input_cache_tokens: 0,
                output_tokens: Some(20),
                output_tokens_time: Some(1000),
                request_time: 500,
                success: true,
                fail_reason: None,
                total_tokens: Some(30),
                start_time: t0 + 1,
            },
        )
        .await;
    }

    // providerId 过滤 → 只留供应商 1。
    let (status, json) = get_json(
        app.clone(),
        &format!(
            "/api/stats/insight?startTime={}&endTime={}&tzOffsetMinutes=0&providerId={}",
            t0,
            t0 + HOUR_MS,
            DEFAULT_PROVIDER_ID
        ),
    )
    .await;
    assert_eq!(status, 200);
    let data = &json["data"];
    assert_eq!(data["apiKeyRank"].as_array().unwrap()[0]["value"], 1);

    // virtualModelId=1 + modelId 组合：仍只留 1 条。
    let (status, json) = get_json(
        app,
        &format!(
            "/api/stats/insight?startTime={}&endTime={}&tzOffsetMinutes=0&virtualModelId=1&modelId=gpt-4o",
            t0,
            t0 + HOUR_MS
        ),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        json["data"]["apiKeyRank"].as_array().unwrap()[0]["value"],
        1
    );
}

#[tokio::test]
async fn test_insight_invalid_window_rejected() {
    // end<=start 回退缺省窗口（与 charts 一致，200）；非法 granularity 才是 400。
    let (app, _db) = setup_app().await;
    let (status, json) = get_json(app, "/api/stats/insight?startTime=100&endTime=100").await;
    assert_eq!(status, 200);
    assert_eq!(json["data"]["failureTrend"].as_array().unwrap().len(), 24);

    let (app, _db) = setup_app().await;
    let now = chrono::Utc::now().timestamp_millis();
    let (status, _) = get_json(
        app,
        &format!(
            "/api/stats/insight?startTime={}&endTime={}&granularity=week",
            now - HOUR_MS,
            now
        ),
    )
    .await;
    assert_eq!(status, 400);
}

/// 月/年粒度：比率在自然月归并后按桶起点重算，空月补零且比率为 0。
#[tokio::test]
async fn test_insight_month_granularity_recomputes_ratios() {
    let (app, db) = setup_app().await;
    // 窗口：2026-06-25 ~ 2026-08-27（东八区）。6 月 1 败 1 成、7 月无数据、8 月 1 成。
    let start = local_ms_cn(2026, 6, 25, 0, 0);
    let end = local_ms_cn(2026, 8, 27, 0, 0);

    for row in [
        FullRow {
            request_id: "m-fail-1".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            stream: true,
            ttft: Some(100),
            input_tokens: Some(100),
            input_cache_tokens: 0,
            output_tokens: Some(200),
            output_tokens_time: Some(2000),
            request_time: 500,
            success: false,
            fail_reason: Some("上游 500".to_string()),
            total_tokens: None,
            start_time: local_ms_cn(2026, 6, 25, 10, 0),
        },
        FullRow {
            request_id: "m-ok-1".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            stream: true,
            ttft: Some(100),
            input_tokens: Some(100),
            input_cache_tokens: 0,
            output_tokens: Some(200),
            output_tokens_time: Some(2000),
            request_time: 500,
            success: true,
            fail_reason: None,
            total_tokens: Some(300),
            start_time: local_ms_cn(2026, 6, 26, 10, 0),
        },
        FullRow {
            request_id: "m-ok-2".into(),
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            stream: true,
            ttft: Some(100),
            input_tokens: Some(100),
            input_cache_tokens: 0,
            output_tokens: Some(200),
            output_tokens_time: Some(2000),
            request_time: 500,
            success: true,
            fail_reason: None,
            total_tokens: Some(300),
            start_time: local_ms_cn(2026, 8, 26, 10, 0),
        },
    ] {
        insert_full(&db, row).await;
    }

    let (status, json) = get_json(
        app,
        &format!(
            "/api/stats/insight?startTime={start}&endTime={end}&granularity=month&tzOffsetMinutes=480"
        ),
    )
    .await;
    assert_eq!(status, 200);
    let data = &json["data"];

    // 三个月桶：6/7/8 月，起点为各月 1 日。
    let failure_trend = data["failureTrend"].as_array().unwrap();
    assert_eq!(failure_trend.len(), 3);
    assert_eq!(
        failure_trend[0]["bucketStart"],
        local_ms_cn(2026, 6, 1, 0, 0)
    );
    assert_eq!(
        failure_trend[1]["bucketStart"],
        local_ms_cn(2026, 7, 1, 0, 0)
    );
    assert_eq!(
        failure_trend[2]["bucketStart"],
        local_ms_cn(2026, 8, 1, 0, 0)
    );
    assert_eq!(failure_trend[0]["value"], 1);
    assert_eq!(failure_trend[1]["value"], 0);
    assert_eq!(failure_trend[2]["value"], 0);

    // 失败率：6 月 2 条（1 败 1 成）= 1/2，7/8 月 = 0（7 月无流量也为 0，不除零）。
    let failure_rate = data["failureRateTrend"].as_array().unwrap();
    assert_eq!(failure_rate.len(), 3);
    let june_rate = failure_rate[0]["value"].as_f64().unwrap();
    assert!(
        (june_rate - 0.5).abs() < 1e-9,
        "june failureRate={june_rate}"
    );
    assert_eq!(failure_rate[1]["value"], 0.0);
    assert_eq!(failure_rate[2]["value"], 0.0);

    // 失败原因只来自失败请求（6 月 1 条），跨月仍按窗口全量聚合。
    let reasons = data["failureReasons"].as_array().unwrap();
    assert_eq!(reasons.len(), 1);
    assert_eq!(reasons[0]["reason"], "上游 500");

    // 分位在月粒度退化为空数组。
    assert_eq!(data["ttftPercentiles"].as_array().unwrap().len(), 0);
    assert_eq!(data["latencyPercentiles"].as_array().unwrap().len(), 0);

    // RPM/TPM 仅小时粒度，月粒度返回空。
    assert_eq!(data["rpmTrend"].as_array().unwrap().len(), 0);
    assert_eq!(data["tpmTrend"].as_array().unwrap().len(), 0);

    // 流式占比：6 月全流式 = 1.0，7 月 0。
    let stream_ratio = data["streamRatioTrend"].as_array().unwrap();
    assert_eq!(stream_ratio[0]["value"].as_f64().unwrap(), 1.0);
    assert_eq!(stream_ratio[1]["value"].as_f64().unwrap(), 0.0);
}

// ---------- /api/stats/api-key-rank API Key 维度赛马 ----------

/// 插入一条 request，支持自定义 api_key_name / virtual_model_id（其余字段按种子默认）。
struct ApiKeyRow {
    request_id: String,
    virtual_model_id: i32,
    provider_id: i32,
    model_id: String,
    api_key_name: String,
    success: bool,
    start_time: i64,
    input_tokens: Option<i64>,
    input_cache_tokens: i64,
    total_tokens: Option<i64>,
}

async fn insert_ak_row(db: &DatabaseConnection, row: ApiKeyRow) {
    let end_time = row.start_time + 500;
    request_entity::ActiveModel {
        request_id: Set(row.request_id),
        virtual_model_id: Set(row.virtual_model_id),
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
        api_key_name: Set(row.api_key_name),
    }
    .insert(db)
    .await
    .unwrap();
}

/// 全量聚合：按 api_key_name 分组，6 指标正确 + 缺省 totalTokens 降序。
#[tokio::test]
async fn test_api_key_rank_aggregates_by_key_name() {
    let (app, db) = setup_app().await;
    let t0 = (1_700_000_000_000i64 / HOUR_MS) * HOUR_MS;
    for row in [
        ApiKeyRow {
            request_id: "ak-1".into(),
            virtual_model_id: 1,
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            api_key_name: "key-a".into(),
            success: true,
            start_time: t0 + 1,
            input_tokens: Some(100),
            input_cache_tokens: 40,
            total_tokens: Some(150),
        },
        // key-a 第二笔：累计 request_count=2、totalTokens=350、缓存率 40/200=0.2。
        ApiKeyRow {
            request_id: "ak-2".into(),
            virtual_model_id: 1,
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            api_key_name: "key-a".into(),
            success: true,
            start_time: t0 + 2,
            input_tokens: Some(100),
            input_cache_tokens: 0,
            total_tokens: Some(200),
        },
        ApiKeyRow {
            request_id: "ak-3".into(),
            virtual_model_id: 1,
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "claude-sonnet".into(),
            api_key_name: "key-b".into(),
            success: true,
            start_time: t0 + 3,
            input_tokens: Some(50),
            input_cache_tokens: 10,
            total_tokens: Some(100),
        },
        // 失败行不计入赛马指标。
        ApiKeyRow {
            request_id: "ak-4".into(),
            virtual_model_id: 1,
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            api_key_name: "key-b".into(),
            success: false,
            start_time: t0 + 4,
            input_tokens: Some(999),
            input_cache_tokens: 0,
            total_tokens: Some(999),
        },
        // 窗口外行不计入。
        ApiKeyRow {
            request_id: "ak-5".into(),
            virtual_model_id: 1,
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            api_key_name: "key-c".into(),
            success: true,
            start_time: t0 - 1,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(10),
        },
    ] {
        insert_ak_row(&db, row).await;
    }

    let (status, json) = get_json(
        app,
        &format!(
            "/api/stats/api-key-rank?startTime={t0}&endTime={}",
            t0 + 2 * HOUR_MS
        ),
    )
    .await;
    assert_eq!(status, 200);
    let data = &json["data"];
    assert_eq!(data["startTime"], t0);
    let items = data["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);

    // 缺省 totalTokens 降序：key-a (350) 在前，key-b (100) 在后。
    assert_eq!(items[0]["apiKeyName"], "key-a");
    assert_eq!(items[0]["requestCount"], 2);
    assert_eq!(items[0]["totalTokens"], 350);
    let cache_a = items[0]["cacheHitRate"].as_f64().unwrap();
    assert!((cache_a - 0.2).abs() < 1e-9, "cacheA={cache_a}");

    assert_eq!(items[1]["apiKeyName"], "key-b");
    assert_eq!(items[1]["requestCount"], 1);
    assert_eq!(items[1]["totalTokens"], 100);
    let cache_b = items[1]["cacheHitRate"].as_f64().unwrap();
    assert!((cache_b - 0.2).abs() < 1e-9, "cacheB={cache_b}");
}

/// 过滤组合：providerId / virtualModelId / providerId + modelId 各自生效。
#[tokio::test]
async fn test_api_key_rank_filters() {
    let (app, db) = setup_app().await;
    let t0 = (1_700_000_000_000i64 / HOUR_MS) * HOUR_MS;
    for row in [
        // 供应商 1 · 虚拟模型 1 · gpt-4o · key-a
        ApiKeyRow {
            request_id: "akf-1".into(),
            virtual_model_id: 1,
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            api_key_name: "key-a".into(),
            success: true,
            start_time: t0 + 1,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(100),
        },
        // 供应商 2 · 虚拟模型 1 · claude-sonnet · key-a（不同 provider）
        ApiKeyRow {
            request_id: "akf-2".into(),
            virtual_model_id: 1,
            provider_id: 2,
            model_id: "claude-sonnet".into(),
            api_key_name: "key-a".into(),
            success: true,
            start_time: t0 + 2,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(200),
        },
        // 供应商 1 · 虚拟模型 2 · deepseek-v3 · key-b（不同虚拟模型 + 不同模型，
        // 用于区分 virtualModelId 与 providerId+modelId 两种过滤口径）。
        ApiKeyRow {
            request_id: "akf-3".into(),
            virtual_model_id: 2,
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "deepseek-v3".into(),
            api_key_name: "key-b".into(),
            success: true,
            start_time: t0 + 3,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(300),
        },
        // 供应商 1 · 虚拟模型 1 · deepseek-v3 · key-c（不同模型，与 akf-3 同模型不同虚拟模型）。
        ApiKeyRow {
            request_id: "akf-4".into(),
            virtual_model_id: 1,
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "deepseek-v3".into(),
            api_key_name: "key-c".into(),
            success: true,
            start_time: t0 + 4,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(400),
        },
    ] {
        insert_ak_row(&db, row).await;
    }
    let window = format!("startTime={t0}&endTime={}", t0 + 2 * HOUR_MS);

    // providerId=1 过滤：只留 akf-1 / akf-3 / akf-4 → key-a/key-b/key-c。
    let (status, json) = get_json(
        app.clone(),
        &format!("/api/stats/api-key-rank?{window}&providerId={DEFAULT_PROVIDER_ID}"),
    )
    .await;
    assert_eq!(status, 200);
    let items = json["data"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 3);

    // virtualModelId=1 过滤：akf-1 / akf-2 / akf-4 → key-a（两笔合计 300）/ key-c。
    let (status, json) = get_json(
        app.clone(),
        &format!("/api/stats/api-key-rank?{window}&virtualModelId=1"),
    )
    .await;
    assert_eq!(status, 200);
    let items = json["data"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    let key_a = items.iter().find(|i| i["apiKeyName"] == "key-a").unwrap();
    assert_eq!(key_a["requestCount"], 2);
    assert_eq!(key_a["totalTokens"], 300);

    // providerId + modelId 过滤：只留 akf-1（provider 1 下 gpt-4o）→ key-a 一笔 100。
    let (status, json) = get_json(
        app,
        &format!(
            "/api/stats/api-key-rank?{window}&providerId={DEFAULT_PROVIDER_ID}&modelId=gpt-4o"
        ),
    )
    .await;
    assert_eq!(status, 200);
    let items = json["data"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["apiKeyName"], "key-a");
    assert_eq!(items[0]["totalTokens"], 100);
}

/// 排序：sortBy 白名单 + asc/desc 生效；非法 sortBy / 缺窗口返回 400。
#[tokio::test]
async fn test_api_key_rank_sort_and_validation() {
    let (app, db) = setup_app().await;
    let t0 = (1_700_000_000_000i64 / HOUR_MS) * HOUR_MS;
    for row in [
        ApiKeyRow {
            request_id: "aks-1".into(),
            virtual_model_id: 1,
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            api_key_name: "key-a".into(),
            success: true,
            start_time: t0 + 1,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(100),
        },
        ApiKeyRow {
            request_id: "aks-2".into(),
            virtual_model_id: 1,
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            api_key_name: "key-b".into(),
            success: true,
            start_time: t0 + 2,
            input_tokens: Some(10),
            input_cache_tokens: 0,
            total_tokens: Some(50),
        },
    ] {
        insert_ak_row(&db, row).await;
    }
    let window = format!("startTime={t0}&endTime={}", t0 + 2 * HOUR_MS);

    // sortBy=totalTokens&sortOrder=asc → key-b(50) 在前。
    let (status, json) = get_json(
        app.clone(),
        &format!("/api/stats/api-key-rank?{window}&sortBy=totalTokens&sortOrder=asc"),
    )
    .await;
    assert_eq!(status, 200);
    let items = json["data"]["items"].as_array().unwrap();
    assert_eq!(items[0]["apiKeyName"], "key-b");

    // sortBy=requestCount（白名单合法）→ 默认降序。
    let (status, _) = get_json(
        app.clone(),
        &format!("/api/stats/api-key-rank?{window}&sortBy=requestCount"),
    )
    .await;
    assert_eq!(status, 200);

    // 非法 sortBy → 400。
    let (status, _) = get_json(
        app.clone(),
        &format!("/api/stats/api-key-rank?{window}&sortBy=badKey"),
    )
    .await;
    assert_eq!(status, 400);

    // 缺窗口 → 400。
    let (status, _) = get_json(app.clone(), "/api/stats/api-key-rank?sortBy=totalTokens").await;
    assert_eq!(status, 400);

    // 过滤组合契约：providerId + virtualModelId 同传 → 400。
    let (status, _) = get_json(
        app.clone(),
        &format!(
            "/api/stats/api-key-rank?{window}&providerId={DEFAULT_PROVIDER_ID}&virtualModelId=1"
        ),
    )
    .await;
    assert_eq!(status, 400);

    // modelId 无 providerId → 400。
    let (status, _) = get_json(
        app,
        &format!("/api/stats/api-key-rank?{window}&modelId=gpt-4o"),
    )
    .await;
    assert_eq!(status, 400);
}

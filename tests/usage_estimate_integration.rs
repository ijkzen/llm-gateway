//! 订阅周期 Token 预估接口（GET /api/providers/{id}/usage/estimate）集成测试。
//!
//! 直接向 provider_usage_cache 表写入 UsageData（模拟用量缓存），
//! 不依赖上游 mock：覆盖可预估 / 覆盖缺口无法预估 / 非订阅制 400 三态。

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sea_orm::{ActiveModelTrait, DatabaseConnection, Set};
use serde_json::{Value, json};
use tower::ServiceExt;

use llm_gateway::entity::{provider, request, usage_cache};

const DAY_MS: i64 = 24 * 3_600_000;

async fn setup_app() -> (axum::Router, DatabaseConnection) {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    scheduler.start().await.unwrap();
    let app = common::build_authed_app(db.clone(), scheduler, log_tx).await;
    (app, db)
}

async fn send_get(app: &axum::Router, uri: &str) -> (StatusCode, Value) {
    let request: Request<Body> = Request::builder().uri(uri).body(Body::empty()).unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

/// 种子供应商（billing_mode 可覆盖）。
async fn seed_provider(db: &DatabaseConnection, id: i32, name: &str, billing_mode: i32) {
    let now = chrono::Utc::now();
    provider::ActiveModel {
        id: Set(id),
        name: Set(name.to_string()),
        enable: Set(true),
        base_url: Set("https://example.com".to_string()),
        api_key: Set("encrypted".to_string()),
        custom_header: Set("{}".to_string()),
        status: Set(0),
        protocol_type: Set(0),
        billing_mode: Set(billing_mode),
        extra: Set(r#"{"usage":true}"#.to_string()),
        sort_order: Set(0),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db)
    .await
    .unwrap();
}

/// 种子一条 request 行（成功，total_tokens 可指定，时间可指定）。
async fn seed_request(
    db: &DatabaseConnection,
    request_id: &str,
    provider_id: i32,
    start_time: i64,
    total_tokens: i64,
) {
    request::ActiveModel {
        request_id: Set(request_id.to_string()),
        virtual_model_id: Set(1),
        provider_id: Set(provider_id),
        model_id: Set("gpt-4o".to_string()),
        stream: Set(false),
        ttft: Set(None),
        input_tokens: Set(Some(100)),
        input_cache_tokens: Set(0),
        input_cache_rate: Set(0.0),
        output_tokens: Set(Some(50)),
        output_tokens_time: Set(Some(500)),
        tps: Set(100.0),
        start_time: Set(start_time),
        end_time: Set(start_time + 1000),
        request_time: Set(1000),
        success: Set(true),
        fail_reason: Set(None),
        total_tokens: Set(Some(total_tokens)),
        api_key_name: Set("itest-key".to_string()),
    }
    .insert(db)
    .await
    .unwrap();
}

/// 写入用量缓存（weekly 窗口：used=50, limit=100，resets_at 指定）。
async fn seed_usage_cache(db: &DatabaseConnection, provider_id: i32, resets_at_ms: i64) {
    let now = chrono::Utc::now();
    let usage_json = json!({
        "providerId": provider_id,
        "fetchedAt": now.to_rfc3339(),
        "kind": "quota",
        "windows": [
            {"window": "five_hour", "available": false},
            {
                "window": "weekly",
                "available": true,
                "used": 50.0,
                "limit": 100.0,
                "remainingPercent": 50.0,
                "usedPercent": 50.0,
                "resetsAt": chrono::DateTime::from_timestamp_millis(resets_at_ms)
                    .unwrap()
                    .to_rfc3339(),
                "unit": "credits"
            },
            {"window": "monthly", "available": false}
        ]
    });
    usage_cache::ActiveModel {
        id: Set(1),
        provider_id: Set(provider_id),
        usage_json: Set(usage_json.to_string()),
        fetched_at: Set(now),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db)
    .await
    .unwrap();
}

/// 可预估：weekly 窗口已过去的天数每天都有请求数据，比例 0.5。
#[tokio::test]
async fn test_estimate_full_coverage() {
    let (app, db) = setup_app().await;
    seed_provider(&db, 1, "sub-provider", 1).await;
    // 窗口 = [now-3天, now+4天]（resets_at 在未来 4 天，窗口起点 = now-3 天）。
    // 已过去区间 = [now-3天, now]，共 4 天。
    let now = chrono::Utc::now().timestamp_millis();
    let resets_at = now + 4 * DAY_MS;
    seed_usage_cache(&db, 1, resets_at).await;

    let window_start = resets_at - 7 * DAY_MS; // = now - 3 天
    // 已过去的 4 个整数天桶（now-3天 .. now）各有一条数据。
    for day in 0..3 {
        seed_request(
            &db,
            &format!("r-day-{day}"),
            1,
            window_start + day * DAY_MS + 1000,
            100,
        )
        .await;
    }
    // 第 4 个桶（now 当天）的数据：明确在 now 之前。
    seed_request(&db, "r-recent", 1, now - 3_600_000, 200).await;

    let (status, body) = send_get(&app, "/api/providers/1/usage/estimate").await;
    assert_eq!(status, StatusCode::OK);
    let data = &body["data"];
    assert_eq!(data["providerId"], 1);
    assert_eq!(data["window"], "weekly");
    assert_eq!(data["estimatable"], true, "已过去时段完整覆盖应可预估：{data}");
    // 已用 token = 3*100 + 200 = 500；比例 0.5 → 预估总量 1000。
    assert_eq!(data["usedTokens"], 500);
    assert_eq!(data["estimatedTotalTokens"], 1000);
    // 应覆盖天数 = 已过去天数 = 4（而非整个窗口 7 天）。
    assert_eq!(data["coveredDays"], 4);
    assert_eq!(data["totalDays"], 4);
}

/// 覆盖缺口：已过去时段内只有 3 天数据（应覆盖 4 天）→ 无法预估。
#[tokio::test]
async fn test_estimate_gap_coverage_not_estimatable() {
    let (app, db) = setup_app().await;
    seed_provider(&db, 1, "sub-provider", 1).await;
    let now = chrono::Utc::now().timestamp_millis();
    let resets_at = now + 4 * DAY_MS;
    seed_usage_cache(&db, 1, resets_at).await;

    // 已过去 4 天中只有 3 天有数据（缺第 4 天）。
    let window_start = resets_at - 7 * DAY_MS; // = now - 3 天
    for day in 0..3 {
        seed_request(&db, &format!("r-{day}"), 1, window_start + day * DAY_MS, 100).await;
    }

    let (status, body) = send_get(&app, "/api/providers/1/usage/estimate").await;
    assert_eq!(status, StatusCode::OK);
    let data = &body["data"];
    assert_eq!(data["estimatable"], false, "覆盖缺口应无法预估：{data}");
    assert_eq!(data["coveredDays"], 3);
    assert_eq!(data["totalDays"], 4);
    assert!(data["estimatedTotalTokens"].is_null(), "无预估值时该字段为 null");
}

/// 非订阅制 → 400。
#[tokio::test]
async fn test_estimate_payg_rejected() {
    let (app, db) = setup_app().await;
    seed_provider(&db, 1, "payg-provider", 0).await;

    let (status, _) = send_get(&app, "/api/providers/1/usage/estimate").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// 无可用窗口（缓存里 weekly/monthly 均不可用）→ 无法预估。
#[tokio::test]
async fn test_estimate_no_window_not_estimatable() {
    let (app, db) = setup_app().await;
    seed_provider(&db, 1, "sub-provider", 1).await;
    let now = chrono::Utc::now();
    let usage_json = json!({
        "providerId": 1,
        "fetchedAt": now.to_rfc3339(),
        "kind": "quota",
        "windows": [
            {"window": "five_hour", "available": false},
            {"window": "weekly", "available": false},
            {"window": "monthly", "available": false}
        ]
    });
    usage_cache::ActiveModel {
        id: Set(1),
        provider_id: Set(1),
        usage_json: Set(usage_json.to_string()),
        fetched_at: Set(now),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(&db)
    .await
    .unwrap();

    let (status, body) = send_get(&app, "/api/providers/1/usage/estimate").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["estimatable"], false);
}

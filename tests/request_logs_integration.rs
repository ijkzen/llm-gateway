//! 请求日志查询接口（GET /api/request-logs）集成测试。

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sea_orm::{ActiveModelTrait, DatabaseConnection, Set};
use serde_json::Value;
use tower::ServiceExt;

use llm_gateway::entity::{provider, request};

/// 种入一条 provider 行（指定 id/name），供列表 JOIN 断言供应商名。
async fn seed_provider(db: &DatabaseConnection, id: i32, name: &str) {
    provider::ActiveModel {
        id: Set(id),
        name: Set(name.to_string()),
        enable: Set(true),
        base_url: Set("https://example.com".to_string()),
        api_key: Set(llm_gateway::crypto::encrypt("sk-test")),
        custom_header: Set("{}".to_string()),
        protocol_type: Set(0),
        billing_mode: Set(0),
        extra: Set("{}".to_string()),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap();
}

async fn setup_app() -> (axum::Router, DatabaseConnection) {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    let app = common::build_authed_app(db.clone(), scheduler, log_tx).await;
    (app, db)
}

/// 种入一条 request 行（可覆盖供应商/模型）。
#[allow(clippy::too_many_arguments)]
async fn seed_request_full(
    db: &DatabaseConnection,
    request_id: &str,
    vm_id: i32,
    provider_id: i32,
    model_id: &str,
    api_key: &str,
    start_time: i64,
    success: bool,
) {
    request::ActiveModel {
        request_id: Set(request_id.to_string()),
        virtual_model_id: Set(vm_id),
        provider_id: Set(provider_id),
        model_id: Set(model_id.to_string()),
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
        success: Set(success),
        fail_reason: Set(None),
        total_tokens: Set(Some(150)),
        api_key_name: Set(api_key.to_string()),
    }
    .insert(db)
    .await
    .unwrap();
}

/// 种入一条 request 行（默认 provider 1 / 模型 gpt-4o）。
async fn seed_request(
    db: &DatabaseConnection,
    request_id: &str,
    vm_id: i32,
    api_key: &str,
    start_time: i64,
    success: bool,
) {
    seed_request_full(
        db, request_id, vm_id, 1, "gpt-4o", api_key, start_time, success,
    )
    .await;
}

async fn get(app: &axum::Router, uri: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn test_pagination_and_defaults() {
    let (app, db) = setup_app().await;
    // 5 条：vm 1 三条、vm 2 两条；时间错开。
    for i in 0..5 {
        seed_request(
            &db,
            &format!("req-{i}"),
            if i < 3 { 1 } else { 2 },
            "key-a",
            1_700_000_000_000 + i * 1000,
            i % 2 == 0,
        )
        .await;
    }

    // 默认分页 pageSize=20 应返回全部 5 条，total=5。
    let (status, body) = get(&app, "/api/request-logs").await;
    assert_eq!(status, StatusCode::OK);
    let data = &body["data"];
    assert_eq!(data["total"], 5);
    assert_eq!(data["page"], 1);
    assert_eq!(data["pageSize"], 20);
    assert_eq!(data["items"].as_array().unwrap().len(), 5);
    // 默认 start_time DESC：req-4 最前。
    assert_eq!(data["items"][0]["requestId"], "req-4");
}

#[tokio::test]
async fn test_filters_vm_api_key_time() {
    let (app, db) = setup_app().await;
    seed_request(&db, "r1", 1, "key-a", 1_700_000_000_000, true).await;
    seed_request(&db, "r2", 1, "key-b", 1_700_000_100_000, true).await;
    seed_request(&db, "r3", 2, "key-a", 1_700_000_200_000, true).await;

    // 按虚拟模型过滤。
    let (status, body) = get(&app, "/api/request-logs?vmId=1").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["total"], 2);

    // 按 API Key 过滤。
    let (status, body) = get(&app, "/api/request-logs?apiKey=key-a").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["total"], 2);

    // 时间段过滤（start_time 范围）。
    let (status, body) = get(
        &app,
        "/api/request-logs?startTime=1700000100000&endTime=1700000150000",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["total"], 1);
    assert_eq!(body["data"]["items"][0]["requestId"], "r2");

    // 组合：vm + apiKey。
    let (status, body) = get(&app, "/api/request-logs?vmId=1&apiKey=key-a").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["total"], 1);
    assert_eq!(body["data"]["items"][0]["requestId"], "r1");
}

#[tokio::test]
async fn test_filters_provider_model_success() {
    let (app, db) = setup_app().await;
    // provider 1 / gpt-4o（成功）、provider 1 / gpt-4o（失败）、provider 2 / claude（成功）。
    seed_request_full(
        &db,
        "p1-ok",
        1,
        1,
        "gpt-4o",
        "key-a",
        1_700_000_000_000,
        true,
    )
    .await;
    seed_request_full(
        &db,
        "p1-bad",
        1,
        1,
        "gpt-4o",
        "key-a",
        1_700_000_100_000,
        false,
    )
    .await;
    seed_request_full(
        &db,
        "p2-ok",
        1,
        2,
        "claude-3",
        "key-a",
        1_700_000_200_000,
        true,
    )
    .await;

    // 按供应商过滤。
    let (status, body) = get(&app, "/api/request-logs?providerId=2").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["total"], 1);
    assert_eq!(body["data"]["items"][0]["requestId"], "p2-ok");

    // 按供应商模型过滤。
    let (status, body) = get(&app, "/api/request-logs?modelId=claude-3").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["total"], 1);
    assert_eq!(body["data"]["items"][0]["requestId"], "p2-ok");

    // 按结果状态过滤：成功。
    let (status, body) = get(&app, "/api/request-logs?success=true").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["total"], 2);

    // 失败。
    let (status, body) = get(&app, "/api/request-logs?success=false").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["total"], 1);
    assert_eq!(body["data"]["items"][0]["requestId"], "p1-bad");

    // 组合：供应商 + 模型 + 结果。
    let (status, body) = get(
        &app,
        "/api/request-logs?providerId=1&modelId=gpt-4o&success=true",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["total"], 1);
    assert_eq!(body["data"]["items"][0]["requestId"], "p1-ok");
}

#[tokio::test]
async fn test_multi_value_filters() {
    let (app, db) = setup_app().await;
    seed_request(&db, "r1", 1, "key-a", 1_700_000_000_000, true).await;
    seed_request(&db, "r2", 2, "key-b", 1_700_000_100_000, true).await;
    seed_request(&db, "r3", 3, "key-c", 1_700_000_200_000, true).await;

    // vmId 逗号分隔多值 = IN 语义（并集）。
    let (_, body) = get(&app, "/api/request-logs?vmId=1,2").await;
    assert_eq!(body["data"]["total"], 2);
    let ids: Vec<&str> = body["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["requestId"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["r2", "r1"]);

    // 单值形式兼容。
    let (_, body) = get(&app, "/api/request-logs?vmId=3").await;
    assert_eq!(body["data"]["total"], 1);
    assert_eq!(body["data"]["items"][0]["requestId"], "r3");

    // apiKey 多值 + 空段忽略。
    let (_, body) = get(&app, "/api/request-logs?apiKey=key-a,,key-c").await;
    assert_eq!(body["data"]["total"], 2);

    // 多值过滤缺省 = 不过滤。
    let (_, body) = get(&app, "/api/request-logs?apiKey=").await;
    assert_eq!(body["data"]["total"], 3);
}

#[tokio::test]
async fn test_multi_value_provider_and_model_filters() {
    let (app, db) = setup_app().await;
    seed_request_full(&db, "m1", 1, 1, "gpt-4o", "key-a", 1_700_000_000_000, true).await;
    seed_request_full(
        &db,
        "m2",
        1,
        1,
        "claude-3",
        "key-a",
        1_700_000_100_000,
        true,
    )
    .await;
    seed_request_full(&db, "m3", 1, 2, "gpt-4o", "key-a", 1_700_000_200_000, true).await;

    // providerId 多值。
    let (_, body) = get(&app, "/api/request-logs?providerId=1,2").await;
    assert_eq!(body["data"]["total"], 3);

    // modelId 多值。
    let (_, body) = get(&app, "/api/request-logs?modelId=gpt-4o,claude-3").await;
    assert_eq!(body["data"]["total"], 3);

    // 组合：providerId + modelId 多值交集。
    let (_, body) = get(&app, "/api/request-logs?providerId=1&modelId=claude-3").await;
    assert_eq!(body["data"]["total"], 1);
    assert_eq!(body["data"]["items"][0]["requestId"], "m2");
}

#[tokio::test]
async fn test_sorting_and_page_size() {
    let (app, db) = setup_app().await;
    seed_request(&db, "a", 1, "k", 1_700_000_000_000, true).await;
    seed_request(&db, "b", 1, "k", 1_700_000_300_000, true).await;
    seed_request(&db, "c", 1, "k", 1_700_000_600_000, true).await;

    // 升序。
    let (_, body) = get(&app, "/api/request-logs?sortBy=startTime&sortOrder=asc").await;
    assert_eq!(body["data"]["items"][0]["requestId"], "a");

    // 降序 + pageSize=2 + page=2 → 最后 1 条。
    let (_, body) = get(
        &app,
        "/api/request-logs?page=2&pageSize=2&sortBy=startTime&sortOrder=desc",
    )
    .await;
    assert_eq!(body["data"]["total"], 3);
    assert_eq!(body["data"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(body["data"]["items"][0]["requestId"], "a");

    // 非法排序字段回落默认。
    let (status, body) = get(&app, "/api/request-logs?sortBy=drop;table").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["total"], 3);
}

#[tokio::test]
async fn test_rows_include_all_fields() {
    let (app, db) = setup_app().await;
    seed_request(&db, "full", 1, "key-a", 1_700_000_000_000, false).await;

    let (_, body) = get(&app, "/api/request-logs").await;
    let item = &body["data"]["items"][0];
    // 关键字段齐全（详情弹窗依赖全字段）。
    for key in [
        "requestId",
        "virtualModelId",
        "providerId",
        "providerName",
        "modelId",
        "stream",
        "ttft",
        "inputTokens",
        "inputCacheTokens",
        "inputCacheRate",
        "outputTokens",
        "outputTokensTime",
        "tps",
        "startTime",
        "endTime",
        "requestTime",
        "success",
        "failReason",
        "totalTokens",
        "apiKeyName",
    ] {
        assert!(item.get(key).is_some(), "缺少字段 {key}");
    }
}

#[tokio::test]
async fn test_provider_name_joined_from_provider_table() {
    let (app, db) = setup_app().await;
    // provider 1 有名字、provider 2 无记录（如已删除）。
    seed_provider(&db, 1, "Provider Alpha").await;
    seed_request_full(
        &db,
        "r-with-p",
        1,
        1,
        "gpt-4o",
        "key-a",
        1_700_000_000_000,
        true,
    )
    .await;
    seed_request_full(
        &db,
        "r-orphan",
        1,
        2,
        "claude-3",
        "key-a",
        1_700_000_100_000,
        true,
    )
    .await;

    let (_, body) = get(&app, "/api/request-logs").await;
    let items = body["data"]["items"].as_array().unwrap();

    let with_provider = items.iter().find(|i| i["requestId"] == "r-with-p").unwrap();
    assert_eq!(with_provider["providerName"], "Provider Alpha");

    let orphan = items.iter().find(|i| i["requestId"] == "r-orphan").unwrap();
    assert_eq!(orphan["providerId"], 2);
    // 供应商不存在时 providerName 为 null，前端兜底 #providerId。
    assert!(orphan["providerName"].is_null());
}

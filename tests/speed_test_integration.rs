//! 模型测速集成测试（ADR-0027）：候选过滤、批量运行、失败行、取消、重启恢复
//! 与统计口径排除。mock 上游返回 OpenAI 兼容 SSE（内容块 + usage 尾块）。
#![allow(clippy::await_holding_lock)]

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode as HttpStatus};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde_json::{Value, json};
use tower::ServiceExt;

use llm_gateway::crypto;
use llm_gateway::entity::{
    provider, provider_model, request, speed_test_result, speed_test_run, virtual_model,
    virtual_model_item,
};

type Captured = Arc<Mutex<Vec<Value>>>;

fn capture() -> Captured {
    Arc::new(Mutex::new(Vec::new()))
}

fn record_capture(captured: &Captured, body: &str) {
    if let Ok(value) = serde_json::from_str::<Value>(body) {
        captured.lock().unwrap().push(value);
    }
}

/// mock 上游行为：正常吐 3 个内容 chunk + usage 尾块，或直接 500。
///
/// `Ok` 在块之间留 20ms、`Slow` 留 150ms：瞬时返回会让 TTFT 与输出窗口
/// 都落在同一毫秒里，TPS 分母为 0（真实上游不会这样）；`Slow` 用于需要
/// 「运行还在跑」的时间窗场景（单实例、取消）。
#[derive(Clone, Copy, PartialEq)]
enum MockMode {
    Ok,
    Slow,
    Fail,
}

impl MockMode {
    fn chunk_gap(self) -> Option<Duration> {
        match self {
            MockMode::Ok => Some(Duration::from_millis(20)),
            MockMode::Slow => Some(Duration::from_millis(150)),
            MockMode::Fail => None,
        }
    }
}

async fn sse_ok(gap: Option<Duration>) -> Response {
    let events = [
        r#"{"id":"c1","model":"m-1","choices":[{"index":0,"delta":{"content":"hello"}}]}"#,
        r#"{"id":"c1","model":"m-1","choices":[{"index":0,"delta":{"content":" world"}}]}"#,
        r#"{"id":"c1","model":"m-1","choices":[{"index":0,"delta":{"content":"!"},"finish_reason":"stop"}]}"#,
        r#"{"id":"c1","model":"m-1","choices":[],"usage":{"prompt_tokens":10,"completion_tokens":40,"total_tokens":50}}"#,
        "[DONE]",
    ];
    let mut payload = String::new();
    for (index, event) in events.iter().enumerate() {
        if index > 0
            && let Some(gap) = gap
        {
            tokio::time::sleep(gap).await;
        }
        payload.push_str(&format!("data: {event}\n\n"));
    }
    (
        HttpStatus::OK,
        [("content-type", "text/event-stream")],
        payload,
    )
        .into_response()
}

async fn spawn_mock(captured: Captured, mode: MockMode) -> String {
    let app = Router::new().route(
        "/v1/chat/completions",
        post(move |body: String| {
            let captured = captured.clone();
            async move {
                record_capture(&captured, &body);
                match mode {
                    MockMode::Ok | MockMode::Slow => sse_ok(mode.chunk_gap()).await,
                    MockMode::Fail => (
                        HttpStatus::INTERNAL_SERVER_ERROR,
                        Json(json!({"error": {"message": "upstream boom"}})),
                    )
                        .into_response(),
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

async fn setup_app() -> (axum::Router, sea_orm::DatabaseConnection) {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    scheduler.start().await.unwrap();
    let app = common::build_authed_app(db.clone(), scheduler, log_tx).await;
    (app, db)
}

async fn seed_provider(
    db: &sea_orm::DatabaseConnection,
    name: &str,
    base_url: &str,
    enable: bool,
    disabled_reason: Option<&str>,
) -> i32 {
    provider::ActiveModel {
        name: Set(name.to_string()),
        enable: Set(enable),
        base_url: Set(base_url.to_string()),
        api_key: Set(crypto::encrypt("sk-test")),
        custom_header: Set("{}".to_string()),
        protocol_type: Set(0),
        billing_mode: Set(0),
        extra: Set("{}".to_string()),
        disabled_reason: Set(disabled_reason.map(str::to_string)),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap()
    .id
}

async fn seed_model(
    db: &sea_orm::DatabaseConnection,
    provider_id: i32,
    provider_model_id: &str,
    max_output_tokens: i64,
) -> i32 {
    provider_model::ActiveModel {
        provider_id: Set(provider_id),
        provider_model_id: Set(provider_model_id.to_string()),
        context_length: Set(128_000),
        max_output_tokens: Set(max_output_tokens),
        reasoning: Set(false),
        tool_use: Set(true),
        image_understand: Set(false),
        video_understand: Set(false),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap()
    .model_id
}

async fn seed_virtual_model(
    db: &sea_orm::DatabaseConnection,
    display_id: &str,
    model_pks: &[i32],
) -> i32 {
    let vm_id = virtual_model::ActiveModel {
        display_id: Set(display_id.to_string()),
        enable: Set(true),
        load_balancing_strategy: Set(0),
        fallback_strategy: Set(0),
        interface_type: Set(0),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap()
    .virtual_model_id;
    for model_pk in model_pks {
        virtual_model_item::ActiveModel {
            virtual_model_id: Set(vm_id),
            model_id: Set(*model_pk),
            enable: Set(true),
            cascade_disabled: Set(false),
            created_at: Set(chrono::Utc::now()),
            updated_at: Set(chrono::Utc::now()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
    }
    vm_id
}

async fn get_json(app: &axum::Router, uri: &str) -> (u16, Value) {
    let response = app
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status().as_u16();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn post_json(app: &axum::Router, uri: &str, body: Value) -> (u16, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status().as_u16();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// 等待 request 表落库到至少 `expected` 行。
///
/// 指标走进程级单写者（批 50 行 / 空闲 200ms 冲刷），运行结束后行可能还在
/// 队列里；订阅落库事件做同步，事件先于订阅到达的行由首查存量覆盖。
async fn wait_for_request_rows(
    db: &sea_orm::DatabaseConnection,
    expected: usize,
) -> Vec<request::Model> {
    let mut rx = llm_gateway::proxy::metrics::subscribe_request_writes();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let rows = request::Entity::find().all(db).await.unwrap();
        if rows.len() >= expected {
            return rows;
        }
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return rows;
        }
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Ok(())) | Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => {}
            Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) | Err(_) => return rows,
        }
    }
}

/// 轮询等到运行进入终态（取数接口是前端 2s 轮询的同一入口）。
async fn wait_for_run_end(app: &axum::Router) -> Value {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        let (status, body) = get_json(app, "/api/speed-test/run").await;
        assert_eq!(status, 200, "{body}");
        let run = &body["data"]["run"];
        if !run.is_null() {
            let state = run["status"].as_str().unwrap_or_default();
            if state != "running" {
                return body["data"].clone();
            }
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "测速运行未在预算内结束：{body}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn previews_only_enabled_providers_models() {
    let captured = capture();
    let base = spawn_mock(captured, MockMode::Ok).await;
    let (app, db) = setup_app().await;

    let enabled = seed_provider(&db, "启用供应商", &base, true, None).await;
    seed_model(&db, enabled, "m-enabled", 2048).await;

    let quota_disabled = seed_provider(&db, "额度耗尽供应商", &base, false, Some("quota")).await;
    seed_model(&db, quota_disabled, "m-quota", 2048).await;

    let manual_disabled = seed_provider(&db, "手动停用供应商", &base, false, Some("manual")).await;
    seed_model(&db, manual_disabled, "m-manual", 2048).await;

    let (status, body) = get_json(&app, "/api/speed-test/candidates").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["data"]["total"], 1, "只应命中启用供应商的模型");
    assert_eq!(body["data"]["providers"][0]["providerName"], "启用供应商");
    assert_eq!(body["data"]["providers"][0]["count"], 1);
    // 候选清单直接可渲染（筛选一变化就展示会测谁，不必等跑过测速）。
    assert_eq!(body["data"]["models"][0]["modelId"], "m-enabled");
    assert_eq!(body["data"]["models"][0]["providerName"], "启用供应商");
}

/// 运行响应带回筛选快照：前端据此判断「当前结果是否属于当前筛选」。
#[tokio::test]
async fn run_response_carries_filter_snapshot() {
    let captured = capture();
    let base = spawn_mock(captured, MockMode::Ok).await;
    let (app, db) = setup_app().await;
    let p = seed_provider(&db, "p-snapshot", &base, true, None).await;
    let model_pk = seed_model(&db, p, "m-snap", 2048).await;
    let vm_id = seed_virtual_model(&db, "vm-snap", &[model_pk]).await;

    let (status, body) = post_json(
        &app,
        "/api/speed-test/run",
        json!({"providerIds": [p], "vmIds": [vm_id], "keyword": "m-snap"}),
    )
    .await;
    assert_eq!(status, 200, "{body}");

    let data = wait_for_run_end(&app).await;
    let run = &data["run"];
    assert_eq!(run["providerIds"][0], p);
    assert_eq!(run["vmIds"][0], vm_id);
    assert_eq!(run["keyword"], "m-snap");
}

#[tokio::test]
async fn previews_filter_by_keyword_and_virtual_model() {
    let captured = capture();
    let base = spawn_mock(captured, MockMode::Ok).await;
    let (app, db) = setup_app().await;

    let p1 = seed_provider(&db, "云雾", &base, true, None).await;
    let claude_pk = seed_model(&db, p1, "claude-sonnet", 2048).await;
    let gpt_pk = seed_model(&db, p1, "gpt-4o", 2048).await;
    let vm_id = seed_virtual_model(&db, "prod-gpt", &[gpt_pk]).await;

    // 关键词命中模型名
    let (_, body) = get_json(&app, "/api/speed-test/candidates?keyword=claude").await;
    assert_eq!(body["data"]["total"], 1);
    assert_eq!(
        body["data"]["providers"][0]["providerName"], "云雾",
        "供应商级计数按供应商名展示"
    );

    // 关键词命中供应商名
    let (_, body) = get_json(&app, "/api/speed-test/candidates?keyword=云雾").await;
    assert_eq!(body["data"]["total"], 2);

    // 关键词命中虚拟模型 display_id
    let (_, body) = get_json(&app, "/api/speed-test/candidates?keyword=PROD").await;
    assert_eq!(body["data"]["total"], 1, "只命中被 prod-gpt 引用的模型");

    // 虚拟模型过滤
    let (_, body) = get_json(&app, &format!("/api/speed-test/candidates?vmIds={vm_id}")).await;
    assert_eq!(body["data"]["total"], 1);

    // 供应商过滤（命中 p1 的两个模型，未命中 claude 过滤组合）
    let (_, body) = get_json(
        &app,
        &format!("/api/speed-test/candidates?providerIds={p1}&keyword=claude"),
    )
    .await;
    assert_eq!(body["data"]["total"], 1);

    let _ = claude_pk;
}

#[tokio::test]
async fn run_completes_with_tps_and_request_row_marked_test() {
    let captured = capture();
    let base = spawn_mock(captured.clone(), MockMode::Ok).await;
    let (app, db) = setup_app().await;
    let p = seed_provider(&db, "p-ok", &base, true, None).await;
    seed_model(&db, p, "m-1", 1024).await;

    let (status, body) = post_json(&app, "/api/speed-test/run", json!({})).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["data"]["alreadyRunning"], false);
    assert_eq!(body["data"]["total"], 1);

    let data = wait_for_run_end(&app).await;
    assert_eq!(data["run"]["status"], "completed");
    assert_eq!(data["results"].as_array().unwrap().len(), 1);

    let row = &data["results"][0];
    assert_eq!(row["status"], "done");
    assert_eq!(row["providerName"], "p-ok");
    assert_eq!(row["modelId"], "m-1");
    assert_eq!(row["outputTokens"], 40);
    let tps = row["tps"].as_f64().expect("成功行应带 TPS");
    assert!(tps > 0.0, "TPS 应大于 0，实际 {tps}");
    row["requestId"].as_str().expect("成功行应带 request_id");

    // 上游收到长文题面 + 流式 + 模型登记的 max_tokens（不再夹取，1024 原样透传）。
    let bodies = captured.lock().unwrap();
    assert_eq!(bodies[0]["stream"], true);
    assert_eq!(bodies[0]["max_tokens"], 1024);
    let prompt = bodies[0]["messages"][0]["content"].as_str().unwrap();
    assert!(prompt.contains("编程语言"), "题面应为内置长文指令");
    drop(bodies);

    // request 表：api_key_name='test' + request_id 前缀 speedtest-。
    let rows = wait_for_request_rows(&db, 1).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].api_key_name, "test");
    assert_eq!(rows[0].virtual_model_id, 0);
    assert!(
        rows[0].request_id.starts_with("speedtest-"),
        "request_id 应为测速前缀：{}",
        rows[0].request_id
    );
    assert!(rows[0].ttft.is_some(), "流式测速应记录 TTFT");
}

#[tokio::test]
async fn upstream_failure_marks_row_failed_but_run_completes() {
    let captured = capture();
    let base = spawn_mock(captured, MockMode::Fail).await;
    let (app, db) = setup_app().await;
    let p = seed_provider(&db, "p-fail", &base, true, None).await;
    seed_model(&db, p, "m-bad", 2048).await;

    let (status, _) = post_json(&app, "/api/speed-test/run", json!({})).await;
    assert_eq!(status, 200);

    let data = wait_for_run_end(&app).await;
    assert_eq!(
        data["run"]["status"], "completed",
        "单模型失败不应中断整次运行"
    );
    let row = &data["results"][0];
    assert_eq!(row["status"], "failed");
    let reason = row["failReason"].as_str().unwrap_or_default();
    assert!(reason.contains("500"), "失败原因应带状态码：{reason}");
    assert!(row["tps"].is_null(), "失败行不应有 TPS");
    row["requestId"].as_str().expect("失败行也应带 request_id");

    // 失败也照常落 request 表（供审计），且仍是 test 标记。
    let rows = wait_for_request_rows(&db, 1).await;
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].success);
    assert_eq!(rows[0].api_key_name, "test");
}

#[tokio::test]
async fn cancel_marks_run_cancelled_and_leaves_no_pending_rows() {
    let captured = capture();
    let base = spawn_mock(captured, MockMode::Slow).await;
    let (app, db) = setup_app().await;
    let p = seed_provider(&db, "p-cancel", &base, true, None).await;
    // 无并发上限：4 个模型一开跑就全部在飞。取消此时只能把「尚未开始」的行
    // 标记掉（可能一行都没有），在飞的自然结束并回写成绩——因此这里断言的是
    // 「运行按取消收尾」与「不残留 pending」，而不是「一定有 cancelled 行」。
    for i in 0..4 {
        seed_model(&db, p, &format!("m-{i}"), 2048).await;
    }

    let (status, body) = post_json(&app, "/api/speed-test/run", json!({})).await;
    assert_eq!(status, 200, "{body}");

    let (status, body) = post_json(&app, "/api/speed-test/run/cancel", json!({})).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["data"]["cancelled"], true);

    let data = wait_for_run_end(&app).await;
    assert_eq!(data["run"]["status"], "cancelled");
    let results = data["results"].as_array().unwrap();
    assert_eq!(results.len(), 4, "占位行从开始就是候选总数");
    assert!(
        results.iter().all(|r| r["status"] != "pending"),
        "运行结束后不应残留 pending 行：{data}"
    );
    assert!(
        results
            .iter()
            .all(|r| ["done", "failed", "cancelled"].contains(&r["status"].as_str().unwrap())),
        "每行都应处于终态：{data}"
    );
}

#[tokio::test]
async fn second_start_while_running_reports_already_running() {
    let captured = capture();
    let base = spawn_mock(captured, MockMode::Slow).await;
    let (app, db) = setup_app().await;
    let p = seed_provider(&db, "p-single", &base, true, None).await;
    for i in 0..3 {
        seed_model(&db, p, &format!("m-{i}"), 2048).await;
    }

    let (_, first) = post_json(&app, "/api/speed-test/run", json!({})).await;
    assert_eq!(first["data"]["alreadyRunning"], false);

    let (status, second) = post_json(&app, "/api/speed-test/run", json!({})).await;
    assert_eq!(status, 200, "重复点击不应报错");
    assert_eq!(second["data"]["alreadyRunning"], true);

    wait_for_run_end(&app).await;
}

#[tokio::test]
async fn empty_candidate_set_is_rejected() {
    let captured = capture();
    let base = spawn_mock(captured, MockMode::Ok).await;
    let (app, db) = setup_app().await;
    let p = seed_provider(&db, "p-off", &base, false, Some("manual")).await;
    seed_model(&db, p, "m-off", 2048).await;

    let (status, body) = post_json(&app, "/api/speed-test/run", json!({})).await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["code"], "INVALID_INPUT");

    // 不应留下空运行。
    let runs = speed_test_run::Entity::find().all(&db).await.unwrap();
    assert!(runs.is_empty());
}

#[tokio::test]
async fn restart_recovery_marks_running_as_interrupted() {
    let (_, db) = setup_app().await;
    let run = speed_test_run::ActiveModel {
        status: Set("running".to_string()),
        started_at: Set(1_700_000_000_000),
        finished_at: Set(None),
        total: Set(2),
        provider_ids: Set("[]".to_string()),
        vm_ids: Set("[]".to_string()),
        keyword: Set(String::new()),
        prompt: Set("题面".to_string()),
        max_tokens: Set(2048),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();

    llm_gateway::speedtest::recover_on_startup(&db).await;

    let loaded = speed_test_run::Entity::find_by_id(run.id)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.status, "interrupted");
    assert!(loaded.finished_at.is_some());
}

#[tokio::test]
async fn new_run_replaces_previous_results() {
    let captured = capture();
    let base = spawn_mock(captured, MockMode::Ok).await;
    let (app, db) = setup_app().await;
    let p = seed_provider(&db, "p-replace", &base, true, None).await;
    seed_model(&db, p, "m-1", 2048).await;

    post_json(&app, "/api/speed-test/run", json!({})).await;
    let first = wait_for_run_end(&app).await;
    let first_id = first["run"]["id"].as_i64().unwrap();

    post_json(&app, "/api/speed-test/run", json!({})).await;
    let second = wait_for_run_end(&app).await;
    let second_id = second["run"]["id"].as_i64().unwrap();
    assert_ne!(first_id, second_id, "新运行应产生新 id");

    // 只留最近一次：旧运行与旧结果行都不在了。
    let runs = speed_test_run::Entity::find().all(&db).await.unwrap();
    assert_eq!(runs.len(), 1, "单行语义：只留最近一次运行");
    assert_eq!(runs[0].id, second_id as i32);

    let stale = speed_test_result::Entity::find()
        .filter(speed_test_result::Column::RunId.eq(first_id as i32))
        .all(&db)
        .await
        .unwrap();
    assert!(stale.is_empty(), "旧运行的结果行应被清理");
}

#[tokio::test]
async fn speed_test_traffic_is_excluded_from_stats() {
    let captured = capture();
    let base = spawn_mock(captured, MockMode::Ok).await;
    let (app, db) = setup_app().await;
    let p = seed_provider(&db, "p-stats", &base, true, None).await;
    seed_model(&db, p, "m-1", 2048).await;

    post_json(&app, "/api/speed-test/run", json!({})).await;
    wait_for_run_end(&app).await;

    // request 表确有测速行……
    let rows = wait_for_request_rows(&db, 1).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].total_tokens, Some(50));

    // ……但数据面板统计口径排除（ADR-0024）。
    let (status, body) = get_json(&app, "/api/stats/summary").await;
    assert_eq!(status, 200, "{body}");
    let request_count = body["data"]["requestCount"].as_f64().unwrap_or(0.0);
    assert_eq!(request_count, 0.0, "测速流量不应计入数据面板：{body}");
}

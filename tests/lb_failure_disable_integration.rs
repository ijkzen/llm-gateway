//! 连续失败熔断 + 失败复查集成测试：成员失败计入内存连续失败计数，达到阈值后
//! 熔断停用供应商及其虚拟模型子模型并打 failure_disabled 标记；成功清零；设置项
//! max_consecutive_failures 可配置且热生效；失败复查在失败后实时核验用量，
//! 耗尽走额度门控禁用（可自动恢复），充足不动。

mod common;

use std::sync::Arc;
use std::sync::Mutex;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde_json::json;
use tower::ServiceExt;

use llm_gateway::entity::{
    provider, provider_model, usage_cache, virtual_model, virtual_model_item,
};

const TEST_BEARER: &str = "Bearer itest-key";
/// 用量请求重定向到本地 mock 的环境变量。
const USAGE_OVERRIDE_ENV: &str = "LLM_GATEWAY_USAGE_HTTP_OVERRIDE";

/// 恒定返回 500 的 mock 上游。
async fn spawn_fail_mock() -> String {
    let app = Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": "boom"}})),
            )
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

/// 恒定返回 400（非可重试 4xx）的 mock 上游。
async fn spawn_bad_request_mock() -> String {
    let app = Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": {"message": "bad input"}})),
            )
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

/// 成功返回补全的 mock 上游。
async fn spawn_ok_mock() -> String {
    let app = Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            Json(json!({
                "id": "chatcmpl-ok",
                "object": "chat.completion",
                "model": "m-ok",
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "hi"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
            }))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

async fn seed_provider(db: &sea_orm::DatabaseConnection, name: &str, base_url: &str) -> i32 {
    provider::ActiveModel {
        name: Set(name.to_string()),
        enable: Set(true),
        base_url: Set(base_url.to_string()),
        api_key: Set(llm_gateway::crypto::encrypt("sk-mock")),
        custom_header: Set("{}".to_string()),
        status: Set(0),
        protocol_type: Set(0),
        billing_mode: Set(0),
        extra: Set("{}".to_string()),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap()
    .id
}

/// 供应商 + 模型 + 单成员虚拟模型（fallback 0：直接失败不降级）。
async fn seed_vm_with_member(
    db: &sea_orm::DatabaseConnection,
    provider_id: i32,
    display_id: &str,
) -> i32 {
    let model_id = provider_model::ActiveModel {
        provider_id: Set(provider_id),
        provider_model_id: Set("m-1".to_string()),
        context_length: Set(128000),
        max_output_tokens: Set(4096),
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
    .model_id;

    let vm = virtual_model::ActiveModel {
        display_id: Set(display_id.to_string()),
        enable: Set(true),
        load_balancing_strategy: Set(3),
        fallback_strategy: Set(0),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap();

    virtual_model_item::ActiveModel {
        virtual_model_id: Set(vm.virtual_model_id),
        model_id: Set(model_id),
        enable: Set(true),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap();

    model_id
}

async fn send_chat(app: &axum::Router, model: &str) -> u16 {
    let body = json!({
        "model": model,
        "stream": false,
        "messages": [{"role": "user", "content": "hi"}],
        "max_tokens": 16,
    });
    let request = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .header("authorization", TEST_BEARER)
        .body(Body::from(body.to_string()))
        .unwrap();
    app.clone()
        .oneshot(request)
        .await
        .unwrap()
        .status()
        .as_u16()
}

async fn send_setting(app: &axum::Router, value: &str) -> StatusCode {
    let request = Request::builder()
        .method("PUT")
        .uri("/api/settings/max_consecutive_failures")
        .header("content-type", "application/json")
        .body(Body::from(json!({"value": value}).to_string()))
        .unwrap();
    app.clone().oneshot(request).await.unwrap().status()
}

async fn provider_row(db: &sea_orm::DatabaseConnection, id: i32) -> provider::Model {
    provider::Entity::find_by_id(id)
        .one(db)
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn consecutive_failures_reach_threshold_disable_provider() {
    let base = spawn_fail_mock().await;
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    scheduler.start().await.unwrap();
    let app = common::build_authed_app(db.clone(), scheduler, log_tx).await;
    let pid = seed_provider(&db, "p-fail", &base).await;
    let model_id = seed_vm_with_member(&db, pid, "vm-break").await;

    // 默认阈值 5：前 4 次失败不禁用。
    for _ in 0..4 {
        let status = send_chat(&app, "vm-break").await;
        assert_eq!(status, 500);
    }
    let row = provider_row(&db, pid).await;
    assert!(row.enable, "未达阈值不应停用");
    assert!(!row.failure_disabled);

    // 第 5 次连续失败 → 熔断：供应商 + 子模型停用 + 标记。
    let status = send_chat(&app, "vm-break").await;
    assert_eq!(status, 500);
    let row = provider_row(&db, pid).await;
    assert!(!row.enable, "达到阈值应停用供应商");
    assert!(row.failure_disabled, "应打上 failure_disabled 标记");
    let item = virtual_model_item::Entity::find()
        .filter(virtual_model_item::Column::ModelId.eq(model_id))
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert!(!item.enable, "子模型应被级联停用");
    assert!(item.cascade_disabled, "子模型应带级联停用标记");
}

#[tokio::test]
async fn success_resets_failure_counter() {
    let fail_base = spawn_fail_mock().await;
    let ok_base = spawn_ok_mock().await;
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    scheduler.start().await.unwrap();
    let app = common::build_authed_app(db.clone(), scheduler, log_tx).await;

    // 供应商先对失败地址，成功后再切回成功地址（模拟恢复）。
    let pid = seed_provider(&db, "p-mixed", &fail_base).await;
    seed_vm_with_member(&db, pid, "vm-mixed").await;

    // 失败 4 次（差一次到阈值）。
    for _ in 0..4 {
        send_chat(&app, "vm-mixed").await;
    }
    let row = provider_row(&db, pid).await;
    assert!(row.enable);

    // 切到成功地址，一次成功清零计数。
    let mut active: provider::ActiveModel = provider::Entity::find_by_id(pid)
        .one(&db)
        .await
        .unwrap()
        .unwrap()
        .into();
    active.base_url = Set(ok_base);
    active.update(&db).await.unwrap();
    let status = send_chat(&app, "vm-mixed").await;
    assert_eq!(status, 200, "成功地址应返回 200");

    // 再失败 4 次（若未清零，累计 9 次早已触发熔断）。
    let mut active: provider::ActiveModel = provider::Entity::find_by_id(pid)
        .one(&db)
        .await
        .unwrap()
        .unwrap()
        .into();
    active.base_url = Set(fail_base);
    active.update(&db).await.unwrap();
    for _ in 0..4 {
        let status = send_chat(&app, "vm-mixed").await;
        assert_eq!(status, 500);
    }
    let row = provider_row(&db, pid).await;
    assert!(row.enable, "成功清零后不应触发熔断");
    assert!(!row.failure_disabled);
}

#[tokio::test]
async fn threshold_setting_configurable_and_hot() {
    let base = spawn_fail_mock().await;
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    scheduler.start().await.unwrap();
    // 经 load_from_db 建立设置缓存并写入种子行（与生产启动路径一致）。
    let settings = llm_gateway::app_settings::AppSettings::load_from_db(&db)
        .await
        .unwrap();
    let app = common::build_authed_app_with_settings(db.clone(), scheduler, log_tx, settings).await;
    let pid = seed_provider(&db, "p-threshold", &base).await;
    seed_vm_with_member(&db, pid, "vm-th").await;

    // 非法值被拒绝：0 与负数。
    assert_eq!(send_setting(&app, "0").await, StatusCode::BAD_REQUEST);
    assert_eq!(send_setting(&app, "-1").await, StatusCode::BAD_REQUEST);
    assert_eq!(send_setting(&app, "abc").await, StatusCode::BAD_REQUEST);

    // 阈值改为 2，热生效：两次连续失败即熔断。
    assert_eq!(send_setting(&app, "2").await, StatusCode::OK);
    for _ in 0..2 {
        send_chat(&app, "vm-th").await;
    }
    let row = provider_row(&db, pid).await;
    assert!(!row.enable, "阈值改为 2 后两次失败应熔断");
    assert!(row.failure_disabled);
}

// ---------- 失败复查 ----------

/// 可变负载的用量 mock（DeepSeek 余额形态），返回可随时改写的余额响应体。
async fn spawn_usage_mock() -> (String, Arc<Mutex<String>>) {
    let payload = Arc::new(Mutex::new(String::new()));
    let payload_clone = payload.clone();
    let app = Router::new().fallback(move || {
        let payload = payload_clone.clone();
        async move {
            let body = payload.lock().unwrap().clone();
            (StatusCode::OK, axum::response::Html(body))
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), payload)
}

/// 用量开启的供应商：base_url host 命中火山费用中心（用量从 extra 读 ak/sk，
/// 不依赖 api_key），api_key 为损坏密文 → 转发在解密阶段失败（不触网 502）；
/// 用量请求经 LLM_GATEWAY_USAGE_HTTP_OVERRIDE 重定向到本地 mock。
async fn seed_usage_provider(
    db: &sea_orm::DatabaseConnection,
    name: &str,
    display_id: &str,
) -> (i32, i32) {
    let pid = provider::ActiveModel {
        name: Set(name.to_string()),
        enable: Set(true),
        base_url: Set("https://ark.cn-beijing.volces.com/v1".to_string()),
        api_key: Set("enc:v1:AA==".to_string()),
        custom_header: Set("{}".to_string()),
        status: Set(0),
        protocol_type: Set(0),
        billing_mode: Set(0),
        extra: Set(
            r#"{"usage": true, "usage_type": 0, "ak": "test-ak", "sk": "test-sk"}"#.to_string(),
        ),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap()
    .id;
    let model_id = seed_vm_with_member(db, pid, display_id).await;
    (pid, model_id)
}

/// 轮询等待供应商被停用（后台复查任务完成）。
async fn wait_provider_disabled(db: &sea_orm::DatabaseConnection, pid: i32) {
    for _ in 0..60 {
        let row = provider::Entity::find_by_id(pid)
            .one(db)
            .await
            .unwrap()
            .unwrap();
        if !row.enable {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("等待供应商停用超时");
}

/// 轮询等待用量缓存落库（复查已执行）。
async fn wait_usage_cache(db: &sea_orm::DatabaseConnection, pid: i32) {
    for _ in 0..60 {
        let cache = usage_cache::Entity::find()
            .filter(usage_cache::Column::ProviderId.eq(pid))
            .one(db)
            .await
            .unwrap();
        if cache.is_some() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("等待用量缓存落库超时");
}

fn exhausted_balance() -> String {
    serde_json::json!({
        "Result": { "AvailableBalance": "0.00", "CashBalance": "0.00" }
    })
    .to_string()
}

fn sufficient_balance() -> String {
    serde_json::json!({
        "Result": { "AvailableBalance": "110.00", "CashBalance": "110.00" }
    })
    .to_string()
}

#[tokio::test]
async fn recheck_exhausted_disables_and_auto_restores() {
    let (mock_base, payload) = spawn_usage_mock().await;
    *payload.lock().unwrap() = exhausted_balance();

    temp_env::async_with_vars([(USAGE_OVERRIDE_ENV, Some(mock_base.as_str()))], async {
        let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
        scheduler.start().await.unwrap();
        let app = common::build_authed_app(db.clone(), scheduler, log_tx).await;
        let (pid, model_id) = seed_usage_provider(&db, "p-recheck", "vm-recheck").await;

        // 一次转发失败（解密失败）→ 后台复查发现余额耗尽 → 额度门控禁用。
        let status = send_chat(&app, "vm-recheck").await;
        assert_eq!(status, 502, "解密失败应返回 502");
        wait_provider_disabled(&db, pid).await;

        let row = provider_row(&db, pid).await;
        assert!(!row.enable, "余额耗尽应禁用供应商");
        assert!(
            !row.failure_disabled,
            "额度门控禁用不打 failure_disabled 标记（可自动恢复）"
        );
        let item = virtual_model_item::Entity::find()
            .filter(virtual_model_item::Column::ModelId.eq(model_id))
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert!(!item.enable, "子模型应被级联停用");
        let cache = usage_cache::Entity::find()
            .filter(usage_cache::Column::ProviderId.eq(pid))
            .one(&db)
            .await
            .unwrap();
        assert!(cache.is_some(), "复查结果应写入用量数据库缓存");

        // 余额恢复 + usage_refresh → 自动重新启用（与 failure_disabled 路径区分）。
        *payload.lock().unwrap() = sufficient_balance();
        llm_gateway::usage::persist::refresh_all_usage(&db)
            .await
            .unwrap();
        let row = provider_row(&db, pid).await;
        assert!(row.enable, "额度恢复后应自动启用");
        assert!(!row.failure_disabled);
    })
    .await;
}

#[tokio::test]
async fn recheck_sufficient_keeps_provider_enabled() {
    let (mock_base, payload) = spawn_usage_mock().await;
    *payload.lock().unwrap() = sufficient_balance();

    temp_env::async_with_vars([(USAGE_OVERRIDE_ENV, Some(mock_base.as_str()))], async {
        let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
        scheduler.start().await.unwrap();
        let app = common::build_authed_app(db.clone(), scheduler, log_tx).await;
        let (pid, _model_id) = seed_usage_provider(&db, "p-sufficient", "vm-sufficient").await;

        let status = send_chat(&app, "vm-sufficient").await;
        assert_eq!(status, 502);

        // 等复查落库完成（写缓存即复查已执行），供应商保持启用。
        wait_usage_cache(&db, pid).await;
        let row = provider_row(&db, pid).await;
        assert!(row.enable, "余额充足不应禁用");
        assert!(!row.failure_disabled);
    })
    .await;
}

#[tokio::test]
async fn non_retryable_4xx_counts_toward_threshold() {
    let base = spawn_bad_request_mock().await;
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    scheduler.start().await.unwrap();
    let app = common::build_authed_app(db.clone(), scheduler, log_tx).await;
    let pid = seed_provider(&db, "p-4xx", &base).await;
    seed_vm_with_member(&db, pid, "vm-4xx").await;

    // 400 不进入降级循环（直接返回），但同样计入连续失败。
    for _ in 0..4 {
        let status = send_chat(&app, "vm-4xx").await;
        assert_eq!(status, 400);
    }
    let row = provider_row(&db, pid).await;
    assert!(row.enable, "未达阈值不应停用");

    let status = send_chat(&app, "vm-4xx").await;
    assert_eq!(status, 400);
    let row = provider_row(&db, pid).await;
    assert!(!row.enable, "4xx 也应累计到连续失败并触发熔断");
    assert!(row.failure_disabled);
}

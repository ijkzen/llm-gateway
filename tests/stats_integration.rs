mod common;

use axum::body::Body;
use axum::http::Request;
use sea_orm::{ActiveModelTrait, DatabaseConnection, Set};
use tower::ServiceExt;

use llm_gateway::entity::provider as provider_entity;
use llm_gateway::entity::request as request_entity;

#[path = "stats_integration/api_key_filter.rs"]
mod api_key_filter;
#[path = "stats_integration/api_key_rank.rs"]
mod api_key_rank;
#[path = "stats_integration/granularity.rs"]
mod granularity;
#[path = "stats_integration/insight.rs"]
mod insight;
#[path = "stats_integration/summary_charts.rs"]
mod summary_charts;

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
        protocol_type: Set(0),
        billing_mode: Set(0),
        extra: Set("{}".to_string()),
        sort_order: Set(0),
        created_at: Set(now),
        updated_at: Set(now),
        proxy_enabled: Set(false),
        proxy_addr: Set(String::new()),
        disabled_reason: Set(None),
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

// ---------- 显式 granularity 分桶（设置表时区口径） ----------

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

// ---------- API Key 数据面板：stats 各端点按 apiKey_name 过滤 ----------

/// seed 第二供应商（id=2），供跨供应商排行区分。
async fn seed_provider2(db: &DatabaseConnection) {
    seed_provider(db, 2, "第二供应商").await;
}

/// seed 一个虚拟模型行（id=2, display_id=vllm-2），供虚拟模型排行区分。
async fn seed_virtual_model(db: &DatabaseConnection) {
    use llm_gateway::entity::virtual_model as vm_entity;
    vm_entity::ActiveModel {
        virtual_model_id: Set(2),
        display_id: Set("vm-2".to_string()),
        enable: Set(true),
        load_balancing_strategy: Set(0),
        fallback_strategy: Set(0),
        interface_type: Set(4),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
    }
    .insert(db)
    .await
    .unwrap();
}

/// 两个 key 各若干请求（跨供应商/虚拟模型/模型），窗口固定 3 小时。
/// key-a：provider1+vm1+gpt-4o（1 条成功）；key-b：provider1+vm1+claude + provider2+vm2+deepseek（2 条成功）。
async fn seed_two_keys(db: &DatabaseConnection) {
    seed_provider2(db).await;
    seed_virtual_model(db).await;
    let t0 = (1_700_000_000_000i64 / HOUR_MS) * HOUR_MS;
    for row in [
        ApiKeyRow {
            request_id: "akf-1".into(),
            virtual_model_id: 1,
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            api_key_name: "key-a".into(),
            success: true,
            start_time: t0 + 1,
            input_tokens: Some(100),
            input_cache_tokens: 40,
            total_tokens: Some(200),
        },
        // key-b：provider1 的 claude。
        ApiKeyRow {
            request_id: "akf-2".into(),
            virtual_model_id: 1,
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "claude-sonnet".into(),
            api_key_name: "key-b".into(),
            success: true,
            start_time: t0 + 1,
            input_tokens: Some(100),
            input_cache_tokens: 20,
            total_tokens: Some(150),
        },
        // key-b：provider2 + vm2 的 deepseek。
        ApiKeyRow {
            request_id: "akf-3".into(),
            virtual_model_id: 2,
            provider_id: 2,
            model_id: "deepseek-v3".into(),
            api_key_name: "key-b".into(),
            success: true,
            start_time: t0 + 2,
            input_tokens: Some(50),
            input_cache_tokens: 0,
            total_tokens: Some(80),
        },
        // key-a 的失败行（指标/排行不计入，但 charts 计入调用）。
        ApiKeyRow {
            request_id: "akf-4".into(),
            virtual_model_id: 1,
            provider_id: DEFAULT_PROVIDER_ID,
            model_id: "gpt-4o".into(),
            api_key_name: "key-a".into(),
            success: false,
            start_time: t0 + 2,
            input_tokens: Some(999),
            input_cache_tokens: 0,
            total_tokens: Some(999),
        },
    ] {
        insert_ak_row(db, row).await;
    }
}

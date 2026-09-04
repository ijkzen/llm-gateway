//! 连续失败禁用供应商自动恢复任务的业务集成测试。

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use axum::{Json, Router, routing::post};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, Set};
use serde_json::json;

use llm_gateway::cron::JobContext;
use llm_gateway::cron::repository::SeaOrmCronJobRepository;
use llm_gateway::entity::{provider, provider_model, request, virtual_model, virtual_model_item};
use llm_gateway::state::AppState;

async fn spawn_success_upstream() -> String {
    let app = Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            Json(json!({
                "choices": [{"message": {"role": "assistant", "content": "ok"}}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
            }))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}/v1")
}

async fn spawn_probe_proxy() -> (String, Arc<AtomicUsize>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let requests = Arc::new(AtomicUsize::new(0));
    let handler_requests = requests.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut client, _)) = listener.accept().await else {
                break;
            };
            let requests = handler_requests.clone();
            tokio::spawn(async move {
                let mut buf = [0_u8; 8192];
                let Ok(_) = client.read(&mut buf).await else {
                    return;
                };
                if client
                    .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                    .await
                    .is_err()
                {
                    return;
                }
                let Ok(_) = client.read(&mut buf).await else {
                    return;
                };
                requests.fetch_add(1, Ordering::SeqCst);
                let body = r#"{"choices":[{"message":{"role":"assistant","content":"ok"}}]}"#;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = client.write_all(response.as_bytes()).await;
            });
        }
    });
    (format!("http://{addr}"), requests)
}

async fn spawn_usage_mock() -> (String, Arc<Mutex<String>>) {
    let payload = Arc::new(Mutex::new(String::new()));
    let handler_payload = payload.clone();
    let app = Router::new().fallback(move || {
        let payload = handler_payload.clone();
        async move { payload.lock().unwrap().clone() }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{addr}"), payload)
}

fn balance_payload(amount: f64) -> String {
    json!({
        "balance_infos": [{
            "currency": "CNY",
            "total_balance": amount.to_string(),
            "topped_up_balance": "0",
            "granted_balance": "0"
        }]
    })
    .to_string()
}

async fn spawn_redirect_upstream() -> String {
    let app = Router::new().route(
        "/v1/chat/completions",
        post(|| async { (axum::http::StatusCode::FOUND, [("location", "/login")], "") }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}/v1")
}

async fn spawn_delayed_success_upstream() -> (
    String,
    std::sync::Arc<tokio::sync::Notify>,
    std::sync::Arc<tokio::sync::Notify>,
) {
    let received = std::sync::Arc::new(tokio::sync::Notify::new());
    let release = std::sync::Arc::new(tokio::sync::Notify::new());
    let handler_received = received.clone();
    let handler_release = release.clone();
    let app = Router::new().route(
        "/v1/chat/completions",
        post(move || {
            let received = handler_received.clone();
            let release = handler_release.clone();
            async move {
                received.notify_one();
                release.notified().await;
                Json(json!({
                    "choices": [{"message": {"role": "assistant", "content": "ok"}}]
                }))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{addr}/v1"), received, release)
}

async fn test_state() -> AppState {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    AppState {
        db,
        scheduler,
        log_tx,
        lb_state: llm_gateway::proxy::LbState::default(),
        failure_counter: llm_gateway::proxy::failure_counter::FailureCounter::default(),
        recheck_gate: llm_gateway::proxy::failure_recheck::RecheckGate::default(),
        upstream_pool: llm_gateway::proxy::pool::UpstreamPool::new(std::time::Duration::from_secs(
            600,
        )),
        settings: llm_gateway::app_settings::AppSettings::default(),
    }
}

async fn seed_failure_disabled_provider(state: &AppState, base_url: &str) -> (i32, i32) {
    let now = chrono::Utc::now();
    let provider = provider::ActiveModel {
        name: Set(format!("待恢复供应商-{base_url}")),
        enable: Set(false),
        base_url: Set(base_url.to_string()),
        api_key: Set(llm_gateway::crypto::encrypt("sk-test")),
        custom_header: Set("{}".to_string()),
        protocol_type: Set(0),
        billing_mode: Set(0),
        extra: Set("{}".to_string()),
        failure_disabled: Set(true),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .unwrap();
    let model = provider_model::ActiveModel {
        provider_id: Set(provider.id),
        provider_model_id: Set("recovery-model".to_string()),
        context_length: Set(128_000),
        max_output_tokens: Set(1024),
        reasoning: Set(false),
        tool_use: Set(false),
        image_understand: Set(false),
        video_understand: Set(false),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .unwrap();
    let virtual_model = virtual_model::ActiveModel {
        display_id: Set(format!("recovery-vm-{}", provider.id)),
        enable: Set(true),
        load_balancing_strategy: Set(0),
        fallback_strategy: Set(1),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .unwrap();
    virtual_model_item::ActiveModel {
        virtual_model_id: Set(virtual_model.virtual_model_id),
        model_id: Set(model.model_id),
        enable: Set(false),
        cascade_disabled: Set(true),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .unwrap();
    (provider.id, model.model_id)
}

async fn wait_for_requests(state: &AppState, expected: u64) -> Vec<request::Model> {
    for _ in 0..20 {
        if request::Entity::find().count(&state.db).await.unwrap() >= expected {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    request::Entity::find().all(&state.db).await.unwrap()
}

async fn set_usage_enabled(state: &AppState, provider_id: i32, proxy_addr: &str) {
    let provider = provider::Entity::find_by_id(provider_id)
        .one(&state.db)
        .await
        .unwrap()
        .unwrap();
    let mut active: provider::ActiveModel = provider.into();
    active.base_url = Set("http://api.deepseek.com/v1".to_string());
    active.extra = Set(r#"{"usage":true,"usage_type":0}"#.to_string());
    active.update(&state.db).await.unwrap();
    let model = provider_model::Entity::find()
        .filter(provider_model::Column::ProviderId.eq(provider_id))
        .one(&state.db)
        .await
        .unwrap()
        .unwrap();
    let mut active: provider_model::ActiveModel = model.into();
    active.proxy_enabled = Set(true);
    active.proxy_addr = Set(proxy_addr.to_string());
    active.update(&state.db).await.unwrap();
}

#[tokio::test]
async fn hourly_recovery_job_seed_is_idempotent_and_loadable() {
    let (db, scheduler, _log_tx) = common::setup_db_and_scheduler().await;
    scheduler
        .register_handler(
            llm_gateway::cron::seed::FAILURE_RECOVERY_JOB,
            Arc::new(|_ctx: JobContext| Box::pin(async { Ok(()) })),
        )
        .await;
    llm_gateway::cron::seed::ensure_failure_recovery_job(&db)
        .await
        .unwrap();
    llm_gateway::cron::seed::ensure_failure_recovery_job(&db)
        .await
        .unwrap();

    scheduler
        .load_from_db(&SeaOrmCronJobRepository::new(db))
        .await
        .unwrap();
    scheduler.start().await.unwrap();
    assert!(
        scheduler
            .has_job(llm_gateway::cron::seed::FAILURE_RECOVERY_JOB)
            .await
    );
    let jobs = scheduler.list_jobs().await;
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].expression, "@hourly");
    assert_eq!(jobs[0].group, "system");
}

#[tokio::test]
async fn usage_must_be_available_before_probe() {
    let (usage_url, usage_payload) = spawn_usage_mock().await;
    temp_env::async_with_vars(
        [("LLM_GATEWAY_USAGE_HTTP_OVERRIDE", Some(usage_url.as_str()))],
        async {
            let state = test_state().await;
            let (probe_proxy, probe_requests) = spawn_probe_proxy().await;
            let (provider_id, _) =
                seed_failure_disabled_provider(&state, "http://api.deepseek.com/v1").await;
            set_usage_enabled(&state, provider_id, &probe_proxy).await;

            *usage_payload.lock().unwrap() = "not-json".to_string();
            assert_eq!(
                llm_gateway::proxy::failure_recovery::recover_failure_disabled(&state)
                    .await
                    .unwrap(),
                0
            );
            assert_eq!(probe_requests.load(Ordering::SeqCst), 0);

            *usage_payload.lock().unwrap() = balance_payload(0.0);
            assert_eq!(
                llm_gateway::proxy::failure_recovery::recover_failure_disabled(&state)
                    .await
                    .unwrap(),
                0
            );
            assert_eq!(probe_requests.load(Ordering::SeqCst), 0);

            *usage_payload.lock().unwrap() = balance_payload(100.0);
            assert_eq!(
                llm_gateway::proxy::failure_recovery::recover_failure_disabled(&state)
                    .await
                    .unwrap(),
                1
            );
            assert_eq!(probe_requests.load(Ordering::SeqCst), 1);
        },
    )
    .await;
}

#[tokio::test]
async fn provider_without_model_stays_disabled_without_request() {
    let state = test_state().await;
    let base_url = spawn_success_upstream().await;
    let (provider_id, model_id) = seed_failure_disabled_provider(&state, &base_url).await;
    provider_model::Entity::delete_by_id(model_id)
        .exec(&state.db)
        .await
        .unwrap();

    assert_eq!(
        llm_gateway::proxy::failure_recovery::recover_failure_disabled(&state)
            .await
            .unwrap(),
        0
    );
    let provider = provider::Entity::find_by_id(provider_id)
        .one(&state.db)
        .await
        .unwrap()
        .unwrap();
    assert!(!provider.enable);
    assert!(provider.failure_disabled);
    assert_eq!(request::Entity::find().count(&state.db).await.unwrap(), 0);
}

#[tokio::test]
async fn failed_provider_does_not_stop_later_recovery() {
    let state = test_state().await;
    let redirect_url = spawn_redirect_upstream().await;
    let (failed_id, _) = seed_failure_disabled_provider(&state, &redirect_url).await;
    let success_url = spawn_success_upstream().await;
    let (recovered_id, _) = seed_failure_disabled_provider(&state, &success_url).await;

    assert_eq!(
        llm_gateway::proxy::failure_recovery::recover_failure_disabled(&state)
            .await
            .unwrap(),
        1
    );
    let failed = provider::Entity::find_by_id(failed_id)
        .one(&state.db)
        .await
        .unwrap()
        .unwrap();
    let recovered = provider::Entity::find_by_id(recovered_id)
        .one(&state.db)
        .await
        .unwrap()
        .unwrap();
    assert!(failed.failure_disabled);
    assert!(!failed.enable);
    assert!(!recovered.failure_disabled);
    assert!(recovered.enable);
}

#[tokio::test]
async fn redirect_probe_keeps_provider_disabled_and_records_failure() {
    let state = test_state().await;
    let base_url = spawn_redirect_upstream().await;
    let (provider_id, _) = seed_failure_disabled_provider(&state, &base_url).await;

    let recovered = llm_gateway::proxy::failure_recovery::recover_failure_disabled(&state)
        .await
        .unwrap();

    assert_eq!(recovered, 0);
    let provider = provider::Entity::find_by_id(provider_id)
        .one(&state.db)
        .await
        .unwrap()
        .unwrap();
    assert!(!provider.enable);
    assert!(provider.failure_disabled);
    let rows = wait_for_requests(&state, 1).await;
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].success);
}

#[tokio::test]
async fn stale_probe_does_not_overwrite_provider_changed_during_request() {
    let state = test_state().await;
    let (base_url, received, release) = spawn_delayed_success_upstream().await;
    let (provider_id, _) = seed_failure_disabled_provider(&state, &base_url).await;
    let task_state = state.clone();
    let recovery = tokio::spawn(async move {
        llm_gateway::proxy::failure_recovery::recover_failure_disabled(&task_state).await
    });

    received.notified().await;
    let provider = provider::Entity::find_by_id(provider_id)
        .one(&state.db)
        .await
        .unwrap()
        .unwrap();
    let mut active: provider::ActiveModel = provider.into();
    active.updated_at = Set(chrono::Utc::now() + chrono::Duration::seconds(1));
    active.update(&state.db).await.unwrap();
    release.notify_one();

    assert_eq!(recovery.await.unwrap().unwrap(), 0);
    let provider = provider::Entity::find_by_id(provider_id)
        .one(&state.db)
        .await
        .unwrap()
        .unwrap();
    assert!(!provider.enable);
    assert!(provider.failure_disabled);
}

#[tokio::test]
async fn successful_probe_recovers_provider_and_cascade_disabled_item() {
    let state = test_state().await;
    let base_url = spawn_success_upstream().await;
    let (provider_id, model_id) = seed_failure_disabled_provider(&state, &base_url).await;
    let now = chrono::Utc::now();
    let second_model = provider_model::ActiveModel {
        provider_id: Set(provider_id),
        provider_model_id: Set("second-model".to_string()),
        context_length: Set(128_000),
        max_output_tokens: Set(1024),
        reasoning: Set(false),
        tool_use: Set(false),
        image_understand: Set(false),
        video_understand: Set(false),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .unwrap();
    let first_item = virtual_model_item::Entity::find()
        .filter(virtual_model_item::Column::ModelId.eq(model_id))
        .one(&state.db)
        .await
        .unwrap()
        .unwrap();
    virtual_model_item::ActiveModel {
        virtual_model_id: Set(first_item.virtual_model_id),
        model_id: Set(second_model.model_id),
        enable: Set(false),
        cascade_disabled: Set(false),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(&state.db)
    .await
    .unwrap();
    state.failure_counter.record_failure(provider_id);

    let recovered = llm_gateway::proxy::failure_recovery::recover_failure_disabled(&state)
        .await
        .unwrap();

    assert_eq!(recovered, 1);
    let provider = provider::Entity::find_by_id(provider_id)
        .one(&state.db)
        .await
        .unwrap()
        .unwrap();
    assert!(provider.enable);
    assert!(!provider.failure_disabled);
    let item = virtual_model_item::Entity::find()
        .filter(virtual_model_item::Column::ModelId.eq(model_id))
        .one(&state.db)
        .await
        .unwrap()
        .unwrap();
    assert!(item.enable);
    assert!(!item.cascade_disabled);

    let rows = wait_for_requests(&state, 1).await;
    assert_eq!(rows.len(), 1);
    assert!(rows[0].success);
    assert_eq!(rows[0].provider_id, provider_id);
    assert_eq!(
        rows[0].model_id, "recovery-model",
        "应稳定探测主键最小的模型"
    );
    let manual_item = virtual_model_item::Entity::find()
        .filter(virtual_model_item::Column::ModelId.eq(second_model.model_id))
        .one(&state.db)
        .await
        .unwrap()
        .unwrap();
    assert!(!manual_item.enable, "手动禁用条目不应被自动恢复");
    assert!(!manual_item.cascade_disabled);
    assert_eq!(
        state.failure_counter.record_failure(provider_id),
        1,
        "恢复后连续失败计数应从 1 重新开始"
    );
}

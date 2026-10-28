//! 订阅制边界探活集成测试：用量缓存显示任一窗口剩余百分比落在 (0, 1) 时，
//! probe_boundary_providers 向该供应商发最小测试请求——成功保持/恢复可用，
//! 失败按订阅额度耗尽停用（quota 标记 + 级联停用虚拟模型子模型）；禁用后每轮
//! 继续探活直到成功（恢复双通道之一）；manual 停用与余量充足的供应商不探活。

mod common;

use std::sync::{Arc, Mutex};

use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde_json::json;

use llm_gateway::entity::{provider, provider_model, virtual_model, virtual_model_item};
use llm_gateway::state::AppState;
use llm_gateway::usage::persist::{apply_usage_gate, probe_boundary_providers, write_usage_cache};
use llm_gateway::usage::types::{QuotaWindow, UsageData, UsageKind, WindowKind};

/// 订阅制边界供应商 + 名下模型 + 虚拟模型条目（直连本地 mock，无代理）。
async fn seed_boundary_provider(db: &sea_orm::DatabaseConnection, base_url: &str) -> (i32, i32) {
    let now = chrono::Utc::now();
    let p = provider::ActiveModel {
        name: Set("边界订阅供应商".to_string()),
        enable: Set(true),
        base_url: Set(base_url.to_string()),
        api_key: Set(llm_gateway::crypto::encrypt("sk-x")),
        custom_header: Set("{}".to_string()),
        protocol_type: Set(0),
        billing_mode: Set(1),
        extra: Set(r#"{"usage": true, "usage_type": 1}"#.to_string()),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap();

    let m = provider_model::ActiveModel {
        provider_id: Set(p.id),
        provider_model_id: Set("glm-4.5".to_string()),
        context_length: Set(128000),
        max_output_tokens: Set(8192),
        reasoning: Set(true),
        tool_use: Set(true),
        image_understand: Set(false),
        video_understand: Set(false),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap();

    let vm = virtual_model::ActiveModel {
        display_id: Set("vm-boundary".to_string()),
        enable: Set(true),
        load_balancing_strategy: Set(0),
        fallback_strategy: Set(1),
        interface_type: Set(4),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap();

    virtual_model_item::ActiveModel {
        virtual_model_id: Set(vm.virtual_model_id),
        model_id: Set(m.model_id),
        enable: Set(true),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap();

    (p.id, m.model_id)
}

/// 订阅制窗口数据：5h/周/月三个槽位各给剩余百分比。
fn quota_data(provider_id: i32, five_hour: f64, weekly: f64, monthly: f64) -> UsageData {
    UsageData {
        provider_id,
        fetched_at: chrono::Utc::now(),
        kind: UsageKind::Quota,
        plan: Some("pro".to_string()),
        windows: vec![
            QuotaWindow::from_remaining_percent(WindowKind::FiveHour, five_hour, None),
            QuotaWindow::from_remaining_percent(WindowKind::Weekly, weekly, None),
            QuotaWindow::from_remaining_percent(WindowKind::Monthly, monthly, None),
        ],
        balances: vec![],
    }
}

/// 边界探活 mock 上游：状态码与请求数可变（先 402 后切 200 验证恢复通道）。
/// 初始 200；`(status, count)`，改写 status 即可切换成功/失败。
async fn spawn_probe_mock() -> (String, Arc<Mutex<(u16, usize)>>) {
    let shared = Arc::new(Mutex::new((200u16, 0usize)));
    let app = Router::new().route(
        "/v1/chat/completions",
        post({
            let shared = shared.clone();
            move || async move {
                let status = {
                    let mut guard = shared.lock().unwrap();
                    guard.1 += 1;
                    guard.0
                };
                if status >= 400 {
                    (
                        StatusCode::from_u16(status).unwrap(),
                        Json(json!({"error": {"message": "insufficient quota"}})),
                    )
                } else {
                    (
                        StatusCode::OK,
                        Json(json!({
                            "id": "chatcmpl-probe",
                            "object": "chat.completion",
                            "model": "glm-4.5",
                            "choices": [{"index": 0, "message": {"role": "assistant", "content": "hi"}, "finish_reason": "stop"}],
                            "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
                        })),
                    )
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), shared)
}

fn test_state(
    db: sea_orm::DatabaseConnection,
    scheduler: llm_gateway::cron::scheduler::SchedulerRuntime,
    log_tx: tokio::sync::broadcast::Sender<
        std::sync::Arc<llm_gateway::cron::log_capture::JobLogEvent>,
    >,
) -> AppState {
    AppState {
        db,
        scheduler,
        log_tx,
        lb_state: Default::default(),
        failure_counter: Default::default(),
        recheck_gate: Default::default(),
        upstream_pool: llm_gateway::proxy::pool::UpstreamPool::new(std::time::Duration::from_secs(
            600,
        )),
        settings: Default::default(),
        usage_mem: Default::default(),
        feishu_registration: Default::default(),
        speed_test: Default::default(),
    }
}

async fn provider_enabled(db: &sea_orm::DatabaseConnection, id: i32) -> bool {
    provider::Entity::find_by_id(id)
        .one(db)
        .await
        .unwrap()
        .unwrap()
        .enable
}

async fn disabled_reason(db: &sea_orm::DatabaseConnection, id: i32) -> Option<String> {
    provider::Entity::find_by_id(id)
        .one(db)
        .await
        .unwrap()
        .unwrap()
        .disabled_reason
}

async fn item_row(db: &sea_orm::DatabaseConnection, model_id: i32) -> virtual_model_item::Model {
    virtual_model_item::Entity::find()
        .filter(virtual_model_item::Column::ModelId.eq(model_id))
        .one(db)
        .await
        .unwrap()
        .unwrap()
}

/// 边界窗口 + 探活成功（2xx）→ 保持启用，探活请求确实发出。
#[tokio::test]
async fn boundary_probe_success_keeps_provider_enabled() {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    let (url, shared) = spawn_probe_mock().await;
    let (pid, model_id) = seed_boundary_provider(&db, &url).await;
    write_usage_cache(&db, &quota_data(pid, 0.5, 50.0, 100.0))
        .await
        .unwrap();

    let n = probe_boundary_providers(&test_state(db.clone(), scheduler, log_tx))
        .await
        .unwrap();
    assert_eq!(n, 1, "边界供应商应被探活");
    assert_eq!(shared.lock().unwrap().1, 1);
    assert!(provider_enabled(&db, pid).await);
    assert_eq!(disabled_reason(&db, pid).await, None);
    assert!(item_row(&db, model_id).await.enable);
}

/// 边界窗口 + 探活失败（402）→ 按订阅额度耗尽停用（quota 标记 + 级联停用子模型）。
#[tokio::test]
async fn boundary_probe_failure_disables_provider() {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    let (url, shared) = spawn_probe_mock().await;
    shared.lock().unwrap().0 = 402;
    let (pid, model_id) = seed_boundary_provider(&db, &url).await;
    write_usage_cache(&db, &quota_data(pid, 0.5, 50.0, 100.0))
        .await
        .unwrap();

    let n = probe_boundary_providers(&test_state(db.clone(), scheduler, log_tx))
        .await
        .unwrap();
    assert_eq!(n, 1);
    assert_eq!(shared.lock().unwrap().1, 1);
    assert!(!provider_enabled(&db, pid).await);
    assert_eq!(disabled_reason(&db, pid).await.as_deref(), Some("quota"));
    let item = item_row(&db, model_id).await;
    assert!(!item.enable);
    assert!(item.cascade_disabled, "被级联停用的成员应带标记");
}

/// 回归：边界区（剩余 (0,1)）的供应商探活失败停用后，**后续每轮用量门控都不得
/// 凭同一份边界区数据把它恢复回来**——否则每轮刷新「先恢复、1~2 秒后被探活停用」
/// 成对抖动。
///
/// 生产症状（2026-09-18 实测，供应商「Command Code Google」）：月度窗口剩 0.12%
/// 落在边界区，于是每轮 usage_refresh（5 分钟）都发出成对的「自动启用 + 自动停用」
/// 通知；更糟的是那 1~2 秒窗口里它重新进入选路候选，真实请求会打到已耗尽的账号上
/// （上游回 400 insufficient credits，实测 2 分钟内有 10 条真实请求撞死）。
///
/// 根因：恢复通道 A（数据）判据是 `subscription_usable() == Some(true)`，而它只
/// 要求「无窗口剩余 ≤ 0」——边界区 (0,1) 的 0.12% 同样满足。于是探活通道刚用真实
/// 请求判定「不可用」，同一份数据立刻判回「可用」，通道 B（探活成功）根本没机会
/// 参与。两条通道对同一份数据给出相反结论，且每轮互相推翻。
///
/// 本用例按**真实任务顺序**（门控 → 探活，见 usage_refresh handler）跑满两轮。
#[tokio::test]
async fn boundary_band_provider_is_not_recovered_by_data_channel() {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    let (url, shared) = spawn_probe_mock().await;
    shared.lock().unwrap().0 = 400; // 上游额度不足（同生产）
    let (pid, model_id) = seed_boundary_provider(&db, &url).await;
    // 月度剩 0.12%：边界区，但 subscription_usable() 仍为 true。
    let band = || quota_data(pid, 100.0, 80.91, 0.12);
    write_usage_cache(&db, &band()).await.unwrap();

    let reload = async || {
        provider::Entity::find_by_id(pid)
            .one(&db)
            .await
            .unwrap()
            .unwrap()
    };

    // 第 1 轮：门控（已启用 → 幂等空转）→ 探活失败 → 按额度耗尽停用。
    apply_usage_gate(&db, &reload().await, &band())
        .await
        .unwrap();
    probe_boundary_providers(&test_state(db.clone(), scheduler.clone(), log_tx.clone()))
        .await
        .unwrap();
    assert!(!provider_enabled(&db, pid).await, "探活失败应停用");
    assert_eq!(disabled_reason(&db, pid).await.as_deref(), Some("quota"));

    // 第 2 轮：同一份边界区数据再走一遍完整流程。门控不得恢复（这是修复前失败
    // 的那一步），探活继续失败保持停用——两轮之间不应出现任何启用窗口。
    apply_usage_gate(&db, &reload().await, &band())
        .await
        .unwrap();
    assert!(
        !provider_enabled(&db, pid).await,
        "边界区数据不得把探活刚判定耗尽的供应商恢复回来——否则每轮刷新成对抖动，\
         且窗口期内已耗尽的供应商会重新进入选路候选"
    );
    assert!(
        !item_row(&db, model_id).await.enable,
        "子模型不应被级联恢复"
    );

    probe_boundary_providers(&test_state(db.clone(), scheduler.clone(), log_tx.clone()))
        .await
        .unwrap();
    assert!(!provider_enabled(&db, pid).await, "探活仍失败应保持停用");
    assert_eq!(disabled_reason(&db, pid).await.as_deref(), Some("quota"));
}

/// 恢复闭环：quota 停用后每轮继续探活——仍失败保持禁用不抖动，成功后自动恢复。
#[tokio::test]
async fn quota_disabled_provider_probes_until_success_then_recovers() {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    let (url, shared) = spawn_probe_mock().await;
    shared.lock().unwrap().0 = 402;
    let (pid, model_id) = seed_boundary_provider(&db, &url).await;
    write_usage_cache(&db, &quota_data(pid, 0.5, 50.0, 100.0))
        .await
        .unwrap();

    // 第一轮：失败 → 停用。
    probe_boundary_providers(&test_state(db.clone(), scheduler.clone(), log_tx.clone()))
        .await
        .unwrap();
    assert!(!provider_enabled(&db, pid).await);
    assert_eq!(disabled_reason(&db, pid).await.as_deref(), Some("quota"));

    // 第二轮：窗口仍边界、上游仍失败 → 保持停用（不抖动），探活继续。
    probe_boundary_providers(&test_state(db.clone(), scheduler.clone(), log_tx.clone()))
        .await
        .unwrap();
    assert!(!provider_enabled(&db, pid).await);
    assert_eq!(disabled_reason(&db, pid).await.as_deref(), Some("quota"));
    assert!(!item_row(&db, model_id).await.enable);

    // 第三轮：上游恢复 2xx → 探活成功解除停用，子模型级联恢复。
    shared.lock().unwrap().0 = 200;
    probe_boundary_providers(&test_state(db.clone(), scheduler, log_tx))
        .await
        .unwrap();
    assert!(provider_enabled(&db, pid).await);
    assert_eq!(disabled_reason(&db, pid).await, None);
    let item = item_row(&db, model_id).await;
    assert!(item.enable);
    assert!(!item.cascade_disabled, "恢复后应清除级联停用标记");
    assert_eq!(shared.lock().unwrap().1, 3, "三轮各探活一次");
}

/// 余量充足（窗口均 ≥1%）→ 不探活，状态不变（恢复走 apply_usage_gate 通道）。
#[tokio::test]
async fn healthy_remaining_window_skips_probe() {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    let (url, shared) = spawn_probe_mock().await;
    shared.lock().unwrap().0 = 402;
    let (pid, model_id) = seed_boundary_provider(&db, &url).await;
    write_usage_cache(&db, &quota_data(pid, 50.0, 60.0, 100.0))
        .await
        .unwrap();

    let n = probe_boundary_providers(&test_state(db.clone(), scheduler, log_tx))
        .await
        .unwrap();
    assert_eq!(n, 0, "余量充足不应探活");
    assert_eq!(shared.lock().unwrap().1, 0);
    assert!(provider_enabled(&db, pid).await);
    assert!(item_row(&db, model_id).await.enable);
}

/// 窗口已耗尽（0）不落入边界探活（由 apply_usage_gate 停用），不浪费探活请求。
#[tokio::test]
async fn exhausted_window_skips_probe() {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    let (url, shared) = spawn_probe_mock().await;
    let (pid, model_id) = seed_boundary_provider(&db, &url).await;
    write_usage_cache(&db, &quota_data(pid, 0.0, 50.0, 100.0))
        .await
        .unwrap();

    let n = probe_boundary_providers(&test_state(db.clone(), scheduler, log_tx))
        .await
        .unwrap();
    assert_eq!(n, 0);
    assert_eq!(shared.lock().unwrap().1, 0, "已耗尽不应再发探活请求");
    assert!(provider_enabled(&db, pid).await, "停用由额度门控另行处理");
    assert!(item_row(&db, model_id).await.enable);
}

/// manual 停用态不探活（本机制无权解除 manual），reason 保持 manual。
#[tokio::test]
async fn manual_disabled_provider_not_probed() {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    let (url, shared) = spawn_probe_mock().await;
    let (pid, _model_id) = seed_boundary_provider(&db, &url).await;
    llm_gateway::availability::disable_manual(&db, pid)
        .await
        .unwrap();
    write_usage_cache(&db, &quota_data(pid, 0.5, 50.0, 100.0))
        .await
        .unwrap();

    let n = probe_boundary_providers(&test_state(db.clone(), scheduler, log_tx))
        .await
        .unwrap();
    assert_eq!(n, 0);
    assert_eq!(shared.lock().unwrap().1, 0, "manual 停用态不应探活");
    assert!(!provider_enabled(&db, pid).await);
    assert_eq!(disabled_reason(&db, pid).await.as_deref(), Some("manual"));
}

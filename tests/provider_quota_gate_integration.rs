//! 订阅额度耗尽自动停用/恢复（apply_usage_gate）与内置用量刷新任务种子集成测试。

mod common;

use std::sync::Arc;

use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};

use llm_gateway::cron::JobContext;
use llm_gateway::cron::repository::SeaOrmCronJobRepository;
use llm_gateway::entity::{provider, provider_model, virtual_model, virtual_model_item};
use llm_gateway::usage::persist::apply_usage_gate;
use llm_gateway::usage::types::{QuotaWindow, UsageData, UsageKind, WindowKind};

/// 订阅型供应商 + 名下模型 + 虚拟模型条目，返回 (provider_id, model_id)。
async fn seed_subscription_provider(db: &sea_orm::DatabaseConnection) -> (i32, i32) {
    let now = chrono::Utc::now();
    let p = provider::ActiveModel {
        name: Set("订阅供应商".to_string()),
        enable: Set(true),
        base_url: Set("https://open.bigmodel.cn/v1".to_string()),
        api_key: Set(llm_gateway::crypto::encrypt("sk-x")),
        custom_header: Set("{}".to_string()),
        status: Set(0),
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
        display_id: Set("vm-gate".to_string()),
        enable: Set(true),
        load_balancing_strategy: Set(0),
        fallback_strategy: Set(1),
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

async fn seed_balance_provider(db: &sea_orm::DatabaseConnection) -> (i32, i32) {
    let now = chrono::Utc::now();
    let p = provider::ActiveModel {
        name: Set("按量供应商".to_string()),
        enable: Set(true),
        base_url: Set("https://api.deepseek.com/v1".to_string()),
        api_key: Set(llm_gateway::crypto::encrypt("sk-x")),
        custom_header: Set("{}".to_string()),
        status: Set(0),
        protocol_type: Set(0),
        billing_mode: Set(0),
        extra: Set(r#"{"usage": true, "usage_type": 0}"#.to_string()),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap();

    let m = provider_model::ActiveModel {
        provider_id: Set(p.id),
        provider_model_id: Set("deepseek-chat".to_string()),
        context_length: Set(64000),
        max_output_tokens: Set(8192),
        reasoning: Set(false),
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
        display_id: Set("vm-balance".to_string()),
        enable: Set(true),
        load_balancing_strategy: Set(0),
        fallback_strategy: Set(1),
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

fn balance_data(provider_id: i32, amounts: &[f64]) -> UsageData {
    UsageData {
        provider_id,
        fetched_at: chrono::Utc::now(),
        kind: UsageKind::Balance,
        plan: None,
        windows: vec![],
        balances: amounts
            .iter()
            .map(|a| llm_gateway::usage::types::BalanceItem {
                label: "余额".to_string(),
                amount: *a,
                currency: None,
            })
            .collect(),
    }
}

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

async fn provider_enabled(db: &sea_orm::DatabaseConnection, id: i32) -> bool {
    provider::Entity::find_by_id(id)
        .one(db)
        .await
        .unwrap()
        .unwrap()
        .enable
}

async fn item_enabled(db: &sea_orm::DatabaseConnection, model_id: i32) -> bool {
    virtual_model_item::Entity::find()
        .filter(virtual_model_item::Column::ModelId.eq(model_id))
        .one(db)
        .await
        .unwrap()
        .unwrap()
        .enable
}

#[tokio::test]
async fn quota_exhaustion_disables_and_restore_reenables() {
    let (db, _scheduler, _log_tx) = common::setup_db_and_scheduler().await;
    let (pid, model_id) = seed_subscription_provider(&db).await;

    // 5h 窗口耗尽 → 停用 provider 及其虚拟模型子模型。
    let p = provider::Entity::find_by_id(pid)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    apply_usage_gate(&db, &p, &quota_data(pid, 0.0, 80.0, 100.0))
        .await
        .unwrap();
    assert!(!provider_enabled(&db, pid).await);
    assert!(!item_enabled(&db, model_id).await);

    // 额度恢复（全部窗口有剩余）→ 恢复启用 provider 及其子模型。
    let p = provider::Entity::find_by_id(pid)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    apply_usage_gate(&db, &p, &quota_data(pid, 40.0, 50.0, 100.0))
        .await
        .unwrap();
    assert!(provider_enabled(&db, pid).await);
    assert!(item_enabled(&db, model_id).await);
}

#[tokio::test]
async fn gate_skips_unjudgeable_data() {
    let (db, _scheduler, _log_tx) = common::setup_db_and_scheduler().await;
    let (pid, model_id) = seed_subscription_provider(&db).await;

    // 全部窗口不可用 → 无法判定，保持原状。
    let unknown = UsageData {
        provider_id: pid,
        fetched_at: chrono::Utc::now(),
        kind: UsageKind::Quota,
        plan: None,
        windows: vec![
            QuotaWindow::unavailable(WindowKind::FiveHour),
            QuotaWindow::unavailable(WindowKind::Weekly),
            QuotaWindow::unavailable(WindowKind::Monthly),
        ],
        balances: vec![],
    };
    let p = provider::Entity::find_by_id(pid)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    apply_usage_gate(&db, &p, &unknown).await.unwrap();
    assert!(provider_enabled(&db, pid).await);
    assert!(item_enabled(&db, model_id).await);

    // 余额形态（非 quota）→ 不动。
    let balance = UsageData {
        provider_id: pid,
        fetched_at: chrono::Utc::now(),
        kind: UsageKind::Balance,
        plan: None,
        windows: vec![],
        balances: vec![],
    };
    let p = provider::Entity::find_by_id(pid)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    apply_usage_gate(&db, &p, &balance).await.unwrap();
    assert!(provider_enabled(&db, pid).await);

    // 按量供应商（billing_mode=0）+ 查不到余额（空 balances）→ 不动。
    let (bpid, bmodel_id) = seed_balance_provider(&db).await;
    let p = provider::Entity::find_by_id(bpid)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    apply_usage_gate(&db, &p, &balance_data(bpid, &[]))
        .await
        .unwrap();
    assert!(provider_enabled(&db, bpid).await);
    assert!(item_enabled(&db, bmodel_id).await);
}

#[tokio::test]
async fn balance_exhaustion_disables_and_restore_reenables() {
    let (db, _scheduler, _log_tx) = common::setup_db_and_scheduler().await;
    let (pid, model_id) = seed_balance_provider(&db).await;

    // 余额耗尽（合计 0）→ 停用 provider 及其虚拟模型子模型。
    let p = provider::Entity::find_by_id(pid)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    apply_usage_gate(&db, &p, &balance_data(pid, &[0.0]))
        .await
        .unwrap();
    assert!(!provider_enabled(&db, pid).await);
    assert!(!item_enabled(&db, model_id).await);

    // 余额恢复（>0）→ 恢复启用 provider 及其子模型。
    let p = provider::Entity::find_by_id(pid)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    apply_usage_gate(&db, &p, &balance_data(pid, &[50.0]))
        .await
        .unwrap();
    assert!(provider_enabled(&db, pid).await);
    assert!(item_enabled(&db, model_id).await);
}

#[tokio::test]
async fn balance_unjudgeable_keeps_state() {
    let (db, _scheduler, _log_tx) = common::setup_db_and_scheduler().await;
    let (pid, model_id) = seed_balance_provider(&db).await;

    // 查不到余额（空 balances）→ 无法判定，保持原状。
    let p = provider::Entity::find_by_id(pid)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    apply_usage_gate(&db, &p, &balance_data(pid, &[]))
        .await
        .unwrap();
    assert!(provider_enabled(&db, pid).await);
    assert!(item_enabled(&db, model_id).await);
}

#[tokio::test]
async fn usage_refresh_job_seed_is_scheduled() {
    let (db, scheduler, _log_tx) = common::setup_db_and_scheduler().await;
    scheduler
        .register_handler(
            llm_gateway::cron::seed::USAGE_REFRESH_JOB,
            Arc::new(|_ctx: JobContext| Box::pin(async move { Ok(()) })),
        )
        .await;
    llm_gateway::cron::seed::ensure_usage_refresh_job(&db)
        .await
        .unwrap();

    let repo = SeaOrmCronJobRepository::new(db.clone());
    scheduler.load_from_db(&repo).await.unwrap();
    scheduler.start().await.unwrap();

    assert!(
        scheduler
            .has_job(llm_gateway::cron::seed::USAGE_REFRESH_JOB)
            .await
    );
    let jobs = scheduler.list_jobs().await;
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].expression, "@every 5m");
    assert!(jobs[0].enabled);
}

/// 分层控制：用户手动关闭的成员（虚拟模型编辑器操作，无级联标记）不会被
/// 级联恢复打开；被级联停用的成员带 cascade_disabled 标记，恢复时清除。
#[tokio::test]
async fn manual_disabled_item_survives_gate_reenable() {
    let (db, _scheduler, _log_tx) = common::setup_db_and_scheduler().await;
    let (pid, model_id) = seed_subscription_provider(&db).await;

    // 用户手动关闭成员（模拟编辑器操作，cascade_disabled 保持 false，无标记）。
    let item = virtual_model_item::Entity::find()
        .filter(virtual_model_item::Column::ModelId.eq(model_id))
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    let mut active: virtual_model_item::ActiveModel = item.into();
    active.enable = Set(false);
    active.updated_at = Set(chrono::Utc::now());
    active.update(&db).await.unwrap();

    // 额度耗尽 → 级联停用；手动关闭的成员不被打级联标记。
    let p = provider::Entity::find_by_id(pid)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    apply_usage_gate(&db, &p, &quota_data(pid, 0.0, 80.0, 100.0))
        .await
        .unwrap();
    assert!(!provider_enabled(&db, pid).await);
    let item = virtual_model_item::Entity::find()
        .filter(virtual_model_item::Column::ModelId.eq(model_id))
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert!(!item.enable);
    assert!(!item.cascade_disabled, "手动关闭的成员不应被打级联停用标记");

    // 额度恢复 → 只恢复带级联标记的条目；手动关闭的成员保持关闭。
    let p = provider::Entity::find_by_id(pid)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    apply_usage_gate(&db, &p, &quota_data(pid, 40.0, 50.0, 100.0))
        .await
        .unwrap();
    assert!(provider_enabled(&db, pid).await);
    let item = virtual_model_item::Entity::find()
        .filter(virtual_model_item::Column::ModelId.eq(model_id))
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert!(!item.enable, "手动关闭的成员不应被级联恢复打开");
    assert!(!item.cascade_disabled);
}

/// 被级联停用的启用成员在恢复时重新启用并清除标记。
#[tokio::test]
async fn cascade_disabled_item_reenabled_and_flag_cleared_on_recovery() {
    let (db, _scheduler, _log_tx) = common::setup_db_and_scheduler().await;
    let (pid, model_id) = seed_subscription_provider(&db).await;

    let p = provider::Entity::find_by_id(pid)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    apply_usage_gate(&db, &p, &quota_data(pid, 0.0, 80.0, 100.0))
        .await
        .unwrap();
    let item = virtual_model_item::Entity::find()
        .filter(virtual_model_item::Column::ModelId.eq(model_id))
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert!(!item.enable);
    assert!(item.cascade_disabled, "被级联停用的成员应带标记");

    let p = provider::Entity::find_by_id(pid)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    apply_usage_gate(&db, &p, &quota_data(pid, 40.0, 50.0, 100.0))
        .await
        .unwrap();
    let item = virtual_model_item::Entity::find()
        .filter(virtual_model_item::Column::ModelId.eq(model_id))
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert!(item.enable);
    assert!(!item.cascade_disabled, "恢复后应清除级联停用标记");
}

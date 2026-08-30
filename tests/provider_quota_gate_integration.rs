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

use sea_orm::{ColumnTrait, DbErr, EntityTrait, QueryFilter};

use crate::entity::provider;
use crate::state::AppState;

/// 执行一轮连续失败禁用供应商的自动恢复探测，返回成功恢复数量。
pub async fn recover_failure_disabled(state: &AppState) -> Result<usize, DbErr> {
    let providers = provider::Entity::find()
        .filter(
            provider::Column::DisabledReason
                .eq(crate::availability::DisabledReason::Failure.as_str()),
        )
        .all(&state.db)
        .await?;
    let mut recovered = 0;

    for provider in providers {
        match probe_gate(&state.db, &provider).await {
            ProbeGate::Blocked => continue,
            ProbeGate::UsageUnusable => {
                tracing::warn!(
                    provider_id = provider.id,
                    provider_name = &provider.name,
                    "供应商「{}」用量不可用，跳过自动恢复探测",
                    provider.name
                );
                continue;
            }
            ProbeGate::Allowed => {}
        }
        let provider = match provider::Entity::find_by_id(provider.id)
            .one(&state.db)
            .await
        {
            Ok(Some(provider))
                if provider
                    .disabled_reason
                    .as_deref()
                    .and_then(crate::availability::DisabledReason::parse)
                    == Some(crate::availability::DisabledReason::Failure) =>
            {
                provider
            }
            Ok(_) => continue,
            Err(error) => {
                tracing::warn!(
                    provider_id = provider.id,
                    provider_name = &provider.name,
                    "供应商「{}」自动恢复重新读取失败：{error}",
                    provider.name
                );
                continue;
            }
        };
        // 探活前奏（与 probe_provider 同一实现）：失败原因逐阶段 warn 点名供应商。
        let (model, api_key) = match crate::proxy::probe_preamble(state, &provider).await {
            Ok(preamble) => preamble,
            Err(reason) => {
                tracing::warn!(
                    provider_id = provider.id,
                    provider_name = &provider.name,
                    "供应商「{}」自动恢复跳过：{}",
                    provider.name,
                    reason.message()
                );
                continue;
            }
        };

        if let Err(error) = crate::proxy::test_model(state, &provider, &model, &api_key).await {
            tracing::warn!(
                provider_id = provider.id,
                provider_name = &provider.name,
                "供应商「{}」自动恢复探测失败：{error}",
                provider.name
            );
            continue;
        }
        match crate::availability::recover_probe(
            &state.db,
            &state.failure_counter,
            provider.id,
            &provider.name,
            provider.updated_at,
        )
        .await
        {
            Ok(true) => recovered += 1,
            Ok(false) => {}
            Err(error) => {
                tracing::warn!(
                    provider_id = provider.id,
                    provider_name = &provider.name,
                    "供应商「{}」自动恢复状态更新失败：{error}",
                    provider.name
                )
            }
        }
    }

    Ok(recovered)
}

/// 用量门控对自动恢复探测的裁决：供应商未开启用量查询时不设门（允许探测）；
/// 开启用量查询时，用量查询失败或判定为不可用则阻止探测。
enum ProbeGate {
    /// 未开启用量查询，直接允许探测。
    Allowed,
    /// 用量查询失败，无法判定 → 跳过（query 失败已在上层记录）。
    Blocked,
    /// 用量判定为不可用（余额/额度耗尽）→ 跳过并点名说明。
    UsageUnusable,
}

/// 判定用量是否允许探测。用量查询失败与余额/额度耗尽都阻止探测，
/// 但原因不同：前者是数据不可得，后者是确定性不可用。
async fn probe_gate(db: &sea_orm::DatabaseConnection, provider: &provider::Model) -> ProbeGate {
    if !crate::usage::usage_enabled(&provider.extra) {
        return ProbeGate::Allowed;
    }
    let data = match crate::usage::persist::fetch_and_store(db, provider.id).await {
        Ok(data) => data,
        Err(error) => {
            tracing::warn!(
                provider_id = provider.id,
                provider_name = &provider.name,
                "供应商「{}」自动恢复用量查询失败：{error}",
                provider.name
            );
            return ProbeGate::Blocked;
        }
    };
    if data.usable_for_billing_mode(provider.billing_mode) == Some(true) {
        ProbeGate::Allowed
    } else {
        ProbeGate::UsageUnusable
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cron::log_capture::{JobLogEvent, JobLogLayer, SUBSCRIBER_LOCK};
    use crate::cron::scheduler::SchedulerRuntime;
    use crate::cron::worker::JobWorker;
    use sea_orm::{ActiveModelTrait, Set};
    use std::sync::Arc;
    use tokio::sync::broadcast;
    use tracing::Instrument;
    use tracing_subscriber::Registry;
    use tracing_subscriber::layer::SubscriberExt;

    async fn test_state() -> AppState {
        // 单连接内存库：多连接池的内存库每连接独立，种子插入与任务查询互不可见。
        let db = sea_orm::Database::connect("sqlite::memory:").await.unwrap();
        crate::db::migrate(&db).await.unwrap();
        let (log_tx, _) = broadcast::channel::<Arc<JobLogEvent>>(8192);
        let worker = JobWorker::new_with_settings(
            db.clone(),
            2,
            100,
            log_tx.clone(),
            crate::app_settings::AppSettings::default(),
        );
        let handle = worker.start();
        let scheduler = SchedulerRuntime::new_with_settings(
            handle.tx,
            crate::app_settings::AppSettings::default(),
        )
        .await
        .unwrap();
        AppState {
            db,
            scheduler,
            log_tx,
            lb_state: crate::proxy::LbState::default(),
            failure_counter: crate::availability::FailureCounter::default(),
            recheck_gate: crate::proxy::failure_recheck::RecheckGate::default(),
            upstream_pool: crate::proxy::pool::UpstreamPool::new(std::time::Duration::from_secs(
                600,
            )),
            settings: crate::app_settings::AppSettings::default(),
            usage_mem: Default::default(),
        }
    }

    /// 恢复候选供应商无模型时，跳过日志点名供应商（消息文本带「供应商「{name}」」，
    /// 任务日志 UI 只渲染 message）。无模型路径不触发网络请求。
    #[tokio::test(flavor = "current_thread")]
    #[allow(clippy::await_holding_lock)]
    async fn recovery_skip_logs_name_the_provider() {
        let _lock = SUBSCRIBER_LOCK.lock().unwrap();
        let (log_tx, mut log_rx) = broadcast::channel::<Arc<JobLogEvent>>(8192);
        let keep_alive = log_tx.clone();
        let subscriber = Registry::default().with(JobLogLayer::new(log_tx));
        let _guard = tracing::subscriber::set_default(subscriber);

        let state = test_state().await;
        let now = chrono::Utc::now();
        let provider_name = "待恢复供应商".to_string();
        provider::ActiveModel {
            name: Set(provider_name.clone()),
            enable: Set(false),
            base_url: Set("https://api.example.com/v1".to_string()),
            api_key: Set(crate::crypto::encrypt("sk-x")),
            custom_header: Set("{}".to_string()),
            protocol_type: Set(0),
            billing_mode: Set(0),
            extra: Set("{}".to_string()),
            disabled_reason: Set(Some("failure".to_string())),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(&state.db)
        .await
        .unwrap();

        let span = tracing::info_span!(
            target: "cron_job_log",
            "cron_job_run",
            job_name = "failure_recovery",
            run_id = "run-1",
        );
        let recovered = recover_failure_disabled(&state)
            .instrument(span)
            .await
            .unwrap();
        assert_eq!(recovered, 0, "无模型的候选不应恢复");

        let mut messages = Vec::new();
        while let Ok(event) = log_rx.try_recv() {
            if let Some(m) = event.message.clone() {
                messages.push(m);
            }
        }
        assert!(
            messages
                .iter()
                .any(|m| m.contains(&format!("供应商「{provider_name}」自动恢复跳过：没有模型"))),
            "无模型跳过日志未点名供应商: {messages:?}"
        );
        drop(keep_alive);
    }
}

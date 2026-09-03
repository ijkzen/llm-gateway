//! 内置定时任务的种子行（与 `src/lib.rs::init` 中注册的 handler 一一对应）。
//!
//! 仓库没有创建任务的 API，内置周期任务通过启动时幂等 upsert 种子行进入调度器。

use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};

use crate::entity::cron_job;

/// 用量刷新任务名：每 5 分钟刷新全部已开启用量展示的供应商用量并落库，
/// 同时执行订阅额度耗尽自动停用/恢复（见 `src/usage/persist.rs`）。
pub const USAGE_REFRESH_JOB: &str = "usage_refresh";
/// 连续失败供应商自动恢复任务名：每个整点复查并恢复已恢复健康的供应商。
pub const FAILURE_RECOVERY_JOB: &str = "failure_recovery";

/// 内置任务的默认标题（按语言）。语言切换同步未自定义任务时复用。
pub fn default_title(name: &str, lang: crate::i18n::Lang) -> String {
    match name {
        USAGE_REFRESH_JOB => lang
            .tr("供应商用量刷新", "Provider Usage Refresh")
            .to_string(),
        FAILURE_RECOVERY_JOB => lang
            .tr("连续失败供应商恢复", "Failed Provider Recovery")
            .to_string(),
        _ => lang.tr("定时任务", "Cron Job").to_string(),
    }
}

/// 内置任务的默认描述（按语言）。
pub fn default_description(name: &str, lang: crate::i18n::Lang) -> String {
    match name {
        USAGE_REFRESH_JOB => lang
            .tr(
                "每 5 分钟刷新所有已开启用量展示的供应商用量并写入数据库缓存；\
                 订阅额度耗尽时自动停用对应供应商及其虚拟模型子模型，恢复后自动启用",
                "Refreshes usage for all providers with usage query enabled every 5 \
                 minutes and writes the database cache; automatically disables a \
                 provider (and its virtual model members) when its subscription \
                 quota is exhausted, and re-enables it once restored",
            )
            .to_string(),
        FAILURE_RECOVERY_JOB => lang
            .tr(
                "每个整点复查因连续失败而禁用的供应商；用量可用且模型探测成功后恢复供应商及其虚拟模型子模型",
                "Checks providers disabled by consecutive failures every hour and restores the provider and its virtual model members after usage and model probes succeed",
            )
            .to_string(),
        _ => lang
            .tr("系统内置定时任务", "Built-in scheduled job")
            .to_string(),
    }
}

/// 确保 `failure_recovery` 任务行存在（不存在则插入，幂等）。
pub async fn ensure_failure_recovery_job(db: &DatabaseConnection) -> anyhow::Result<()> {
    ensure_job(db, FAILURE_RECOVERY_JOB, "@hourly").await
}

/// 确保 `usage_refresh` 任务行存在（不存在则插入，幂等）。
pub async fn ensure_usage_refresh_job(db: &DatabaseConnection) -> anyhow::Result<()> {
    ensure_job(db, USAGE_REFRESH_JOB, "@every 5m").await
}

async fn ensure_job(db: &DatabaseConnection, name: &str, expression: &str) -> anyhow::Result<()> {
    let exists = cron_job::Entity::find()
        .filter(cron_job::Column::Name.eq(name))
        .one(db)
        .await?;
    if exists.is_some() {
        return Ok(());
    }
    let now = chrono::Utc::now();
    let lang = crate::i18n::Lang::default();
    cron_job::ActiveModel {
        name: Set(name.to_string()),
        title: Set(default_title(name, lang)),
        description: Set(default_description(name, lang)),
        expression: Set(expression.to_string()),
        enabled: Set(true),
        group: Set("system".to_string()),
        last_run_at: Set(now),
        next_run_at: Set(now),
        created_at: Set(now),
        updated_at: Set(now),
        is_deleted: Set(false),
        ..Default::default()
    }
    .insert(db)
    .await?;
    tracing::info!(name, expression, "已创建内置定时任务");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn seed_is_idempotent_and_loadable() {
        let db = crate::db::connect("sqlite::memory:").await.unwrap();
        ensure_usage_refresh_job(&db).await.unwrap();
        ensure_usage_refresh_job(&db).await.unwrap();

        let rows = cron_job::Entity::find()
            .filter(cron_job::Column::Name.eq(USAGE_REFRESH_JOB))
            .all(&db)
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].expression, "@every 5m");
        assert!(rows[0].enabled);
        assert_eq!(rows[0].group, "system");
        assert!(!rows[0].is_deleted);
    }
}

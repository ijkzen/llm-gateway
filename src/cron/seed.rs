//! 内置定时任务的种子行（与 `src/lib.rs::init` 中注册的 handler 一一对应）。
//!
//! 仓库没有创建任务的 API，内置周期任务通过启动时幂等 upsert 种子行进入调度器。

use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};

use crate::entity::cron_job;

/// 用量刷新任务名：每 5 分钟刷新全部已开启用量展示的供应商用量并落库，
/// 同时执行订阅额度耗尽自动停用/恢复（见 `src/usage/persist.rs`）。
pub const USAGE_REFRESH_JOB: &str = "usage_refresh";

/// 确保 `usage_refresh` 任务行存在（不存在则插入，幂等）。
pub async fn ensure_usage_refresh_job(db: &DatabaseConnection) -> anyhow::Result<()> {
    let exists = cron_job::Entity::find()
        .filter(cron_job::Column::Name.eq(USAGE_REFRESH_JOB))
        .one(db)
        .await?;
    if exists.is_some() {
        return Ok(());
    }
    let now = chrono::Utc::now();
    cron_job::ActiveModel {
        name: Set(USAGE_REFRESH_JOB.to_string()),
        title: Set("供应商用量刷新".to_string()),
        description: Set(
            "每 5 分钟刷新所有已开启用量展示的供应商用量并写入数据库缓存；\
             订阅额度耗尽时自动停用对应供应商及其虚拟模型子模型，恢复后自动启用"
                .to_string(),
        ),
        expression: Set("@every 5m".to_string()),
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
    tracing::info!("已创建内置定时任务 {USAGE_REFRESH_JOB}（每 5 分钟）");
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

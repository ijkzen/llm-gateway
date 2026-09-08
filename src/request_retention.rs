//! request 指标表保留策略：按天清理超过保留期的历史行（S3）。
//!
//! request 表只增不减（每次转发一行 + failover 每尝试一行），summary 默认
//! 全历史聚合；给保留期让磁盘与聚合成本有上界。保留期由环境变量
//! `REQUEST_LOG_RETENTION_DAYS` 配置（默认 90 天）。

use sea_orm::{ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter};

use crate::entity::request;

/// 清理 `start_time` 早于保留期的 request 行，返回删除行数。
pub async fn cleanup_request_records(
    db: &DatabaseConnection,
    retention_days: u64,
) -> Result<u64, DbErr> {
    let cutoff_ms = chrono::Utc::now().timestamp_millis() - (retention_days * 86_400_000) as i64;
    let result = request::Entity::delete_many()
        .filter(request::Column::StartTime.lt(cutoff_ms))
        .exec(db)
        .await?;
    Ok(result.rows_affected)
}

/// 每日执行一次清理（与日志文件清理同节奏）。
pub fn spawn_request_cleanup_task(db: DatabaseConnection, retention_days: u64) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(24 * 60 * 60));
        loop {
            ticker.tick().await;
            match cleanup_request_records(&db, retention_days).await {
                Ok(removed) if removed > 0 => {
                    tracing::info!("清理了 {removed} 条超过保留期的 request 指标记录");
                }
                Ok(_) => {}
                Err(e) => tracing::warn!("request 指标保留期清理失败：{e}"),
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::ActiveModelTrait;

    #[tokio::test]
    async fn cleanup_removes_only_rows_older_than_retention() {
        let db = crate::db::connect("sqlite::memory:").await.unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        let insert = |start_time: i64| {
            let db = db.clone();
            async move {
                request::ActiveModel {
                    request_id: sea_orm::Set(format!("req-{start_time}")),
                    virtual_model_id: sea_orm::Set(1),
                    provider_id: sea_orm::Set(1),
                    model_id: sea_orm::Set("m".to_string()),
                    stream: sea_orm::Set(false),
                    ttft: sea_orm::Set(None),
                    input_tokens: sea_orm::Set(None),
                    input_cache_tokens: sea_orm::Set(0),
                    input_cache_rate: sea_orm::Set(0.0),
                    output_tokens: sea_orm::Set(None),
                    output_tokens_time: sea_orm::Set(None),
                    tps: sea_orm::Set(0.0),
                    start_time: sea_orm::Set(start_time),
                    end_time: sea_orm::Set(start_time),
                    request_time: sea_orm::Set(0),
                    success: sea_orm::Set(true),
                    fail_reason: sea_orm::Set(None),
                    total_tokens: sea_orm::Set(None),
                    api_key_name: sea_orm::Set("k".to_string()),
                }
                .insert(&db)
                .await
                .unwrap();
            }
        };
        // 200 天前（应删）与 1 天前（应留）。
        insert(now - 200 * 86_400_000).await;
        insert(now - 86_400_000).await;

        let removed = cleanup_request_records(&db, 90).await.unwrap();
        assert_eq!(removed, 1);
        let remaining = request::Entity::find().all(&db).await.unwrap();
        assert_eq!(remaining.len(), 1);
        assert!(remaining[0].start_time > now - 10 * 86_400_000);
    }
}

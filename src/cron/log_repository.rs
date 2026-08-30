//! 定时任务执行日志（runs + logs）的持久化层。

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect, Set, TransactionTrait,
};

use crate::entity::{cron_job_log, cron_job_run};

/// 每个任务最多保留的最近执行次数，更早的连同日志清理。
pub const MAX_RUNS_KEPT: u64 = 30;

#[derive(Clone, Debug)]
pub struct RunRecord {
    pub run_id: String,
    pub job_name: String,
    pub status: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub log_count: i32,
    pub truncated: bool,
}

impl From<cron_job_run::Model> for RunRecord {
    fn from(model: cron_job_run::Model) -> Self {
        Self {
            run_id: model.run_id,
            job_name: model.job_name,
            status: model.status,
            started_at: model.started_at,
            ended_at: model.ended_at,
            log_count: model.log_count,
            truncated: model.truncated,
        }
    }
}

#[derive(Clone, Debug)]
pub struct LogRecord {
    pub seq: i32,
    pub level: String,
    pub message: String,
    pub ts: DateTime<Utc>,
}

impl From<cron_job_log::Model> for LogRecord {
    fn from(model: cron_job_log::Model) -> Self {
        Self {
            seq: model.seq,
            level: model.level,
            message: model.message,
            ts: model.created_at,
        }
    }
}

#[async_trait]
pub trait CronJobLogRepository: Send + Sync + Clone {
    /// 记录一次执行开始。
    async fn insert_run(
        &self,
        run_id: &str,
        job_name: &str,
        started_at: DateTime<Utc>,
    ) -> Result<(), DbErr>;

    /// 记录执行结束状态与统计。
    async fn finish_run(
        &self,
        run_id: &str,
        status: &str,
        ended_at: DateTime<Utc>,
        log_count: i32,
        truncated: bool,
    ) -> Result<bool, DbErr>;

    /// 追加一条日志。
    async fn insert_log(
        &self,
        run_id: &str,
        seq: i32,
        level: &str,
        message: &str,
        ts: DateTime<Utc>,
    ) -> Result<(), DbErr>;

    /// 最近 `limit` 次执行（按开始时间倒序，最新在前）。
    async fn list_runs(&self, job_name: &str, limit: u64) -> Result<Vec<RunRecord>, DbErr>;

    /// 指定执行的日志（按 seq 升序）。
    async fn list_logs(&self, run_id: &str) -> Result<Vec<LogRecord>, DbErr>;

    /// 进程启动时把残留的 running 执行标记为 failed（服务重启导致中断）。
    async fn mark_interrupted_runs_failed(&self) -> Result<u64, DbErr>;

    /// 清理超出 `keep` 次之外的旧执行及其日志。
    async fn prune_old_runs(&self, job_name: &str, keep: u64) -> Result<(), DbErr>;
}

#[derive(Clone)]
pub struct SeaOrmCronJobLogRepository {
    db: DatabaseConnection,
}

impl SeaOrmCronJobLogRepository {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }
}

#[async_trait]
impl CronJobLogRepository for SeaOrmCronJobLogRepository {
    async fn insert_run(
        &self,
        run_id: &str,
        job_name: &str,
        started_at: DateTime<Utc>,
    ) -> Result<(), DbErr> {
        cron_job_run::ActiveModel {
            run_id: Set(run_id.to_string()),
            job_name: Set(job_name.to_string()),
            status: Set("running".to_string()),
            started_at: Set(started_at),
            ended_at: Set(None),
            log_count: Set(0),
            truncated: Set(false),
            ..Default::default()
        }
        .insert(&self.db)
        .await?;
        Ok(())
    }

    async fn finish_run(
        &self,
        run_id: &str,
        status: &str,
        ended_at: DateTime<Utc>,
        log_count: i32,
        truncated: bool,
    ) -> Result<bool, DbErr> {
        let result = cron_job_run::Entity::update_many()
            .filter(cron_job_run::Column::RunId.eq(run_id))
            .set(cron_job_run::ActiveModel {
                status: Set(status.to_string()),
                ended_at: Set(Some(ended_at)),
                log_count: Set(log_count),
                truncated: Set(truncated),
                ..Default::default()
            })
            .exec(&self.db)
            .await?;
        Ok(result.rows_affected > 0)
    }

    async fn insert_log(
        &self,
        run_id: &str,
        seq: i32,
        level: &str,
        message: &str,
        ts: DateTime<Utc>,
    ) -> Result<(), DbErr> {
        cron_job_log::ActiveModel {
            run_id: Set(run_id.to_string()),
            seq: Set(seq),
            level: Set(level.to_string()),
            message: Set(message.to_string()),
            created_at: Set(ts),
            ..Default::default()
        }
        .insert(&self.db)
        .await?;
        Ok(())
    }

    async fn list_runs(&self, job_name: &str, limit: u64) -> Result<Vec<RunRecord>, DbErr> {
        let runs = cron_job_run::Entity::find()
            .filter(cron_job_run::Column::JobName.eq(job_name))
            .order_by_desc(cron_job_run::Column::StartedAt)
            .limit(limit)
            .all(&self.db)
            .await?;
        Ok(runs.into_iter().map(Into::into).collect())
    }

    async fn list_logs(&self, run_id: &str) -> Result<Vec<LogRecord>, DbErr> {
        let logs = cron_job_log::Entity::find()
            .filter(cron_job_log::Column::RunId.eq(run_id))
            .order_by_asc(cron_job_log::Column::Seq)
            .all(&self.db)
            .await?;
        Ok(logs.into_iter().map(Into::into).collect())
    }

    async fn mark_interrupted_runs_failed(&self) -> Result<u64, DbErr> {
        let result = cron_job_run::Entity::update_many()
            .filter(cron_job_run::Column::Status.eq("running"))
            .set(cron_job_run::ActiveModel {
                status: Set("failed".to_string()),
                ended_at: Set(Some(Utc::now())),
                ..Default::default()
            })
            .exec(&self.db)
            .await?;
        Ok(result.rows_affected)
    }

    async fn prune_old_runs(&self, job_name: &str, keep: u64) -> Result<(), DbErr> {
        let runs = cron_job_run::Entity::find()
            .filter(cron_job_run::Column::JobName.eq(job_name))
            .order_by_desc(cron_job_run::Column::StartedAt)
            .all(&self.db)
            .await?;
        if runs.len() <= keep as usize {
            return Ok(());
        }
        let old_run_ids: Vec<String> = runs
            .into_iter()
            .skip(keep as usize)
            .map(|run| run.run_id)
            .collect();

        let txn = self.db.begin().await?;
        cron_job_log::Entity::delete_many()
            .filter(
                cron_job_log::Column::RunId
                    .is_in(old_run_ids.iter().map(|s| s.as_str()).collect::<Vec<_>>()),
            )
            .exec(&txn)
            .await?;
        cron_job_run::Entity::delete_many()
            .filter(
                cron_job_run::Column::RunId
                    .is_in(old_run_ids.iter().map(|s| s.as_str()).collect::<Vec<_>>()),
            )
            .exec(&txn)
            .await?;
        txn.commit().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::cron::log_capture::JobLogEvent;
    use crate::cron::test_utils::setup_db;

    use super::*;

    #[tokio::test]
    async fn test_insert_and_finish_run() {
        let db = setup_db().await;
        let repo = SeaOrmCronJobLogRepository::new(db);
        let started = Utc::now();
        repo.insert_run("run-a", "job_a", started).await.unwrap();

        let runs = repo.list_runs("job_a", 10).await.unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, "running");

        let ended = started + chrono::TimeDelta::seconds(5);
        repo.finish_run("run-a", "success", ended, 3, false)
            .await
            .unwrap();
        let runs = repo.list_runs("job_a", 10).await.unwrap();
        assert_eq!(runs[0].status, "success");
        assert_eq!(runs[0].ended_at, Some(ended));
        assert_eq!(runs[0].log_count, 3);
    }

    #[tokio::test]
    async fn test_insert_and_list_logs() {
        let db = setup_db().await;
        let repo = SeaOrmCronJobLogRepository::new(db);
        repo.insert_log("run-b", 1, "INFO", "first", Utc::now())
            .await
            .unwrap();
        repo.insert_log("run-b", 2, "WARN", "second", Utc::now())
            .await
            .unwrap();
        repo.insert_log("run-b", 3, "ERROR", "third", Utc::now())
            .await
            .unwrap();

        let logs = repo.list_logs("run-b").await.unwrap();
        assert_eq!(logs.len(), 3);
        assert_eq!(logs[0].message, "first");
        assert_eq!(logs[2].level, "ERROR");
    }

    #[tokio::test]
    async fn test_prune_old_runs_keeps_newest() {
        let db = setup_db().await;
        let repo = SeaOrmCronJobLogRepository::new(db);
        let base = Utc::now();
        for i in 0..5 {
            let run_id = format!("run-{i}");
            repo.insert_run(&run_id, "job_prune", base + chrono::TimeDelta::seconds(i))
                .await
                .unwrap();
            repo.insert_log(&run_id, 1, "INFO", &format!("log {i}"), base)
                .await
                .unwrap();
        }

        repo.prune_old_runs("job_prune", 2).await.unwrap();

        let runs = repo.list_runs("job_prune", 10).await.unwrap();
        assert_eq!(runs.len(), 2);
        // 最新两条（started_at 最大的）保留。
        assert_eq!(runs[0].run_id, "run-4");
        assert_eq!(runs[1].run_id, "run-3");
        // 被删执行的日志一并清理。
        assert!(repo.list_logs("run-0").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_prune_keeps_all_when_within_limit() {
        let db = setup_db().await;
        let repo = SeaOrmCronJobLogRepository::new(db);
        for i in 0..3 {
            repo.insert_run(&format!("run-{i}"), "job_keep", Utc::now())
                .await
                .unwrap();
        }
        repo.prune_old_runs("job_keep", 30).await.unwrap();
        assert_eq!(repo.list_runs("job_keep", 10).await.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn test_mark_interrupted_runs_failed() {
        let db = setup_db().await;
        let repo = SeaOrmCronJobLogRepository::new(db);
        repo.insert_run("run-x", "job_x", Utc::now()).await.unwrap();
        repo.insert_run("run-y", "job_y", Utc::now()).await.unwrap();
        repo.finish_run("run-y", "success", Utc::now(), 0, false)
            .await
            .unwrap();

        let affected = repo.mark_interrupted_runs_failed().await.unwrap();
        assert_eq!(affected, 1);
        let runs = repo.list_runs("job_x", 10).await.unwrap();
        assert_eq!(runs[0].status, "failed");
    }

    #[test]
    fn test_run_record_serializable_shape() {
        // 防止意外改动 JobLogEvent 的字段影响 SSE 契约。
        let event = JobLogEvent {
            kind: "log".to_string(),
            job_name: "j".to_string(),
            run_id: "r".to_string(),
            seq: Some(1),
            level: Some("INFO".to_string()),
            message: Some("m".to_string()),
            status: None,
            truncated: None,
            ts: "t".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"kind\":\"log\""));
        assert!(json.contains("\"job_name\":\"j\""));
    }
}

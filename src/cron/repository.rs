use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, Set,
};

use crate::cron::parser::compute_next_run;
use crate::entity::cron_job;

#[derive(Clone, Debug)]
pub struct JobDefinition {
    pub name: String,
    pub title: String,
    pub description: String,
    pub expression: String,
    pub enabled: bool,
    pub group: String,
}

impl From<&cron_job::Model> for JobDefinition {
    fn from(model: &cron_job::Model) -> Self {
        Self {
            name: model.name.clone(),
            title: model.title.clone(),
            description: model.description.clone(),
            expression: model.expression.clone(),
            enabled: model.enabled,
            group: model.group.clone(),
        }
    }
}

#[async_trait]
pub trait CronJobRepository: Send + Sync + Clone {
    async fn list_active(&self) -> Result<Vec<cron_job::Model>, DbErr>;
    async fn list_by_names(&self, names: &[String]) -> Result<Vec<cron_job::Model>, DbErr>;
    async fn find_by_name(&self, name: &str) -> Result<Option<cron_job::Model>, DbErr>;
    async fn insert(&self, job: &JobDefinition) -> Result<cron_job::Model, DbErr>;
    async fn update_run_times(
        &self,
        name: &str,
        last_run_at: DateTime<Utc>,
        next_run_at: DateTime<Utc>,
    ) -> Result<bool, DbErr>;
    async fn update_job_full(
        &self,
        name: &str,
        job: &JobDefinition,
        last_run_at: DateTime<Utc>,
        next_run_at: DateTime<Utc>,
    ) -> Result<Option<cron_job::Model>, DbErr>;
    async fn set_enabled(&self, name: &str, enabled: bool) -> Result<bool, DbErr>;
    async fn soft_delete(&self, name: &str) -> Result<bool, DbErr>;
    async fn restore(&self, name: &str, enabled: bool) -> Result<bool, DbErr>;
}

#[derive(Clone)]
pub struct SeaOrmCronJobRepository {
    db: DatabaseConnection,
}

impl SeaOrmCronJobRepository {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    pub async fn find_by_name_including_deleted(
        &self,
        name: &str,
    ) -> Result<Option<cron_job::Model>, DbErr> {
        cron_job::Entity::find()
            .filter(cron_job::Column::Name.eq(name))
            .one(&self.db)
            .await
    }
}

#[async_trait]
impl CronJobRepository for SeaOrmCronJobRepository {
    async fn list_active(&self) -> Result<Vec<cron_job::Model>, DbErr> {
        cron_job::Entity::find()
            .filter(cron_job::Column::IsDeleted.eq(false))
            .all(&self.db)
            .await
    }

    async fn list_by_names(&self, names: &[String]) -> Result<Vec<cron_job::Model>, DbErr> {
        if names.is_empty() {
            return Ok(vec![]);
        }
        cron_job::Entity::find()
            .filter(
                cron_job::Column::Name.is_in(names.iter().map(|s| s.as_str()).collect::<Vec<_>>()),
            )
            .all(&self.db)
            .await
    }

    async fn find_by_name(&self, name: &str) -> Result<Option<cron_job::Model>, DbErr> {
        cron_job::Entity::find()
            .filter(cron_job::Column::Name.eq(name))
            .filter(cron_job::Column::IsDeleted.eq(false))
            .one(&self.db)
            .await
    }

    async fn insert(&self, job: &JobDefinition) -> Result<cron_job::Model, DbErr> {
        let now = Utc::now();
        let epoch: DateTime<Utc> = DateTime::UNIX_EPOCH;
        let active = cron_job::ActiveModel {
            name: Set(job.name.clone()),
            title: Set(job.title.clone()),
            description: Set(job.description.clone()),
            expression: Set(job.expression.clone()),
            enabled: Set(job.enabled),
            group: Set(job.group.clone()),
            last_run_at: Set(epoch),
            next_run_at: Set(compute_next_run(&job.expression).unwrap_or(now)),
            created_at: Set(now),
            updated_at: Set(now),
            is_deleted: Set(false),
            ..Default::default()
        };
        active.insert(&self.db).await
    }

    async fn update_run_times(
        &self,
        name: &str,
        last_run_at: DateTime<Utc>,
        next_run_at: DateTime<Utc>,
    ) -> Result<bool, DbErr> {
        let result = cron_job::Entity::update_many()
            .filter(cron_job::Column::Name.eq(name))
            .filter(cron_job::Column::IsDeleted.eq(false))
            .set(cron_job::ActiveModel {
                last_run_at: Set(last_run_at),
                next_run_at: Set(next_run_at),
                ..Default::default()
            })
            .exec(&self.db)
            .await?;
        Ok(result.rows_affected > 0)
    }

    async fn update_job_full(
        &self,
        name: &str,
        job: &JobDefinition,
        last_run_at: DateTime<Utc>,
        next_run_at: DateTime<Utc>,
    ) -> Result<Option<cron_job::Model>, DbErr> {
        use sea_orm::TransactionTrait;

        let txn = self.db.begin().await?;
        let now = Utc::now();
        let result = cron_job::Entity::update_many()
            .filter(cron_job::Column::Name.eq(name))
            .filter(cron_job::Column::IsDeleted.eq(false))
            .set(cron_job::ActiveModel {
                title: Set(job.title.clone()),
                description: Set(job.description.clone()),
                expression: Set(job.expression.clone()),
                group: Set(job.group.clone()),
                enabled: Set(job.enabled),
                last_run_at: Set(last_run_at),
                next_run_at: Set(next_run_at),
                updated_at: Set(now),
                ..Default::default()
            })
            .exec(&txn)
            .await?;
        if result.rows_affected == 0 {
            txn.commit().await?;
            return Ok(None);
        }
        let model = cron_job::Entity::find()
            .filter(cron_job::Column::Name.eq(name))
            .one(&txn)
            .await?;
        txn.commit().await?;
        Ok(model)
    }

    async fn set_enabled(&self, name: &str, enabled: bool) -> Result<bool, DbErr> {
        let result = cron_job::Entity::update_many()
            .filter(cron_job::Column::Name.eq(name))
            .filter(cron_job::Column::IsDeleted.eq(false))
            .set(cron_job::ActiveModel {
                enabled: Set(enabled),
                updated_at: Set(Utc::now()),
                ..Default::default()
            })
            .exec(&self.db)
            .await?;
        Ok(result.rows_affected > 0)
    }

    async fn soft_delete(&self, name: &str) -> Result<bool, DbErr> {
        let result = cron_job::Entity::update_many()
            .filter(cron_job::Column::Name.eq(name))
            .set(cron_job::ActiveModel {
                is_deleted: Set(true),
                enabled: Set(false),
                updated_at: Set(Utc::now()),
                ..Default::default()
            })
            .exec(&self.db)
            .await?;
        Ok(result.rows_affected > 0)
    }

    async fn restore(&self, name: &str, enabled: bool) -> Result<bool, DbErr> {
        let result = cron_job::Entity::update_many()
            .filter(cron_job::Column::Name.eq(name))
            .set(cron_job::ActiveModel {
                is_deleted: Set(false),
                enabled: Set(enabled),
                updated_at: Set(Utc::now()),
                ..Default::default()
            })
            .exec(&self.db)
            .await?;
        Ok(result.rows_affected > 0)
    }
}

#[cfg(test)]
mod tests {
    use crate::cron::test_utils::{sample_job, setup_db};

    use super::*;

    #[tokio::test]
    async fn test_insert_and_find() {
        let db = setup_db().await;
        let repo = SeaOrmCronJobRepository::new(db);
        let job = sample_job("test_insert");
        repo.insert(&job).await.unwrap();
        let found = repo.find_by_name("test_insert").await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().expression, "@hourly");
    }

    #[tokio::test]
    async fn test_list_active_excludes_deleted() {
        let db = setup_db().await;
        let repo = SeaOrmCronJobRepository::new(db);
        repo.insert(&sample_job("active_job")).await.unwrap();
        repo.insert(&sample_job("deleted_job")).await.unwrap();
        repo.soft_delete("deleted_job").await.unwrap();
        let active = repo.list_active().await.unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].name, "active_job");
    }

    #[tokio::test]
    async fn test_set_enabled() {
        let db = setup_db().await;
        let repo = SeaOrmCronJobRepository::new(db);
        repo.insert(&sample_job("toggle_job")).await.unwrap();
        let changed = repo.set_enabled("toggle_job", false).await.unwrap();
        assert!(changed);
        let found = repo.find_by_name("toggle_job").await.unwrap().unwrap();
        assert!(!found.enabled);
    }

    #[tokio::test]
    async fn test_update_run_times() {
        let db = setup_db().await;
        let repo = SeaOrmCronJobRepository::new(db);
        let job = sample_job("run_times_job");
        let _model = repo.insert(&job).await.unwrap();
        let now = chrono::Utc::now();
        let updated = repo
            .update_run_times("run_times_job", now, now)
            .await
            .unwrap();
        assert!(updated);
        let found = repo.find_by_name("run_times_job").await.unwrap().unwrap();
        assert_eq!(found.last_run_at.timestamp(), now.timestamp());
    }

    #[tokio::test]
    async fn test_soft_delete() {
        let db = setup_db().await;
        let repo = SeaOrmCronJobRepository::new(db);
        repo.insert(&sample_job("soft_delete_job")).await.unwrap();
        let deleted = repo.soft_delete("soft_delete_job").await.unwrap();
        assert!(deleted);
        assert!(
            repo.find_by_name("soft_delete_job")
                .await
                .unwrap()
                .is_none()
        );
        let found = repo
            .find_by_name_including_deleted("soft_delete_job")
            .await
            .unwrap()
            .unwrap();
        assert!(found.is_deleted);
        assert!(!found.enabled);
    }

    #[tokio::test]
    async fn test_find_by_name_filters_deleted() {
        let db = setup_db().await;
        let repo = SeaOrmCronJobRepository::new(db);
        repo.insert(&sample_job("find_deleted_job")).await.unwrap();
        repo.soft_delete("find_deleted_job").await.unwrap();
        assert!(
            repo.find_by_name("find_deleted_job")
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            repo.find_by_name_including_deleted("find_deleted_job")
                .await
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn test_default_column_values() {
        let db = setup_db().await;
        let now = Utc::now();
        let epoch: DateTime<Utc> = DateTime::UNIX_EPOCH;
        let active = cron_job::ActiveModel {
            name: Set("default_values_job".to_string()),
            title: Set("Test".to_string()),
            description: Set("".to_string()),
            expression: Set("@hourly".to_string()),
            enabled: Set(true),
            last_run_at: Set(epoch),
            next_run_at: Set(now),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        active.insert(&db).await.unwrap();

        let repo = SeaOrmCronJobRepository::new(db);
        let model = repo
            .find_by_name_including_deleted("default_values_job")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(model.group, "other");
        assert!(!model.is_deleted);
    }

    #[tokio::test]
    async fn test_update_job_full() {
        let db = setup_db().await;
        let repo = SeaOrmCronJobRepository::new(db);
        repo.insert(&sample_job("full_update_job")).await.unwrap();
        let now = chrono::Utc::now();
        let updated = repo
            .update_job_full(
                "full_update_job",
                &JobDefinition {
                    name: "full_update_job".to_string(),
                    title: "New Title".to_string(),
                    description: "New Desc".to_string(),
                    expression: "@daily".to_string(),
                    enabled: false,
                    group: "new-group".to_string(),
                },
                now,
                now,
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.title, "New Title");
        assert_eq!(updated.description, "New Desc");
        assert_eq!(updated.expression, "@daily");
        assert!(!updated.enabled);
        assert_eq!(updated.group, "new-group");
        assert_eq!(updated.last_run_at.timestamp(), now.timestamp());
        assert_eq!(updated.next_run_at.timestamp(), now.timestamp());
    }
}

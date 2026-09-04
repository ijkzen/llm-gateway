use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use async_trait::async_trait;
use sea_orm::DbErr;

use crate::cron::JobContext;
use crate::cron::repository::{CronJobRepository, JobDefinition, SeaOrmCronJobRepository};
use crate::cron::test_utils::{sample_job, setup_db};
use crate::cron::worker::JobWorker;

use super::*;

#[tokio::test]
async fn test_scheduler_loads_from_db() {
    let db = setup_db().await;
    let repo = SeaOrmCronJobRepository::new(db.clone());
    let worker = JobWorker::new_with_settings(
        db.clone(),
        2,
        100,
        tokio::sync::broadcast::channel(64).0,
        AppSettings::default(),
    );
    let handle = worker.start();

    let scheduler = SchedulerRuntime::new_with_settings(handle.tx.clone(), AppSettings::default())
        .await
        .unwrap();

    let executed = Arc::new(AtomicBool::new(false));
    let flag = executed.clone();
    scheduler
        .register_handler(
            "scheduler_load_test",
            Arc::new(move |_ctx: JobContext| {
                let flag = flag.clone();
                Box::pin(async move {
                    flag.store(true, Ordering::SeqCst);
                    Ok(())
                })
            }),
        )
        .await;

    repo.insert(&sample_job("scheduler_load_test"), None)
        .await
        .unwrap();
    scheduler.load_from_db(&repo).await.unwrap();

    let jobs = scheduler.list_jobs().await;
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].name, "scheduler_load_test");
}

#[tokio::test]
async fn test_load_from_db_skips_missed_cron_jobs() {
    let db = setup_db().await;
    let repo = SeaOrmCronJobRepository::new(db.clone());
    let worker = JobWorker::new_with_settings(
        db.clone(),
        2,
        100,
        tokio::sync::broadcast::channel(64).0,
        AppSettings::default(),
    );
    let handle = worker.start();

    let scheduler = SchedulerRuntime::new_with_settings(handle.tx.clone(), AppSettings::default())
        .await
        .unwrap();

    let executed = Arc::new(AtomicBool::new(false));
    let flag = executed.clone();
    scheduler
        .register_handler(
            "missed_cron_test",
            Arc::new(move |_ctx: JobContext| {
                let flag = flag.clone();
                Box::pin(async move {
                    flag.store(true, Ordering::SeqCst);
                    Ok(())
                })
            }),
        )
        .await;

    repo.insert(&sample_job("missed_cron_test"), None)
        .await
        .unwrap();
    let past = chrono::Utc::now() - chrono::TimeDelta::minutes(5);
    repo.update_run_times("missed_cron_test", chrono::DateTime::UNIX_EPOCH, past)
        .await
        .unwrap();

    scheduler.load_from_db(&repo).await.unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    assert!(
        !executed.load(Ordering::SeqCst),
        "missed cron job should not be triggered on load"
    );

    let model = repo
        .find_by_name("missed_cron_test")
        .await
        .unwrap()
        .unwrap();
    assert!(
        model.next_run_at > chrono::Utc::now(),
        "next_run_at should be recomputed to a future time"
    );
}

#[tokio::test]
async fn test_run_job_now_returns_worker_channel_closed_when_receiver_dropped() {
    let db = setup_db().await;

    // Create a channel and drop the receiver so any send returns a Closed error.
    let (tx, _rx) = tokio::sync::mpsc::channel::<JobInvocation>(1);
    drop(_rx);

    let scheduler = SchedulerRuntime::new_with_settings(tx, AppSettings::default())
        .await
        .unwrap();
    scheduler
        .register_handler(
            "dropped_receiver_test",
            Arc::new(|_ctx: JobContext| Box::pin(async move { Ok(()) })),
        )
        .await;

    let repo = SeaOrmCronJobRepository::new(db);
    repo.insert(&sample_job("dropped_receiver_test"), None)
        .await
        .unwrap();
    scheduler.load_from_db(&repo).await.unwrap();

    let err = scheduler
        .run_job_now("dropped_receiver_test")
        .await
        .unwrap_err();
    assert!(matches!(err, SchedulerError::WorkerChannelClosed(_)));
}

#[tokio::test]
async fn test_soft_delete_removes_from_scheduler_and_db() {
    let db = setup_db().await;
    let repo = SeaOrmCronJobRepository::new(db.clone());
    let worker = JobWorker::new_with_settings(
        db.clone(),
        2,
        100,
        tokio::sync::broadcast::channel(64).0,
        AppSettings::default(),
    );
    let handle = worker.start();

    let scheduler = SchedulerRuntime::new_with_settings(handle.tx.clone(), AppSettings::default())
        .await
        .unwrap();
    scheduler
        .register_handler(
            "soft_delete_test",
            Arc::new(|_ctx: JobContext| Box::pin(async move { Ok(()) })),
        )
        .await;

    repo.insert(&sample_job("soft_delete_test"), None)
        .await
        .unwrap();
    scheduler.load_from_db(&repo).await.unwrap();

    scheduler
        .soft_delete_job(&repo, "soft_delete_test")
        .await
        .unwrap();

    let jobs = scheduler.list_jobs().await;
    assert!(jobs.is_empty());

    let model = repo
        .find_by_name_including_deleted("soft_delete_test")
        .await
        .unwrap()
        .unwrap();
    assert!(model.is_deleted);
}

#[derive(Clone)]
struct FailingRepo {
    inner: SeaOrmCronJobRepository,
    fail_insert: Arc<AtomicBool>,
    fail_set_enabled: Arc<AtomicBool>,
    fail_soft_delete: Arc<AtomicBool>,
}

impl FailingRepo {
    fn new(inner: SeaOrmCronJobRepository) -> Self {
        Self {
            inner,
            fail_insert: Arc::new(AtomicBool::new(false)),
            fail_set_enabled: Arc::new(AtomicBool::new(false)),
            fail_soft_delete: Arc::new(AtomicBool::new(false)),
        }
    }
}

/// Implements `CronJobRepository` for `FailingRepo`.
///
/// Methods listed in `delegates` are generated as plain pass-throughs to the
/// inner repository; methods in `overrides` are written out by hand and inject
/// the configured failures before delegating.
///
/// The macro has to emit the whole impl block, `#[async_trait]` included:
/// attribute macros run before `macro_rules!` invocations nested inside the
/// impl block are expanded, so `async_trait` would never transform the
/// generated `async fn`s to match the trait's desugared signatures.
macro_rules! impl_failing_repo {
    (
        delegates { $(fn $name:ident($($arg:ident : $ty:ty),* $(,)?) -> $ret:ty;)* }
        overrides { $($methods:item)* }
    ) => {
        #[async_trait]
        impl CronJobRepository for FailingRepo {
            $(
                async fn $name(&self, $($arg: $ty),*) -> $ret {
                    self.inner.$name($($arg),*).await
                }
            )*
            $($methods)*
        }
    };
}

impl_failing_repo! {
    delegates {
        fn list_active() -> Result<Vec<cron_job::Model>, DbErr>;
        fn list_by_names(names: &[String]) -> Result<Vec<cron_job::Model>, DbErr>;
        fn find_by_name(name: &str) -> Result<Option<cron_job::Model>, DbErr>;
        fn update_run_times(
            name: &str,
            last_run_at: chrono::DateTime<chrono::Utc>,
            next_run_at: chrono::DateTime<chrono::Utc>,
        ) -> Result<bool, DbErr>;
        fn update_job_full(
            name: &str,
            job: &JobDefinition,
            last_run_at: chrono::DateTime<chrono::Utc>,
            next_run_at: chrono::DateTime<chrono::Utc>,
        ) -> Result<Option<cron_job::Model>, DbErr>;
        fn restore(name: &str, enabled: bool) -> Result<bool, DbErr>;
    }
    overrides {
        async fn insert(
            &self,
            job: &JobDefinition,
            tz: Option<chrono_tz::Tz>,
        ) -> Result<cron_job::Model, DbErr> {
            if self.fail_insert.load(Ordering::SeqCst) {
                return Err(DbErr::Custom("mock insert failure".to_string()));
            }
            self.inner.insert(job, tz).await
        }

        async fn set_enabled(&self, name: &str, enabled: bool) -> Result<bool, DbErr> {
            if self.fail_set_enabled.load(Ordering::SeqCst) {
                return Err(DbErr::Custom("mock set_enabled failure".to_string()));
            }
            self.inner.set_enabled(name, enabled).await
        }

        async fn soft_delete(&self, name: &str) -> Result<bool, DbErr> {
            if self.fail_soft_delete.load(Ordering::SeqCst) {
                return Err(DbErr::Custom("mock soft_delete failure".to_string()));
            }
            self.inner.soft_delete(name).await
        }
    }
}

#[tokio::test]
async fn test_add_job_rollbacks_scheduler_on_db_failure() {
    let db = setup_db().await;
    let inner = SeaOrmCronJobRepository::new(db.clone());
    let repo = FailingRepo::new(inner);
    let worker = JobWorker::new_with_settings(
        db,
        2,
        100,
        tokio::sync::broadcast::channel(64).0,
        AppSettings::default(),
    );
    let handle = worker.start();

    let scheduler = SchedulerRuntime::new_with_settings(handle.tx.clone(), AppSettings::default())
        .await
        .unwrap();
    scheduler
        .register_handler(
            "add_rollback",
            Arc::new(|_ctx: JobContext| Box::pin(async move { Ok(()) })),
        )
        .await;

    repo.fail_insert.store(true, Ordering::SeqCst);
    let job = sample_job("add_rollback");
    let result = scheduler
        .add_job(
            &repo,
            &job,
            scheduler.get_handler("add_rollback").await.unwrap(),
        )
        .await;
    assert!(result.is_err());

    let jobs = scheduler.list_jobs().await;
    assert!(jobs.is_empty());
}

#[tokio::test]
async fn test_set_enabled_rollbacks_scheduler_on_db_failure() {
    let db = setup_db().await;
    let inner = SeaOrmCronJobRepository::new(db.clone());
    let repo = FailingRepo::new(inner);
    let worker = JobWorker::new_with_settings(
        db.clone(),
        2,
        100,
        tokio::sync::broadcast::channel(64).0,
        AppSettings::default(),
    );
    let handle = worker.start();

    let scheduler = SchedulerRuntime::new_with_settings(handle.tx.clone(), AppSettings::default())
        .await
        .unwrap();
    scheduler
        .register_handler(
            "enabled_rollback",
            Arc::new(|_ctx: JobContext| Box::pin(async move { Ok(()) })),
        )
        .await;

    repo.inner
        .insert(&sample_job("enabled_rollback"), None)
        .await
        .unwrap();
    scheduler.load_from_db(&repo).await.unwrap();

    repo.fail_set_enabled.store(true, Ordering::SeqCst);
    let result = scheduler
        .set_enabled(&repo, "enabled_rollback", false)
        .await;
    assert!(result.is_err());

    let jobs = scheduler.list_jobs().await;
    assert_eq!(jobs.len(), 1);
    assert!(jobs[0].enabled);
}

#[tokio::test]
async fn test_soft_delete_rollbacks_scheduler_on_db_failure() {
    let db = setup_db().await;
    let inner = SeaOrmCronJobRepository::new(db.clone());
    let repo = FailingRepo::new(inner);
    let worker = JobWorker::new_with_settings(
        db.clone(),
        2,
        100,
        tokio::sync::broadcast::channel(64).0,
        AppSettings::default(),
    );
    let handle = worker.start();

    let scheduler = SchedulerRuntime::new_with_settings(handle.tx.clone(), AppSettings::default())
        .await
        .unwrap();
    scheduler
        .register_handler(
            "delete_rollback",
            Arc::new(|_ctx: JobContext| Box::pin(async move { Ok(()) })),
        )
        .await;

    repo.inner
        .insert(&sample_job("delete_rollback"), None)
        .await
        .unwrap();
    scheduler.load_from_db(&repo).await.unwrap();

    repo.fail_soft_delete.store(true, Ordering::SeqCst);
    let result = scheduler.soft_delete_job(&repo, "delete_rollback").await;
    assert!(result.is_err());

    let jobs = scheduler.list_jobs().await;
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].name, "delete_rollback");

    let model = repo
        .inner
        .find_by_name("delete_rollback")
        .await
        .unwrap()
        .unwrap();
    assert!(!model.is_deleted);
}

#[tokio::test]
async fn test_modification_lock_serializes_concurrent_mutations() {
    let db = setup_db().await;
    let repo = SeaOrmCronJobRepository::new(db.clone());
    let worker = JobWorker::new_with_settings(
        db.clone(),
        2,
        100,
        tokio::sync::broadcast::channel(64).0,
        AppSettings::default(),
    );
    let handle = worker.start();

    let scheduler = SchedulerRuntime::new_with_settings(handle.tx.clone(), AppSettings::default())
        .await
        .unwrap();
    scheduler
        .register_handler(
            "concurrent_job",
            Arc::new(|_ctx: JobContext| Box::pin(async move { Ok(()) })),
        )
        .await;

    repo.insert(&sample_job("concurrent_job"), None)
        .await
        .unwrap();
    scheduler.load_from_db(&repo).await.unwrap();

    let scheduler_a = scheduler.clone();
    let scheduler_b = scheduler.clone();
    let repo_b = repo.clone();

    let task_a = tokio::spawn(async move {
        let _guard = scheduler_a.modification_lock().await;
        scheduler_a
            .update_job_in_memory(
                "concurrent_job",
                &JobDefinition {
                    title: "Updated Title".to_string(),
                    expression: "@daily".to_string(),
                    ..sample_job("concurrent_job")
                },
            )
            .await
    });
    let task_b = tokio::spawn(async move {
        let _guard = scheduler_b.modification_lock().await;
        scheduler_b.soft_delete_job(&repo_b, "concurrent_job").await
    });

    let (result_a, result_b) = tokio::join!(task_a, task_b);
    let _ = result_a.unwrap();
    let _ = result_b.unwrap();

    let jobs = scheduler.list_jobs().await;
    let model = repo.find_by_name("concurrent_job").await.unwrap();

    // With serialization, the final state must be either deleted or
    // updated, never both and never duplicated.
    let deleted_state = model.is_none() && jobs.is_empty();
    let updated_state = model.is_some()
        && jobs.len() == 1
        && jobs[0].title == "Updated Title"
        && jobs[0].expression == "@daily";
    assert!(
        deleted_state || updated_state,
        "inconsistent final state: jobs={:?}, model={:?}",
        jobs,
        model
    );
}

#[tokio::test]
async fn test_update_job_in_memory_updates_metadata() {
    let db = setup_db().await;
    let repo = SeaOrmCronJobRepository::new(db.clone());
    let worker = JobWorker::new_with_settings(
        db.clone(),
        2,
        100,
        tokio::sync::broadcast::channel(64).0,
        AppSettings::default(),
    );
    let handle = worker.start();

    let scheduler = SchedulerRuntime::new_with_settings(handle.tx.clone(), AppSettings::default())
        .await
        .unwrap();
    scheduler
        .register_handler(
            "metadata_update_test",
            Arc::new(|_ctx: JobContext| Box::pin(async move { Ok(()) })),
        )
        .await;

    repo.insert(&sample_job("metadata_update_test"), None)
        .await
        .unwrap();
    scheduler.load_from_db(&repo).await.unwrap();

    scheduler
        .update_job_in_memory(
            "metadata_update_test",
            &JobDefinition {
                title: "New Title".to_string(),
                description: "New Desc".to_string(),
                group: "new-group".to_string(),
                ..sample_job("metadata_update_test")
            },
        )
        .await
        .unwrap();

    let jobs = scheduler.list_jobs().await;
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].title, "New Title");
    assert_eq!(jobs[0].description, "New Desc");
    assert_eq!(jobs[0].group, "new-group");
    assert_eq!(jobs[0].expression, "@hourly");
    assert!(jobs[0].enabled);
}

#[tokio::test]
async fn test_update_job_in_memory_updates_enabled() {
    let db = setup_db().await;
    let repo = SeaOrmCronJobRepository::new(db.clone());
    let worker = JobWorker::new_with_settings(
        db.clone(),
        2,
        100,
        tokio::sync::broadcast::channel(64).0,
        AppSettings::default(),
    );
    let handle = worker.start();

    let scheduler = SchedulerRuntime::new_with_settings(handle.tx.clone(), AppSettings::default())
        .await
        .unwrap();
    scheduler
        .register_handler(
            "enabled_update_test",
            Arc::new(|_ctx: JobContext| Box::pin(async move { Ok(()) })),
        )
        .await;

    repo.insert(&sample_job("enabled_update_test"), None)
        .await
        .unwrap();
    scheduler.load_from_db(&repo).await.unwrap();

    scheduler
        .update_job_in_memory(
            "enabled_update_test",
            &JobDefinition {
                enabled: false,
                ..sample_job("enabled_update_test")
            },
        )
        .await
        .unwrap();

    let jobs = scheduler.list_jobs().await;
    assert_eq!(jobs.len(), 1);
    assert!(!jobs[0].enabled);
}

#[tokio::test]
async fn test_update_job_in_memory_recreates_on_expression_change() {
    let db = setup_db().await;
    let repo = SeaOrmCronJobRepository::new(db.clone());
    let worker = JobWorker::new_with_settings(
        db.clone(),
        2,
        100,
        tokio::sync::broadcast::channel(64).0,
        AppSettings::default(),
    );
    let handle = worker.start();

    let scheduler = SchedulerRuntime::new_with_settings(handle.tx.clone(), AppSettings::default())
        .await
        .unwrap();
    scheduler
        .register_handler(
            "expression_update_test",
            Arc::new(|_ctx: JobContext| Box::pin(async move { Ok(()) })),
        )
        .await;

    repo.insert(&sample_job("expression_update_test"), None)
        .await
        .unwrap();
    scheduler.load_from_db(&repo).await.unwrap();

    scheduler
        .update_job_in_memory(
            "expression_update_test",
            &JobDefinition {
                expression: "@daily".to_string(),
                enabled: false,
                ..sample_job("expression_update_test")
            },
        )
        .await
        .unwrap();

    let jobs = scheduler.list_jobs().await;
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].expression, "@daily");
    assert!(!jobs[0].enabled);
    assert_eq!(jobs[0].frequency_secs, 86400);
}

#[tokio::test]
async fn test_update_job_in_memory_returns_not_found_for_missing_job() {
    let db = setup_db().await;
    let worker = JobWorker::new_with_settings(
        db.clone(),
        2,
        100,
        tokio::sync::broadcast::channel(64).0,
        AppSettings::default(),
    );
    let handle = worker.start();

    let scheduler = SchedulerRuntime::new_with_settings(handle.tx.clone(), AppSettings::default())
        .await
        .unwrap();

    let err = scheduler
        .update_job_in_memory("missing_job", &sample_job("missing_job"))
        .await
        .unwrap_err();
    assert!(matches!(err, SchedulerError::JobNotFound(_)));
}

#[tokio::test]
async fn test_disabled_job_does_not_fire_until_enabled() {
    let db = setup_db().await;
    let repo = SeaOrmCronJobRepository::new(db.clone());
    let worker = JobWorker::new_with_settings(
        db.clone(),
        2,
        100,
        tokio::sync::broadcast::channel(64).0,
        AppSettings::default(),
    );
    let handle = worker.start();

    let scheduler = SchedulerRuntime::new_with_settings(handle.tx.clone(), AppSettings::default())
        .await
        .unwrap();

    let counter = Arc::new(AtomicUsize::new(0));
    let c = counter.clone();
    scheduler
        .register_handler(
            "disabled_no_fire",
            Arc::new(move |_ctx: JobContext| {
                let c = c.clone();
                Box::pin(async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
            }),
        )
        .await;

    let mut job = sample_job("disabled_no_fire");
    job.expression = "@every 1s".to_string();
    job.enabled = false;
    repo.insert(&job, None).await.unwrap();
    scheduler.load_from_db(&repo).await.unwrap();
    scheduler.start().await.unwrap();

    // Regression: tokio-cron-scheduler's set_stop() does not prevent
    // firing, so a disabled job must simply not be in the scheduler.
    tokio::time::sleep(tokio::time::Duration::from_millis(1600)).await;
    assert_eq!(
        counter.load(Ordering::SeqCst),
        0,
        "disabled job must not fire"
    );

    // Enabling must schedule the job: it starts firing.
    scheduler
        .set_enabled(&repo, "disabled_no_fire", true)
        .await
        .unwrap();
    tokio::time::sleep(tokio::time::Duration::from_millis(1600)).await;
    assert!(
        counter.load(Ordering::SeqCst) >= 1,
        "enabled job did not fire"
    );

    // Disabling again must stop the firing.
    scheduler
        .set_enabled(&repo, "disabled_no_fire", false)
        .await
        .unwrap();
    let count_at_disable = counter.load(Ordering::SeqCst);
    tokio::time::sleep(tokio::time::Duration::from_millis(1600)).await;
    assert_eq!(
        counter.load(Ordering::SeqCst),
        count_at_disable,
        "job fired after being disabled"
    );
}

#[tokio::test]
async fn test_disabled_job_stays_listed_and_runnable_manually() {
    let db = setup_db().await;
    let repo = SeaOrmCronJobRepository::new(db.clone());
    let worker = JobWorker::new_with_settings(
        db.clone(),
        2,
        100,
        tokio::sync::broadcast::channel(64).0,
        AppSettings::default(),
    );
    let handle = worker.start();

    let scheduler = SchedulerRuntime::new_with_settings(handle.tx.clone(), AppSettings::default())
        .await
        .unwrap();

    let counter = Arc::new(AtomicUsize::new(0));
    let c = counter.clone();
    scheduler
        .register_handler(
            "disabled_manual_run",
            Arc::new(move |_ctx: JobContext| {
                let c = c.clone();
                Box::pin(async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
            }),
        )
        .await;

    let mut job = sample_job("disabled_manual_run");
    job.enabled = false;
    repo.insert(&job, None).await.unwrap();
    scheduler.load_from_db(&repo).await.unwrap();

    let jobs = scheduler.list_jobs().await;
    assert_eq!(jobs.len(), 1);
    assert!(!jobs[0].enabled);
    assert!(scheduler.has_job("disabled_manual_run").await);
    assert!(!scheduler.has_job("missing_job").await);

    scheduler.run_job_now("disabled_manual_run").await.unwrap();
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

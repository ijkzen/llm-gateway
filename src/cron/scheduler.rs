use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use chrono::Utc;

use tokio::sync::{Mutex, RwLock, mpsc};
use tokio_cron_scheduler::{Job, JobScheduler};

use crate::cron::parser::{
    ScheduleType, compute_frequency_secs, compute_next_run, parse_expression,
};
use crate::cron::repository::{CronJobRepository, JobDefinition};
use crate::cron::worker::JobInvocation;
use crate::cron::{JobHandler, JobInfo, SchedulerError};
use crate::entity::cron_job;

#[derive(Clone)]
pub struct JobEntry {
    pub name: String,
    pub title: String,
    pub description: String,
    pub expression: String,
    pub job: Job,
    pub enabled: bool,
    pub group: String,
    pub handler: JobHandler,
}

impl From<&JobEntry> for JobDefinition {
    fn from(entry: &JobEntry) -> Self {
        Self {
            name: entry.name.clone(),
            title: entry.title.clone(),
            description: entry.description.clone(),
            expression: entry.expression.clone(),
            enabled: entry.enabled,
            group: entry.group.clone(),
        }
    }
}

#[derive(Clone)]
pub struct SchedulerRuntime {
    scheduler: Arc<Mutex<JobScheduler>>,
    jobs: Arc<RwLock<HashMap<String, JobEntry>>>,
    handlers: Arc<RwLock<HashMap<String, JobHandler>>>,
    worker_tx: mpsc::Sender<JobInvocation>,
    modification_lock: Arc<tokio::sync::Mutex<()>>,
}

impl SchedulerRuntime {
    pub async fn new(worker_tx: mpsc::Sender<JobInvocation>) -> Result<Self, SchedulerError> {
        let scheduler = JobScheduler::new().await?;
        Ok(Self {
            scheduler: Arc::new(Mutex::new(scheduler)),
            jobs: Arc::new(RwLock::new(HashMap::new())),
            handlers: Arc::new(RwLock::new(HashMap::new())),
            worker_tx,
            modification_lock: Arc::new(Mutex::new(())),
        })
    }

    pub(crate) async fn modification_lock(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.modification_lock.lock().await
    }

    pub async fn start(&self) -> Result<(), SchedulerError> {
        let scheduler = self.scheduler.lock().await;
        scheduler.start().await?;
        Ok(())
    }

    pub async fn stop(&self) -> Result<(), SchedulerError> {
        let mut scheduler = self.scheduler.lock().await;
        scheduler.shutdown().await?;
        Ok(())
    }

    pub async fn register_handler(&self, name: &str, handler: JobHandler) {
        let mut handlers = self.handlers.write().await;
        handlers.insert(name.to_string(), handler);
    }

    pub async fn get_handler(&self, name: &str) -> Option<JobHandler> {
        let handlers = self.handlers.read().await;
        handlers.get(name).cloned()
    }

    pub async fn load_from_db<R: CronJobRepository>(&self, repo: &R) -> Result<(), SchedulerError> {
        let configs = repo.list_active().await?;
        let now = Utc::now();

        let mut skipped: Vec<String> = Vec::new();
        for config in configs {
            let handler = match self.get_handler(&config.name).await {
                Some(h) => h,
                None => {
                    skipped.push(config.name);
                    continue;
                }
            };

            // Only reset @every schedules for jobs that will actually be loaded
            // into the scheduler; a job is skipped when its interval cannot be
            // represented.
            if !reset_every_schedule(repo, &config, now).await? {
                continue;
            }

            let definition = JobDefinition::from(&config);
            if let Err(e) = self.add_job_internal(&definition, handler).await {
                tracing::error!("Failed to load job '{}': {}", config.name, e);
                continue;
            }

            skip_missed_run(repo, &config, now).await;
        }

        if !skipped.is_empty() {
            tracing::warn!(
                "Skipped {} cron jobs because their handlers are not registered: {:?}",
                skipped.len(),
                skipped
            );
        }

        Ok(())
    }

    pub async fn add_job<R: CronJobRepository>(
        &self,
        repo: &R,
        job: &JobDefinition,
        handler: JobHandler,
    ) -> Result<(), SchedulerError> {
        self.add_job_internal(job, handler).await?;
        if let Err(e) = repo.insert(job).await {
            tracing::error!("Failed to insert job '{}' into DB: {}", job.name, e);
            if let Err(rollback_err) = self.remove_job_from_scheduler(&job.name).await {
                tracing::error!(
                    "Failed to rollback scheduler add for '{}': {}",
                    job.name,
                    rollback_err
                );
            }
            return Err(e.into());
        }
        Ok(())
    }

    pub async fn run_job_now(&self, name: &str) -> Result<(), SchedulerError> {
        let entry = {
            let jobs = self.jobs.read().await;
            jobs.get(name)
                .ok_or_else(|| SchedulerError::JobNotFound(name.to_string()))?
                .clone()
        };
        let invocation = JobInvocation {
            name: entry.name.clone(),
            expression: entry.expression.clone(),
            handler: entry.handler.clone(),
            scheduled_at: Utc::now(),
        };
        self.worker_tx
            .send(invocation)
            .await
            .map_err(|_| SchedulerError::WorkerChannelClosed(name.to_string()))?;
        Ok(())
    }

    /// Returns whether a job with this name is currently loaded in the
    /// in-memory map (regardless of enabled state). Jobs skipped at load time
    /// because no handler is registered are not present.
    pub async fn has_job(&self, name: &str) -> bool {
        let jobs = self.jobs.read().await;
        jobs.contains_key(name)
    }

    pub async fn set_enabled<R: CronJobRepository>(
        &self,
        repo: &R,
        name: &str,
        enabled: bool,
    ) -> Result<(), SchedulerError> {
        let entry = {
            let jobs = self.jobs.read().await;
            jobs.get(name)
                .ok_or_else(|| SchedulerError::JobNotFound(name.to_string()))?
                .clone()
        };

        if entry.enabled == enabled {
            // Nothing to change in the scheduler; still persist so the DB
            // stays the source of truth.
            repo.set_enabled(name, enabled).await?;
            return Ok(());
        }

        // Persist first so the in-memory change can be rolled back on failure.
        repo.set_enabled(name, enabled).await?;

        if enabled {
            // Re-create the job in the scheduler. For @every jobs this also
            // restarts the interval from now, matching the next_run_at
            // computed by the API layer.
            let mut definition = JobDefinition::from(&entry);
            definition.enabled = true;
            if let Err(e) = self
                .add_job_internal(&definition, entry.handler.clone())
                .await
            {
                if let Err(rollback_err) = repo.set_enabled(name, false).await {
                    tracing::error!(
                        "Failed to rollback DB set_enabled for '{}': {}",
                        name,
                        rollback_err
                    );
                }
                return Err(e);
            }
        } else {
            // Disabling must remove the job from the scheduler; pausing via
            // set_stop() does not prevent it from firing.
            {
                let scheduler = self.scheduler.lock().await;
                if let Err(e) = scheduler.remove(&entry.job.guid()).await {
                    if let Err(rollback_err) = repo.set_enabled(name, true).await {
                        tracing::error!(
                            "Failed to rollback DB set_enabled for '{}': {}",
                            name,
                            rollback_err
                        );
                    }
                    return Err(e.into());
                }
            }
            let mut jobs = self.jobs.write().await;
            if let Some(entry) = jobs.get_mut(name) {
                entry.enabled = false;
            }
        }

        Ok(())
    }

    /// Updates the in-memory scheduler state for an existing job to match the
    /// provided definition. This method does **not** touch the database;
    /// callers should persist changes first and then use this method to keep
    /// the running scheduler in sync.
    ///
    /// The `name` field of `job` is ignored; the `name` argument takes
    /// precedence.
    ///
    /// When only metadata (title/description/group) changes, the update is
    /// applied in place. When the expression or the enabled flag changes, the
    /// underlying tokio-cron-scheduler job is removed and recreated, because
    /// the library can neither reschedule nor reliably pause an existing job
    /// (its set_stop() does not prevent firing). A recreated disabled job is
    /// kept in the in-memory map but not added to the scheduler.
    pub async fn update_job_in_memory(
        &self,
        name: &str,
        job: &JobDefinition,
    ) -> Result<(), SchedulerError> {
        let entry = {
            let jobs = self.jobs.read().await;
            jobs.get(name)
                .ok_or_else(|| SchedulerError::JobNotFound(name.to_string()))?
                .clone()
        };

        // Metadata-only update: nothing in the scheduler needs to change.
        if entry.expression == job.expression && entry.enabled == job.enabled {
            let mut jobs = self.jobs.write().await;
            let entry = jobs
                .get_mut(name)
                .ok_or_else(|| SchedulerError::JobNotFound(name.to_string()))?;
            entry.title = job.title.clone();
            entry.description = job.description.clone();
            entry.group = job.group.clone();
            return Ok(());
        }

        let handler = entry.handler.clone();
        let new_definition = JobDefinition {
            name: name.to_string(),
            ..job.clone()
        };
        let original_definition = JobDefinition::from(&entry);

        self.remove_job_from_scheduler(name).await?;

        if let Err(e) = self
            .add_job_internal(&new_definition, handler.clone())
            .await
        {
            tracing::error!(
                "Failed to add updated job '{}' to scheduler during in-memory update: {}",
                name,
                e
            );
            if let Err(rollback_err) = self.add_job_internal(&original_definition, handler).await {
                tracing::error!(
                    "Failed to restore original job during in-memory update rollback for '{}': {}",
                    name,
                    rollback_err
                );
            }
            return Err(e);
        }

        Ok(())
    }

    pub async fn soft_delete_job<R: CronJobRepository>(
        &self,
        repo: &R,
        name: &str,
    ) -> Result<(), SchedulerError> {
        let original_enabled = {
            let jobs = self.jobs.read().await;
            let entry = jobs
                .get(name)
                .ok_or_else(|| SchedulerError::JobNotFound(name.to_string()))?;
            entry.enabled
        };

        repo.soft_delete(name).await?;

        if let Err(e) = self.remove_job_from_scheduler(name).await {
            if let Err(rollback_err) = repo.restore(name, original_enabled).await {
                tracing::error!(
                    "Failed to rollback DB soft_delete for '{}': {}",
                    name,
                    rollback_err
                );
            }
            return Err(e);
        }

        Ok(())
    }

    pub async fn list_jobs(&self) -> Vec<JobInfo> {
        let jobs = self.jobs.read().await;
        jobs.values()
            .map(|e| JobInfo {
                name: e.name.clone(),
                title: e.title.clone(),
                description: e.description.clone(),
                expression: e.expression.clone(),
                enabled: e.enabled,
                last_run_at: None,
                next_run_at: None,
                updated_at: None,
                group: e.group.clone(),
                frequency_secs: compute_frequency_secs(&e.expression),
            })
            .collect()
    }

    pub async fn list_jobs_detailed<R: CronJobRepository>(
        &self,
        repo: &R,
    ) -> Result<Vec<JobInfo>, SchedulerError> {
        let mut result = self.list_jobs().await;
        let names: Vec<String> = result.iter().map(|j| j.name.clone()).collect();
        let models = repo.list_by_names(&names).await?;
        let model_map: HashMap<String, cron_job::Model> =
            models.into_iter().map(|m| (m.name.clone(), m)).collect();

        for job in &mut result {
            if let Some(model) = model_map.get(&job.name) {
                job.last_run_at = Some(model.last_run_at);
                job.next_run_at = Some(model.next_run_at);
                job.updated_at = Some(model.updated_at);
            }
        }

        Ok(result)
    }

    async fn add_job_internal(
        &self,
        job: &JobDefinition,
        handler: JobHandler,
    ) -> Result<(), SchedulerError> {
        let schedule = parse_expression(&job.expression).map_err(SchedulerError::ParseError)?;

        let invocation = JobInvocation {
            name: job.name.clone(),
            expression: job.expression.clone(),
            handler: handler.clone(),
            scheduled_at: Utc::now(),
        };

        let tx = self.worker_tx.clone();
        let wrapped = move |_uuid, _l| -> Pin<Box<dyn Future<Output = ()> + Send>> {
            let tx = tx.clone();
            let mut inv = invocation.clone();
            let name = invocation.name.clone();
            Box::pin(async move {
                inv.scheduled_at = Utc::now();
                // The scheduler job closure returns (), so we cannot propagate
                // WorkerChannelClosed here; log the error instead.
                if let Err(e) = tx.send(inv).await {
                    tracing::error!("Failed to dispatch job '{}': {}", name, e);
                }
            })
        };

        let scheduler_job = match schedule {
            ScheduleType::Cron(cron_expr) => {
                // Interpret cron expressions in the server's local timezone so
                // they match compute_next_run and the frontend's local-time
                // display. Note: tokio-cron-scheduler snapshots the UTC offset
                // at creation time, so in regions with DST a job keeps the
                // offset from its creation until it is recreated.
                Job::new_async_tz(cron_expr, chrono::Local, wrapped)?
            }
            ScheduleType::Every(duration) => Job::new_repeated_async(duration, wrapped)?,
        };

        // Disabled jobs are kept in the in-memory map (so they stay listed and
        // can be triggered manually) but are NOT added to the scheduler:
        // tokio-cron-scheduler's set_stop() does not actually prevent a job
        // from firing, so removal is the only reliable way to disable.
        if job.enabled {
            let scheduler = self.scheduler.lock().await;
            scheduler.add(scheduler_job.clone()).await?;
        }

        {
            let mut jobs = self.jobs.write().await;
            jobs.insert(
                job.name.clone(),
                JobEntry {
                    name: job.name.clone(),
                    title: job.title.clone(),
                    description: job.description.clone(),
                    expression: job.expression.clone(),
                    job: scheduler_job,
                    enabled: job.enabled,
                    group: job.group.clone(),
                    handler,
                },
            );
        }

        Ok(())
    }

    async fn remove_job_from_scheduler(
        &self,
        name: &str,
    ) -> Result<Option<JobEntry>, SchedulerError> {
        let entry = {
            let mut jobs = self.jobs.write().await;
            jobs.remove(name)
        };
        if let Some(ref entry) = entry {
            // Disabled jobs were never added to the scheduler; skipping the
            // removal also avoids a spurious not-found error from it.
            if entry.enabled {
                let scheduler = self.scheduler.lock().await;
                if let Err(e) = scheduler.remove(&entry.job.guid()).await {
                    // Restore the entry to the in-memory map so the map stays in
                    // sync with the scheduler when removal fails.
                    let mut jobs = self.jobs.write().await;
                    jobs.insert(name.to_string(), entry.clone());
                    return Err(e.into());
                }
            }
        }
        Ok(entry)
    }
}

/// Resets `next_run_at` for an `@every` job so its interval restarts from the
/// current time: a restart never makes up the elapsed part of an interval.
///
/// Returns `Ok(true)` when loading may proceed — including for non-`@every`
/// expressions, which are left untouched — and `Ok(false)` when the job must
/// be skipped because its interval is too large to represent.
async fn reset_every_schedule<R: CronJobRepository>(
    repo: &R,
    config: &cron_job::Model,
    now: chrono::DateTime<Utc>,
) -> Result<bool, SchedulerError> {
    let Ok(ScheduleType::Every(duration)) = parse_expression(&config.expression) else {
        return Ok(true);
    };
    let Some(secs) = i64::try_from(duration.as_secs()).ok() else {
        tracing::warn!(
            "duration too large to reset next_run_at for job '{}'",
            config.name
        );
        return Ok(false);
    };
    let Some(delta) = chrono::TimeDelta::try_seconds(secs) else {
        tracing::warn!(
            "duration too large to reset next_run_at for job '{}'",
            config.name
        );
        return Ok(false);
    };
    let Some(new_next_run_at) = now.checked_add_signed(delta) else {
        tracing::warn!("next_run_at overflow when resetting job '{}'", config.name);
        return Ok(false);
    };
    repo.update_run_times(&config.name, config.last_run_at, new_next_run_at)
        .await?;
    Ok(true)
}

/// Skips a missed cron run: when an enabled cron job's `next_run_at` lies in
/// the past without a matching `last_run_at`, recompute it to the next future
/// run instead of firing on startup. `@every` jobs already had their schedule
/// reset by [`reset_every_schedule`].
async fn skip_missed_run<R: CronJobRepository>(
    repo: &R,
    config: &cron_job::Model,
    now: chrono::DateTime<Utc>,
) {
    if !config.enabled || config.next_run_at >= now || config.last_run_at > config.next_run_at {
        return;
    }
    // @every jobs already had next_run_at reset above, and unparsable
    // expressions have no schedule to recompute.
    let Ok(ScheduleType::Cron(_)) = parse_expression(&config.expression) else {
        return;
    };
    let next = compute_next_run(&config.expression).unwrap_or(config.next_run_at);
    tracing::info!(
        "Job '{}' missed its scheduled run (next_run_at={}), skipping to next run at {}",
        config.name,
        config.next_run_at,
        next
    );
    if let Err(e) = repo
        .update_run_times(&config.name, config.last_run_at, next)
        .await
    {
        tracing::error!(
            "Failed to update next_run_at for missed job '{}': {}",
            config.name,
            e
        );
    }
}

#[cfg(test)]
mod tests;

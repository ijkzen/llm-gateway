//! Shared bootstrap helpers for the HTTP integration tests.

use llm_gateway::cron::log_capture::JobLogEvent;
use llm_gateway::cron::scheduler::SchedulerRuntime;
use llm_gateway::cron::worker::JobWorker;
use llm_gateway::db;
use llm_gateway::routes;
use llm_gateway::state::AppState;

/// Creates an in-memory database, starts a job worker, and builds the
/// scheduler on top of it. The scheduler is returned unstarted so each test
/// can register handlers and seed data before calling `start()`.
pub async fn setup_db_and_scheduler(
) -> (
    sea_orm::DatabaseConnection,
    SchedulerRuntime,
    tokio::sync::broadcast::Sender<JobLogEvent>,
) {
    let db = db::connect("sqlite::memory:").await.unwrap();

    let (log_tx, _) = tokio::sync::broadcast::channel::<JobLogEvent>(64);
    let worker = JobWorker::new(db.clone(), 2, 100, log_tx.clone());
    let handle = worker.start();

    let scheduler = SchedulerRuntime::new(handle.tx.clone()).await.unwrap();
    (db, scheduler, log_tx)
}

/// Builds the Axum app with the given database and scheduler as state.
pub fn build_app(
    db: sea_orm::DatabaseConnection,
    scheduler: SchedulerRuntime,
    log_tx: tokio::sync::broadcast::Sender<JobLogEvent>,
) -> axum::Router {
    let state = AppState {
        db,
        scheduler,
        log_tx,
    };
    routes::create_app().with_state(state)
}

//! Shared helpers for the cron module's unit tests.

use sea_orm::{Database, DatabaseConnection};

use crate::cron::repository::JobDefinition;

/// Connects to a fresh in-memory SQLite database and runs migrations.
pub(crate) async fn setup_db() -> DatabaseConnection {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    crate::db::migrate(&db).await.unwrap();
    db
}

/// Returns a job definition with the default fields used across cron tests.
pub(crate) fn sample_job(name: &str) -> JobDefinition {
    JobDefinition {
        name: name.to_string(),
        title: "Test".to_string(),
        description: "".to_string(),
        expression: "@hourly".to_string(),
        enabled: true,
        group: "default".to_string(),
    }
}

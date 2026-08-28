use std::time::{Duration, SystemTime};
use tokio::fs;
use tokio::time::interval;

pub async fn cleanup_old_logs(log_dir: &str, keep_days: u64) -> std::io::Result<()> {
    let mut entries = fs::read_dir(log_dir).await?;
    let now = SystemTime::now();
    let cutoff = Duration::from_secs(keep_days * 24 * 60 * 60);

    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        let metadata = match entry.metadata().await {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("Failed to read metadata for {}: {}", path.display(), e);
                continue;
            }
        };
        if metadata.is_file()
            && let Ok(modified) = metadata.modified()
            && let Ok(age) = now.duration_since(modified)
            && age > cutoff
            && let Err(e) = fs::remove_file(&path).await
        {
            tracing::error!("Failed to remove old log file {}: {}", path.display(), e);
        }
    }
    Ok(())
}

pub fn spawn_cleanup_task(log_dir: String, keep_days: u64) {
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(24 * 60 * 60));
        loop {
            ticker.tick().await;
            if let Err(e) = cleanup_old_logs(&log_dir, keep_days).await {
                tracing::warn!("Scheduled log cleanup failed: {}", e);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_cleanup_old_logs_keeps_recent_files() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("app.log");
        {
            let mut f = std::fs::File::create(&file).unwrap();
            f.write_all(b"log").unwrap();
        }

        cleanup_old_logs(dir.path().to_str().unwrap(), 30)
            .await
            .unwrap();

        assert!(file.exists());
    }

    #[tokio::test]
    async fn test_cleanup_old_logs_empty_dir_ok() {
        let dir = TempDir::new().unwrap();
        cleanup_old_logs(dir.path().to_str().unwrap(), 30)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_cleanup_old_logs_nonexistent_dir_returns_error() {
        let result = cleanup_old_logs("/nonexistent/path/that/does/not/exist", 30).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_cleanup_old_logs_removes_old_files() {
        use std::fs::{File, FileTimes};

        let dir = TempDir::new().unwrap();
        let old_file = dir.path().join("old.log");
        {
            let mut file = File::options()
                .create(true)
                .write(true)
                .truncate(false)
                .open(&old_file)
                .unwrap();
            file.write_all(b"old log").unwrap();

            let old_modified = SystemTime::now() - Duration::from_secs(31 * 24 * 60 * 60);
            let times = FileTimes::new().set_modified(old_modified);
            file.set_times(times).unwrap();
        }

        cleanup_old_logs(dir.path().to_str().unwrap(), 30)
            .await
            .unwrap();

        assert!(!old_file.exists());
    }
}

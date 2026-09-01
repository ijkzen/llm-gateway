use sea_orm::{ConnectOptions, Database, DatabaseConnection, DbErr, Schema, Statement};
use std::path::Path;
use std::time::Duration;

const SLOW_QUERY_THRESHOLD_MS: u64 = 100;

/// Extracts the filesystem path from a SQLite URL for directory creation.
///
/// sqlx URL conventions: `sqlite::memory:` (no file), `sqlite://rel/path.db`
/// (relative), `sqlite:///abs/path.db` (absolute), `sqlite:plain.db` (relative).
/// Returns None for in-memory databases and non-path URLs.
fn sqlite_url_path(database_url: &str) -> Option<String> {
    let rest = database_url.strip_prefix("sqlite:")?;
    let rest = rest.split('?').next().unwrap_or(rest);
    if rest.is_empty() || rest == ":memory:" {
        return None;
    }
    // "///abs/path" → "/abs/path"; "//rel/path" → "rel/path"; "/x" or "x" → "x".
    if let Some(abs) = rest.strip_prefix("///") {
        return Some(format!("/{abs}"));
    }
    let rel = rest.strip_prefix("//").unwrap_or(rest);
    let rel = rel.strip_prefix('/').unwrap_or(rel);
    if rel.is_empty() {
        None
    } else {
        Some(rel.to_string())
    }
}

async fn ensure_sqlite_dir(database_url: &str) -> Result<(), std::io::Error> {
    if let Some(path) = sqlite_url_path(database_url)
        && let Some(parent) = Path::new(&path).parent()
        && !parent.as_os_str().is_empty()
    {
        tokio::fs::create_dir_all(parent).await.map_err(|e| {
            tracing::error!(
                "Failed to create database directory '{}': {}",
                parent.display(),
                e
            );
            e
        })?;
    }
    Ok(())
}

pub async fn connect(database_url: &str) -> Result<DatabaseConnection, DbErr> {
    ensure_sqlite_dir(database_url)
        .await
        .map_err(|e| DbErr::Custom(format!("Failed to create database directory: {e}")))?;

    let mut opt = ConnectOptions::new(database_url.to_owned());

    opt.max_connections(5)
        .min_connections(1)
        .connect_timeout(Duration::from_secs(8))
        .acquire_timeout(Duration::from_secs(8))
        .idle_timeout(Duration::from_secs(60))
        .max_lifetime(Duration::from_secs(3600))
        .sqlx_logging(true)
        .sqlx_slow_statements_logging_settings(
            tracing::log::LevelFilter::Warn,
            Duration::from_millis(SLOW_QUERY_THRESHOLD_MS),
        );

    if database_url.starts_with("sqlite:") {
        use sea_orm::sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous};

        opt.map_sqlx_sqlite_opts(|opts: SqliteConnectOptions| {
            opts.journal_mode(SqliteJournalMode::Wal)
                .synchronous(SqliteSynchronous::Normal)
                // SQLite 写操作是串行的，过长的 busy_timeout 会掩盖锁竞争。
                .busy_timeout(Duration::from_secs(5))
                .foreign_keys(true)
                // 约 256 MB 页缓存，提升读性能。
                .pragma("cache_size", "-64000")
                // 临时表/排序全部走内存。
                .pragma("temp_store", "2")
                // 限制 WAL/回滚日志文件大小不超过 64 MB。
                .pragma("journal_size_limit", "67108864")
                // WAL 自动检查点阈值（页数），默认即 1000，显式声明便于维护。
                .pragma("wal_autocheckpoint", "1000")
                // 内存映射 I/O，读多场景可降低系统调用开销。
                .pragma("mmap_size", "268435456")
        });
    }

    let db = Database::connect(opt).await?;

    let changed = migrate(&db).await?;
    if changed {
        use sea_orm::ConnectionTrait;
        db.execute_unprepared("ANALYZE;").await?;
    }

    Ok(db)
}

pub(crate) async fn migrate(db: &DatabaseConnection) -> Result<bool, DbErr> {
    use crate::entity::{
        api_key, cron_job, cron_job_log, cron_job_run, provider, provider_model, provider_template,
        request, session, setting, usage_cache, user, virtual_model, virtual_model_item,
    };
    use sea_orm::ConnectionTrait;

    let backend = db.get_database_backend();

    let mut stmt = Schema::new(backend).create_table_from_entity(cron_job::Entity);
    stmt.if_not_exists();
    db.execute(&stmt).await?;

    let mut stmt = Schema::new(backend).create_table_from_entity(setting::Entity);
    stmt.if_not_exists();
    db.execute(&stmt).await?;

    let mut stmt = Schema::new(backend).create_table_from_entity(cron_job_run::Entity);
    stmt.if_not_exists();
    db.execute(&stmt).await?;

    let mut stmt = Schema::new(backend).create_table_from_entity(cron_job_log::Entity);
    stmt.if_not_exists();
    db.execute(&stmt).await?;

    let mut stmt = Schema::new(backend).create_table_from_entity(provider_template::Entity);
    stmt.if_not_exists();
    db.execute(&stmt).await?;

    let mut stmt = Schema::new(backend).create_table_from_entity(provider::Entity);
    stmt.if_not_exists();
    db.execute(&stmt).await?;

    let mut stmt = Schema::new(backend).create_table_from_entity(provider_model::Entity);
    stmt.if_not_exists();
    db.execute(&stmt).await?;

    let mut stmt = Schema::new(backend).create_table_from_entity(virtual_model::Entity);
    stmt.if_not_exists();
    db.execute(&stmt).await?;

    let mut stmt = Schema::new(backend).create_table_from_entity(virtual_model_item::Entity);
    stmt.if_not_exists();
    db.execute(&stmt).await?;

    let mut stmt = Schema::new(backend).create_table_from_entity(api_key::Entity);
    stmt.if_not_exists();
    db.execute(&stmt).await?;

    let mut stmt = Schema::new(backend).create_table_from_entity(user::Entity);
    stmt.if_not_exists();
    db.execute(&stmt).await?;

    let mut stmt = Schema::new(backend).create_table_from_entity(session::Entity);
    stmt.if_not_exists();
    db.execute(&stmt).await?;

    let mut stmt = Schema::new(backend).create_table_from_entity(request::Entity);
    stmt.if_not_exists();
    db.execute(&stmt).await?;

    let mut stmt = Schema::new(backend).create_table_from_entity(usage_cache::Entity);
    stmt.if_not_exists();
    db.execute(&stmt).await?;

    db.execute_unprepared(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL
        )",
    )
    .await?;

    let mut changed = false;

    let group_exists = column_exists(db, "cron_jobs", "group").await?;
    let is_deleted_exists = column_exists(db, "cron_jobs", "is_deleted").await?;

    let mut migration_1_statements: Vec<&str> = Vec::new();
    if !group_exists {
        migration_1_statements
            .push("ALTER TABLE cron_jobs ADD COLUMN \"group\" TEXT NOT NULL DEFAULT 'other'");
    }
    if !is_deleted_exists {
        migration_1_statements
            .push("ALTER TABLE cron_jobs ADD COLUMN \"is_deleted\" BOOLEAN NOT NULL DEFAULT 0");
    }

    if !migration_1_statements.is_empty() {
        ensure_migration(db, 1, &migration_1_statements).await?;
        changed = true;
    } else {
        // Columns already exist; record version 1 without re-running ALTER.
        ensure_migration(db, 1, &["SELECT 1"]).await?;
    }

    // Migration 2 originally created a redundant non-unique index on `name`.
    // It is now a placeholder so existing databases skip it; migration 3 drops
    // the index because `name` already has a unique constraint.
    changed |= ensure_migration(db, 2, &["SELECT 1"]).await?;

    changed |= ensure_migration(db, 3, &["DROP INDEX IF EXISTS idx_cron_jobs_name"]).await?;

    // Migration 4: 定时任务执行日志（runs + logs）的查询索引。
    changed |= ensure_migration(
        db,
        4,
        &[
            "CREATE INDEX IF NOT EXISTS idx_cron_job_runs_job_name ON cron_job_runs (job_name)",
            "CREATE INDEX IF NOT EXISTS idx_cron_job_logs_run_id ON cron_job_logs (run_id)",
        ],
    )
    .await?;

    // Migration 5: 供应商模型的供应商索引与 (provider_id, provider_model_id) 复合唯一约束。
    changed |= ensure_migration(
        db,
        5,
        &[
            "CREATE INDEX IF NOT EXISTS idx_provider_models_provider_id ON provider_model (provider_id)",
            "CREATE UNIQUE INDEX IF NOT EXISTS uq_provider_models_provider_model_id ON provider_model (provider_id, provider_model_id)",
        ],
    )
    .await?;

    // Migration 6: 虚拟模型成员的全局唯一约束（一个供应商模型最多归属一个虚拟模型）
    // 与按虚拟模型查成员的索引。
    changed |= ensure_migration(
        db,
        6,
        &[
            "CREATE INDEX IF NOT EXISTS idx_virtual_model_items_virtual_model_id ON virtual_model_item (virtual_model_id)",
            "CREATE UNIQUE INDEX IF NOT EXISTS uq_virtual_model_items_model_id ON virtual_model_item (model_id)",
        ],
    )
    .await?;

    // Migration 7: 登录认证与请求指标。
    // - api_key.key_hash：明文密钥的 SHA-256 摘要，供 /v1 Bearer 鉴权 O(1) 查找
    //   （数据回填由 auth::backfill_api_key_hashes 在启动时完成）。
    //   新建的 api_key 表已由实体携带该列，因此仅对历史库执行 ALTER。
    // - request 表查询索引与会话过期清理索引。
    let key_hash_exists = column_exists(db, "api_key", "key_hash").await?;
    let mut migration_7_statements: Vec<&str> = Vec::new();
    if !key_hash_exists {
        migration_7_statements.push("ALTER TABLE api_key ADD COLUMN key_hash TEXT");
    }
    migration_7_statements.extend([
        "CREATE INDEX IF NOT EXISTS idx_api_key_key_hash ON api_key (key_hash)",
        "CREATE INDEX IF NOT EXISTS idx_request_start_time ON request (start_time)",
        "CREATE INDEX IF NOT EXISTS idx_request_virtual_model_id ON request (virtual_model_id)",
        "CREATE INDEX IF NOT EXISTS idx_request_provider_id ON request (provider_id)",
        "CREATE INDEX IF NOT EXISTS idx_session_expires_at ON session (expires_at)",
    ]);
    changed |= ensure_migration(db, 7, &migration_7_statements).await?;

    // Migration 8: 请求日志查询索引（按 API Key 名称过滤加速）。
    changed |= ensure_migration(
        db,
        8,
        &["CREATE INDEX IF NOT EXISTS idx_request_api_key_name ON request (api_key_name)"],
    )
    .await?;

    // Migration 9: 供应商用量数据库缓存表（provider_usage_cache）的供应商唯一索引。
    // 新库已由第一遍 create_table_from_entity 建表并带 UNIQUE 约束，此处兜底历史库。
    changed |= ensure_migration(
        db,
        9,
        &["CREATE UNIQUE INDEX IF NOT EXISTS idx_provider_usage_cache_provider ON provider_usage_cache (provider_id)"],
    )
    .await?;

    // Migration 10: 删除 request.network_latency（建连耗时并入 ttft，见
    // entity::request 口径）；重建 start_time 索引（历史库的旧索引在 DROP
    // COLUMN 时可能失效）并新增 ttft/tps 排序索引（新指标口径的查询路径）。
    let network_latency_exists = column_exists(db, "request", "network_latency").await?;
    if network_latency_exists {
        changed |= ensure_migration(
            db,
            10,
            &[
                "ALTER TABLE request DROP COLUMN network_latency",
                "DROP INDEX IF EXISTS idx_request_start_time",
                "CREATE INDEX idx_request_start_time ON request (start_time)",
                "CREATE INDEX idx_request_ttft ON request (ttft)",
                "CREATE INDEX idx_request_tps ON request (tps)",
            ],
        )
        .await?;
    } else {
        // 新库从未建过该列，仅记录版本。
        changed |= ensure_migration(db, 10, &["SELECT 1"]).await?;
    }

    // Migration 11: provider 列表排序字段（sort_order，越小越靠前）。
    // 新库已由第一遍 create_table_from_entity 建表并带该列，此处兜底历史库。
    let sort_order_exists = column_exists(db, "provider", "sort_order").await?;
    if sort_order_exists {
        changed |= ensure_migration(db, 11, &["SELECT 1"]).await?;
    } else {
        changed |= ensure_migration(
            db,
            11,
            &["ALTER TABLE provider ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0"],
        )
        .await?;
    }

    // Migration 12: 供应商赛马排行查询索引。
    // 赛马/图表/请求日志的查询模式都是「start_time 时间窗口过滤 + 按
    // provider_id 分组（JOIN provider 出名称）」，把 start_time 单列索引升级为
    // (start_time, provider_id, success) 复合索引以覆盖过滤 + 分组；另补
    // success 前置索引服务「只看成功请求」的过滤路径。
    changed |= ensure_migration(
        db,
        12,
        &[
            "DROP INDEX IF EXISTS idx_request_start_time",
            "CREATE INDEX idx_request_start_time ON request (start_time, provider_id, success)",
            "CREATE INDEX idx_request_success_start ON request (success, start_time)",
        ],
    )
    .await?;

    // Migration 13: provider 网络代理字段（proxy_enabled + proxy_addr）。
    // 供应商可单独开启 HTTP 代理转发；新库已由第一遍 create_table_from_entity
    // 建表并带这两列，此处兜底历史库。
    // 注意：两列**分别**检测——历史库可能只有其中一列（早期版本只加了
    // proxy_enabled），不能因一列存在就跳过另一列。
    let proxy_enabled_exists = column_exists(db, "provider", "proxy_enabled").await?;
    if proxy_enabled_exists {
        changed |= ensure_migration(db, 13, &["SELECT 1"]).await?;
    } else {
        changed |= ensure_migration(
            db,
            13,
            &["ALTER TABLE provider ADD COLUMN proxy_enabled boolean NOT NULL DEFAULT '0'"],
        )
        .await?;
    }
    let proxy_addr_exists = column_exists(db, "provider", "proxy_addr").await?;
    if proxy_addr_exists {
        changed |= ensure_migration(db, 13, &["SELECT 1"]).await?;
    } else {
        changed |= ensure_migration(
            db,
            13,
            &["ALTER TABLE provider ADD COLUMN proxy_addr varchar NOT NULL DEFAULT ''"],
        )
        .await?;
    }

    tracing::info!("Database tables migrated");

    Ok(changed)
}

async fn column_exists(db: &DatabaseConnection, table: &str, column: &str) -> Result<bool, DbErr> {
    use sea_orm::ConnectionTrait;

    let rows = db
        .query_all_raw(Statement::from_string(
            db.get_database_backend(),
            format!("PRAGMA table_info({table})"),
        ))
        .await?;
    for row in rows {
        if let Ok(name) = row.try_get::<String>("", "name")
            && name == column
        {
            return Ok(true);
        }
    }
    Ok(false)
}

// Non-idempotent ALTER TABLE statements are acceptable here because the
// in-transaction migration guard prevents concurrent execution, and the
// schema_migrations table is created before any versioned migration runs.
async fn ensure_migration(
    db: &DatabaseConnection,
    version: i32,
    statements: &[&str],
) -> Result<bool, DbErr> {
    use sea_orm::{ConnectionTrait, Statement, TransactionTrait};

    let txn = db.begin().await?;

    let count: i64 = txn
        .query_one_raw(Statement::from_sql_and_values(
            db.get_database_backend(),
            "SELECT COUNT(*) AS c FROM schema_migrations WHERE version = ?",
            [version.into()],
        ))
        .await?
        .map(|row| row.try_get::<i64>("", "c").unwrap_or(0))
        .unwrap_or(0);

    if count == 0 {
        for stmt in statements {
            txn.execute_unprepared(stmt).await?;
        }
        txn.execute_raw(Statement::from_sql_and_values(
            db.get_database_backend(),
            "INSERT INTO schema_migrations (version, applied_at) VALUES (?, datetime('now'))",
            [version.into()],
        ))
        .await?;
        txn.commit().await?;
        Ok(true)
    } else {
        txn.commit().await?;
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sqlite_url_path_relative() {
        assert_eq!(
            sqlite_url_path("sqlite://db/app.db?mode=rwc"),
            Some("db/app.db".to_string())
        );
        assert_eq!(
            sqlite_url_path("sqlite:plain.db"),
            Some("plain.db".to_string())
        );
    }

    #[test]
    fn test_sqlite_url_path_absolute_stays_absolute() {
        // Regression: the absolute prod path must not be turned into a
        // relative path, otherwise the directory is created under the CWD.
        assert_eq!(
            sqlite_url_path("sqlite:///config/db/app.db?mode=rwc"),
            Some("/config/db/app.db".to_string())
        );
    }

    #[test]
    fn test_sqlite_url_path_memory_returns_none() {
        assert_eq!(sqlite_url_path("sqlite::memory:"), None);
        assert_eq!(sqlite_url_path("sqlite:"), None);
    }

    #[tokio::test]
    async fn test_ensure_sqlite_dir_creates_relative_parent() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("sub/dir/app.db");
        let url = format!("sqlite://{}?mode=rwc", db_path.display());
        // The URL above is absolute on disk ("sqlite:///tmp/..."), so the
        // parent must be created at the absolute location.
        ensure_sqlite_dir(&url).await.unwrap();
        assert!(db_path.parent().unwrap().exists());
    }
}

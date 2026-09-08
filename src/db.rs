use sea_orm::{ConnectOptions, Database, DatabaseConnection, DbErr, Schema, Statement};
use std::path::Path;
use std::time::Duration;

const SLOW_QUERY_THRESHOLD_MS: u64 = 100;

/// SQLite 唯一约束冲突。
pub fn is_unique_violation(err: &DbErr) -> bool {
    err.to_string().contains("UNIQUE constraint failed")
}

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
                // cache_size 负值按 KiB 计：-64000 ≈ 62.5 MiB/连接
                // （max_connections 5 全活跃约 0.3GB，SQLite 逐连接生效，
                // 此前注释误算为 256MB）。
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
    // 注意：所有缺失列的 ALTER 必须合并进**单次** ensure_migration 调用——
    // 该函数按版本号去重（同版本第二次调用直接跳过），若分开调用，
    // 第一个调用写入 version 13 后，第二个会被版本守卫吞掉导致列漏加。
    changed |= ensure_columns(
        db,
        13,
        &[
            (
                "provider",
                "proxy_enabled",
                "ALTER TABLE provider ADD COLUMN proxy_enabled boolean NOT NULL DEFAULT '0'",
            ),
            (
                "provider",
                "proxy_addr",
                "ALTER TABLE provider ADD COLUMN proxy_addr varchar NOT NULL DEFAULT ''",
            ),
        ],
    )
    .await?;

    // Migration 16: virtual_model_item.cascade_disabled —— 区分「用户手动关闭」与
    // 「被供应商级联停用」。级联恢复（供应商重新启用/额度恢复）只恢复带该标记的条目，
    // 用户手动关闭的成员保持不变。新库已由第一遍 create_table_from_entity 建表带该列，
    // 此处兜底历史库。
    // 注意：版本号 14/15 已被生产库残留的旧 lg-proxy 方案迁移记录占用（该方案已删除），
    // 新迁移必须从 16 起编，否则 ALTER 会被 ensure_migration 的版本守卫静默吞掉。
    changed |= ensure_columns(
        db,
        16,
        &[(
            "virtual_model_item",
            "cascade_disabled",
            "ALTER TABLE virtual_model_item ADD COLUMN cascade_disabled boolean NOT NULL DEFAULT '0'",
        )],
    )
    .await?;

    // Migration 17: provider.failure_disabled —— 标记「连续失败禁用」来源。
    // 额度门控禁用会在额度恢复后自动重新启用，连续失败禁用只能手动启用解除；
    // 用量定时刷新的恢复分支依据该标记跳过。新库已由建表带该列，此处兜底历史库
    // （含生产残留的 14/15 废弃号段，沿用 column_exists 逐列检测）。
    changed |= ensure_columns(
        db,
        17,
        &[(
            "provider",
            "failure_disabled",
            "ALTER TABLE provider ADD COLUMN failure_disabled boolean NOT NULL DEFAULT '0'",
        )],
    )
    .await?;

    // Migration 18: 移除 provider.status 死字段。该字段自旧 lg-proxy 方案遗留，
    // 全库没有任何写入为 1（不可用）的路径，恒为 0（可用），真实启停语义由
    // enable + failure_disabled 承载，选路过滤也早已不再依赖它。SQLite 从
    // 3.35 起支持 DROP COLUMN；历史库兜底按列存在与否执行。
    let migration_18_statements: Vec<&str> = if column_exists(db, "provider", "status").await? {
        vec!["ALTER TABLE provider DROP COLUMN status"]
    } else {
        Vec::new()
    };
    if migration_18_statements.is_empty() {
        changed |= ensure_migration(db, 18, &["SELECT 1"]).await?;
    } else {
        changed |= ensure_migration(db, 18, &migration_18_statements).await?;
    }

    // Migration 19: provider_model 模型级代理字段（proxy_enabled + proxy_addr）。
    // 模型可单独开启网络代理，优先级高于供应商级代理；新库已由
    // create_table_from_entity 建表带这两列，此处兜底历史库。
    changed |= ensure_columns(
        db,
        19,
        &[
            (
                "provider_model",
                "proxy_enabled",
                "ALTER TABLE provider_model ADD COLUMN proxy_enabled boolean NOT NULL DEFAULT '0'",
            ),
            (
                "provider_model",
                "proxy_addr",
                "ALTER TABLE provider_model ADD COLUMN proxy_addr varchar NOT NULL DEFAULT ''",
            ),
        ],
    )
    .await?;

    // Migration 20: provider_model 模型级协议覆盖字段（protocol_type，可空）。
    // 模型可单独指定上游协议（0..=3，含义与 provider.protocol_type 一致），
    // 为空时回落供应商协议；新库已由 create_table_from_entity 建表带该列，
    // 此处兜底历史库。
    changed |= ensure_columns(
        db,
        20,
        &[(
            "provider_model",
            "protocol_type",
            "ALTER TABLE provider_model ADD COLUMN protocol_type integer",
        )],
    )
    .await?;

    // Migration 21: provider.disabled_reason —— 停用原因四值（NULL=正常启用 /
    // failure=连续失败禁用 / quota=额度耗尽 / manual=手动停用），语义见 ADR-0003；
    // 迁移 22 将删除被取代的 failure_disabled 布尔列。存量回填：failure_disabled=1
    // → 'failure'；enable=0 且非失败禁用 → 'manual'（安全默认：无法区分额度/手动时
    // 宁多一次手动启用，也不被额度刷新自动启用）。新库由实体建表带该列，无需回填。
    let reason_exists = column_exists(db, "provider", "disabled_reason").await?;
    let failure_flag_exists = column_exists(db, "provider", "failure_disabled").await?;
    let mut migration_21_statements: Vec<&str> = Vec::new();
    if !reason_exists {
        migration_21_statements.push("ALTER TABLE provider ADD COLUMN disabled_reason varchar");
    }
    if failure_flag_exists {
        // 回填语句以 failure_disabled 列存在为前提；该列删除后（迁移 22 之后的新库）
        // 仅记录版本号。
        migration_21_statements.extend([
            "UPDATE provider SET disabled_reason = 'failure' WHERE failure_disabled = 1 AND disabled_reason IS NULL",
            "UPDATE provider SET disabled_reason = 'manual' WHERE enable = 0 AND failure_disabled = 0 AND disabled_reason IS NULL",
        ]);
    }
    changed |= ensure_migration(db, 21, &migration_21_statements).await?;

    // Migration 22: 删除 provider.failure_disabled —— 布尔标志已被迁移 21 的
    // disabled_reason 四值停用原因取代（ADR-0003）。SQLite 从 3.35 起支持
    // DROP COLUMN；新库从未建过该列，仅记录版本号。
    let migration_22_statements: Vec<&str> =
        if column_exists(db, "provider", "failure_disabled").await? {
            vec!["ALTER TABLE provider DROP COLUMN failure_disabled"]
        } else {
            Vec::new()
        };
    if migration_22_statements.is_empty() {
        changed |= ensure_migration(db, 22, &["SELECT 1"]).await?;
    } else {
        changed |= ensure_migration(db, 22, &migration_22_statements).await?;
    }

    // Migration 23: virtual_model.interface_type 接口类型（编号与协议类型对齐：
    // 0=OpenAI Compat / 1=Responses / 2=Anthropic Messages / 3=Gemini 保留 /
    // 4=Full Compatible）。存量行回填 4 —— 升级前所有虚拟模型都是「任意协议成员 +
    // chat/completions 转换」语义；新行默认 0 由实体建表/default 提供。
    let mut migration_23_statements: Vec<&str> = Vec::new();
    if !column_exists(db, "virtual_model", "interface_type").await? {
        migration_23_statements.extend([
            "ALTER TABLE virtual_model ADD COLUMN interface_type integer NOT NULL DEFAULT 0",
            "UPDATE virtual_model SET interface_type = 4",
        ]);
    }
    changed |= ensure_migration(db, 23, &migration_23_statements).await?;

    // Migration 24: request 表索引补齐（新库缺 4 条）——
    // - idx_request_ttft / idx_request_tps：原先只建在迁移 10 的老库条件分支内
    //   （为 DROP network_latency 兜底），新库从 0 迁移没有这两条，request_logs
    //   按 ttft/tps 排序退化为整窗 temp sort；老库已有，IF NOT EXISTS 幂等。
    // - (provider_id, model_id, success, start_time)：model_metrics 点查 /
    //   request_logs model_id IN 等值过滤路径（复核修正：整窗 GROUP BY 类不获益）。
    // - (provider_id, success, start_time)：usage_estimate / provider_metrics 的
    //   provider 点查 + success 过滤 + 时间窗截取。
    changed |= ensure_migration(
        db,
        24,
        &[
            "CREATE INDEX IF NOT EXISTS idx_request_ttft ON request (ttft)",
            "CREATE INDEX IF NOT EXISTS idx_request_tps ON request (tps)",
            "CREATE INDEX IF NOT EXISTS idx_request_provider_model_success_start ON request (provider_id, model_id, success, start_time)",
            "CREATE INDEX IF NOT EXISTS idx_request_provider_success_start ON request (provider_id, success, start_time)",
        ],
    )
    .await?;

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

/// 缺失则补列的迁移样板：对 `(table, column)` 逐列检查，缺列时收集其 ADD 语句，
/// 最后以单次 `ensure_migration(version, stmts)` 执行（版本守卫保证同一版本只跑一次）。
/// 无缺列时仍写入版本记录（`SELECT 1`），幂等且不吞后续迁移。
async fn ensure_columns(
    db: &DatabaseConnection,
    version: i32,
    columns: &[(&str, &str, &str)],
) -> Result<bool, DbErr> {
    let mut statements: Vec<&str> = Vec::new();
    for (table, column, add_ddl) in columns {
        if !column_exists(db, table, column).await? {
            statements.push(add_ddl);
        }
    }
    if statements.is_empty() {
        ensure_migration(db, version, &["SELECT 1"]).await
    } else {
        ensure_migration(db, version, &statements).await
    }
}

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
    use sea_orm::ConnectionTrait;

    /// 读取 provider.disabled_reason（按名称），迁移回填断言用。
    async fn disabled_reason(db: &DatabaseConnection, name: &str) -> Option<String> {
        db.query_one_raw(Statement::from_sql_and_values(
            db.get_database_backend(),
            "SELECT disabled_reason AS r FROM provider WHERE name = ?",
            [name.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<Option<String>>("", "r")
        .unwrap()
    }

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

    /// 查询 request 表上的全部索引名。
    async fn request_indexes(db: &DatabaseConnection) -> Vec<String> {
        let rows = db
            .query_all_raw(Statement::from_string(
                db.get_database_backend(),
                "SELECT name FROM sqlite_master WHERE type = 'index' AND tbl_name = 'request'"
                    .to_string(),
            ))
            .await
            .unwrap();
        rows.into_iter()
            .filter_map(|row| row.try_get::<String>("", "name").ok())
            .collect()
    }

    /// 新库从 0 迁移完必须带齐 Migration 24 的四条 request 索引
    /// （回归：ttft/tps 曾只建在迁移 10 老库分支，新库缺索引退化为 temp sort）。
    #[tokio::test]
    async fn migration_24_adds_request_indexes_on_fresh_db() {
        let db = connect("sqlite::memory:").await.unwrap();
        migrate(&db).await.unwrap();

        let indexes = request_indexes(&db).await;
        for expected in [
            "idx_request_ttft",
            "idx_request_tps",
            "idx_request_provider_model_success_start",
            "idx_request_provider_success_start",
        ] {
            assert!(
                indexes.iter().any(|name| name == expected),
                "新库缺索引 {expected}: {indexes:?}"
            );
        }
    }

    /// 老库缺索引时 migrate() 必须补齐（删除版本记录 + 索引后重跑幂等补建）。
    #[tokio::test]
    async fn migration_24_rebuilds_indexes_on_legacy_db() {
        let db = connect("sqlite::memory:").await.unwrap();
        migrate(&db).await.unwrap();
        // 模拟老库：Migration 24 未跑过且四条索引都不存在。
        db.execute_unprepared("DELETE FROM schema_migrations WHERE version = 24")
            .await
            .unwrap();
        for index in [
            "idx_request_ttft",
            "idx_request_tps",
            "idx_request_provider_model_success_start",
            "idx_request_provider_success_start",
        ] {
            db.execute_unprepared(&format!("DROP INDEX IF EXISTS {index}"))
                .await
                .unwrap();
        }

        let changed = migrate(&db).await.unwrap();
        assert!(changed, "migrate 应报告有变更");
        let indexes = request_indexes(&db).await;
        for expected in [
            "idx_request_ttft",
            "idx_request_tps",
            "idx_request_provider_model_success_start",
            "idx_request_provider_success_start",
        ] {
            assert!(
                indexes.iter().any(|name| name == expected),
                "老库补建后仍缺 {expected}: {indexes:?}"
            );
        }
    }

    /// 历史库迁移：provider 表只有 proxy_enabled（缺 proxy_addr），且
    /// schema_migrations 已记录 version 13——migrate() 必须仍补齐 proxy_addr。
    /// 回归：Migration 13 曾分两次 ensure_migration(13, ...) 调用，第二次被
    /// 版本守卫跳过导致 proxy_addr 漏加（合并为单次调用后修复）。
    #[tokio::test]
    async fn migration_13_backfills_missing_proxy_addr_on_legacy_db() {
        let db = connect("sqlite::memory:").await.unwrap();

        // 先完整 migrate 建出全表 + 全部版本记录。
        migrate(&db).await.unwrap();
        // 模拟历史库：删掉 proxy_addr 列 + 移除 version 13 记录（该版本曾执行过）。
        db.execute_unprepared("ALTER TABLE provider DROP COLUMN proxy_addr")
            .await
            .unwrap();
        db.execute_unprepared("DELETE FROM schema_migrations WHERE version = 13")
            .await
            .unwrap();

        // migrate() 必须补齐 proxy_addr。
        let changed = migrate(&db).await.unwrap();
        assert!(changed, "migrate 应报告有变更");

        // 断言两列都在。
        assert!(
            column_exists(&db, "provider", "proxy_enabled")
                .await
                .unwrap()
        );
        assert!(column_exists(&db, "provider", "proxy_addr").await.unwrap());
    }

    /// 历史库迁移：provider 表两列都缺（旧版无任何代理字段）——migrate() 必须补齐两列。
    #[tokio::test]
    async fn migration_13_backfills_both_columns_on_very_old_db() {
        let db = connect("sqlite::memory:").await.unwrap();

        migrate(&db).await.unwrap();
        // 模拟更老的库：两列都删掉 + 移除 version 13 记录。
        db.execute_unprepared("ALTER TABLE provider DROP COLUMN proxy_addr")
            .await
            .unwrap();
        db.execute_unprepared("ALTER TABLE provider DROP COLUMN proxy_enabled")
            .await
            .unwrap();
        db.execute_unprepared("DELETE FROM schema_migrations WHERE version = 13")
            .await
            .unwrap();

        let changed = migrate(&db).await.unwrap();
        assert!(changed);
        assert!(
            column_exists(&db, "provider", "proxy_enabled")
                .await
                .unwrap()
        );
        assert!(column_exists(&db, "provider", "proxy_addr").await.unwrap());
    }

    /// 历史库迁移：schema_migrations 残留废弃号段的版本记录（旧 lg-proxy 方案
    /// 曾占用 14/15），且 virtual_model_item 缺 cascade_disabled 列——migrate()
    /// 必须用 16 号段兜底补列。回归：migration 14 曾与生产残留记录撞号，ALTER
    /// 被版本守卫静默吞掉导致线上缺列。
    #[tokio::test]
    async fn migration_16_backfills_cascade_disabled_despite_stale_versions() {
        let db = connect("sqlite::memory:").await.unwrap();

        migrate(&db).await.unwrap();
        // 模拟生产库：删列 + 注入废弃号段的版本记录（14/15 为旧方案残留）。
        db.execute_unprepared("ALTER TABLE virtual_model_item DROP COLUMN cascade_disabled")
            .await
            .unwrap();
        db.execute_unprepared(
                "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (14, datetime('now')), (15, datetime('now'))",
            )
            .await
            .unwrap();
        db.execute_unprepared("DELETE FROM schema_migrations WHERE version = 16")
            .await
            .unwrap();

        let changed = migrate(&db).await.unwrap();
        assert!(changed, "migrate 应报告有变更");
        assert!(
            column_exists(&db, "virtual_model_item", "cascade_disabled")
                .await
                .unwrap()
        );
    }

    /// 历史库迁移：virtual_model.interface_type 接口类型。存量行回填 4
    /// （Full Compatible —— 升级前全部虚拟模型均为全协议转换语义）。
    #[tokio::test]
    async fn migration_23_backfills_interface_type_on_legacy_db() {
        let db = connect("sqlite::memory:").await.unwrap();

        migrate(&db).await.unwrap();
        // 模拟历史库：删列 + 移除版本记录 + 插入存量行。
        db.execute_unprepared("ALTER TABLE virtual_model DROP COLUMN interface_type")
            .await
            .unwrap();
        db.execute_unprepared("DELETE FROM schema_migrations WHERE version = 23")
            .await
            .unwrap();
        db.execute_unprepared(
            "INSERT INTO virtual_model (display_id, enable, created_at, updated_at) VALUES
                ('legacy-a', 1, datetime('now'), datetime('now')),
                ('legacy-b', 1, datetime('now'), datetime('now'))",
        )
        .await
        .unwrap();

        let changed = migrate(&db).await.unwrap();
        assert!(changed, "migrate 应报告有变更");
        let rows = db
            .query_all_raw(Statement::from_string(
                db.get_database_backend(),
                "SELECT interface_type FROM virtual_model ORDER BY display_id",
            ))
            .await
            .unwrap();
        assert_eq!(rows.len(), 2);
        for row in rows {
            assert_eq!(
                row.try_get::<i32>("", "interface_type").unwrap(),
                4,
                "存量虚拟模型应回填 Full Compatible"
            );
        }
    }

    /// 历史库迁移：provider.disabled_reason 四值停用原因（ADR-0003）。存量回填规则：
    /// failure_disabled=1 → 'failure'；enable=0 且非失败禁用 → 'manual'（安全默认：
    /// 宁多一次手动启用，不被额度刷新自动启用）；启用行保持 NULL。
    #[tokio::test]
    async fn migration_21_backfills_disabled_reason_on_legacy_db() {
        let db = connect("sqlite::memory:").await.unwrap();

        migrate(&db).await.unwrap();
        // 模拟历史库：删 disabled_reason 列 + 加回旧 failure_disabled 列 + 移除
        // 版本记录 + 插入三类存量行。
        db.execute_unprepared("ALTER TABLE provider DROP COLUMN disabled_reason")
            .await
            .unwrap();
        db.execute_unprepared(
            "ALTER TABLE provider ADD COLUMN failure_disabled boolean NOT NULL DEFAULT 0",
        )
        .await
        .unwrap();
        db.execute_unprepared("DELETE FROM schema_migrations WHERE version = 21")
            .await
            .unwrap();
        db.execute_unprepared(
            "INSERT INTO provider (name, base_url, api_key, enable, failure_disabled, created_at, updated_at) VALUES
                ('p-failure', 'http://a', 'k', 0, 1, datetime('now'), datetime('now')),
                ('p-manual', 'http://b', 'k', 0, 0, datetime('now'), datetime('now')),
                ('p-active', 'http://c', 'k', 1, 0, datetime('now'), datetime('now'))",
        )
        .await
        .unwrap();

        let changed = migrate(&db).await.unwrap();
        assert!(changed, "migrate 应报告有变更");
        assert!(
            column_exists(&db, "provider", "disabled_reason")
                .await
                .unwrap()
        );

        assert_eq!(
            disabled_reason(&db, "p-failure").await.as_deref(),
            Some("failure"),
            "连续失败禁用行回填为 failure"
        );
        assert_eq!(
            disabled_reason(&db, "p-manual").await.as_deref(),
            Some("manual"),
            "无法区分来源的禁用行安全回填为 manual"
        );
        assert_eq!(
            disabled_reason(&db, "p-active").await,
            None,
            "启用行保持 NULL（正常）"
        );

        // 再次执行：版本已记录，不报变更（幂等）。
        let changed_again = migrate(&db).await.unwrap();
        assert!(!changed_again, "重复执行不应再报告变更");
    }

    /// 历史库迁移：provider 表残留 failure_disabled 旧列（已被迁移 21 的
    /// disabled_reason 取代）——migrate() 必须 DROP 该列；新库无该列时不重复
    /// 执行（幂等，只记录版本号）。
    #[tokio::test]
    async fn migration_22_drops_stale_failure_disabled() {
        let db = connect("sqlite::memory:").await.unwrap();

        migrate(&db).await.unwrap();
        // 模拟历史库：手动加回旧列 + 移除 22 版本记录。
        db.execute_unprepared(
            "ALTER TABLE provider ADD COLUMN failure_disabled boolean NOT NULL DEFAULT 0",
        )
        .await
        .unwrap();
        db.execute_unprepared("DELETE FROM schema_migrations WHERE version = 22")
            .await
            .unwrap();
        assert!(
            column_exists(&db, "provider", "failure_disabled")
                .await
                .unwrap(),
            "前置：应存在残留 failure_disabled 列"
        );

        let changed = migrate(&db).await.unwrap();
        assert!(changed, "migrate 应报告有变更");
        assert!(
            !column_exists(&db, "provider", "failure_disabled")
                .await
                .unwrap(),
            "failure_disabled 列应被删除"
        );

        let changed_again = migrate(&db).await.unwrap();
        assert!(!changed_again, "重复执行不应再报告变更");
    }

    /// 历史库迁移：provider 表残留 status 死字段（旧 lg-proxy 方案遗留，恒为 0，
    /// 无任何写入为 1 的路径）——migrate() 必须 DROP 该列；已删的库不重复执行
    /// （幂等，只记录版本号）。
    #[tokio::test]
    async fn migration_18_drops_stale_provider_status() {
        let db = connect("sqlite::memory:").await.unwrap();

        migrate(&db).await.unwrap();
        // 模拟历史库：migrate 建表时实体已无 status 列，需手动加回 + 移除 18 版本记录。
        db.execute_unprepared("ALTER TABLE provider ADD COLUMN status INTEGER NOT NULL DEFAULT 0")
            .await
            .unwrap();
        db.execute_unprepared("DELETE FROM schema_migrations WHERE version = 18")
            .await
            .unwrap();
        assert!(
            column_exists(&db, "provider", "status").await.unwrap(),
            "前置：应存在残留 status 列"
        );

        // 首次执行应 DROP 列并报告变更。
        let changed = migrate(&db).await.unwrap();
        assert!(changed, "migrate 应报告有变更");
        assert!(
            !column_exists(&db, "provider", "status").await.unwrap(),
            "status 列应被删除"
        );

        // 再次执行：列已删，只记录版本不报变更（幂等）。
        let changed_again = migrate(&db).await.unwrap();
        assert!(!changed_again, "重复执行不应再报告变更");
    }

    /// 历史库迁移：provider_model 表缺模型级代理字段（旧版无代理列）——
    /// migrate() 必须用 19 号段补齐两列；已补齐的库重复执行幂等（只记录版本号）。
    #[tokio::test]
    async fn migration_19_backfills_provider_model_proxy_columns() {
        let db = connect("sqlite::memory:").await.unwrap();

        migrate(&db).await.unwrap();
        // 模拟历史库：删掉两列 + 移除 version 19 记录。
        db.execute_unprepared("ALTER TABLE provider_model DROP COLUMN proxy_addr")
            .await
            .unwrap();
        db.execute_unprepared("ALTER TABLE provider_model DROP COLUMN proxy_enabled")
            .await
            .unwrap();
        db.execute_unprepared("DELETE FROM schema_migrations WHERE version = 19")
            .await
            .unwrap();

        let changed = migrate(&db).await.unwrap();
        assert!(changed, "migrate 应报告有变更");
        assert!(
            column_exists(&db, "provider_model", "proxy_enabled")
                .await
                .unwrap()
        );
        assert!(
            column_exists(&db, "provider_model", "proxy_addr")
                .await
                .unwrap()
        );

        // 再次执行：列已补，只记录版本不报变更（幂等）。
        let changed_again = migrate(&db).await.unwrap();
        assert!(!changed_again, "重复执行不应再报告变更");
    }

    /// 历史库迁移：schema_migrations 残留废弃号段版本记录（14/15）时，
    /// 新迁移 18 号段（13 之后 + 废弃 14/15 + 16/17 均已占用）不与其撞号，
    /// DROP status 仍会正常执行。
    #[tokio::test]
    async fn migration_18_applies_despite_stale_versions() {
        let db = connect("sqlite::memory:").await.unwrap();

        migrate(&db).await.unwrap();
        // 模拟生产库：手动加回 status 列 + 注入废弃号段记录 + 移除 18。
        db.execute_unprepared("ALTER TABLE provider ADD COLUMN status INTEGER NOT NULL DEFAULT 0")
            .await
            .unwrap();
        db.execute_unprepared(
            "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (14, datetime('now')), (15, datetime('now'))",
        )
        .await
        .unwrap();
        db.execute_unprepared("DELETE FROM schema_migrations WHERE version = 18")
            .await
            .unwrap();

        let changed = migrate(&db).await.unwrap();
        assert!(changed, "migrate 应报告有变更");
        assert!(
            !column_exists(&db, "provider", "status").await.unwrap(),
            "status 列应被删除（即使残留 14/15 号段）"
        );
    }

    /// 历史库迁移：新迁移 19 号段在残留废弃号段版本记录（14/15）存在时，
    /// 仍能正常补齐 provider_model 代理列（不与其撞号）。
    #[tokio::test]
    async fn migration_19_applies_despite_stale_versions() {
        let db = connect("sqlite::memory:").await.unwrap();

        migrate(&db).await.unwrap();
        // 模拟生产库：删列 + 注入废弃号段记录（14/15 为旧方案残留）+ 移除 19。
        db.execute_unprepared("ALTER TABLE provider_model DROP COLUMN proxy_addr")
            .await
            .unwrap();
        db.execute_unprepared("ALTER TABLE provider_model DROP COLUMN proxy_enabled")
            .await
            .unwrap();
        db.execute_unprepared(
            "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (14, datetime('now')), (15, datetime('now'))",
        )
        .await
        .unwrap();
        db.execute_unprepared("DELETE FROM schema_migrations WHERE version = 19")
            .await
            .unwrap();

        let changed = migrate(&db).await.unwrap();
        assert!(changed, "migrate 应报告有变更");
        assert!(
            column_exists(&db, "provider_model", "proxy_enabled")
                .await
                .unwrap()
        );
        assert!(
            column_exists(&db, "provider_model", "proxy_addr")
                .await
                .unwrap()
        );
    }
}

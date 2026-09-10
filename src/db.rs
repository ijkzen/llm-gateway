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
        request, session, setting, snapshot, snapshot_meta, usage_cache, user, virtual_model,
        virtual_model_item,
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

    let mut stmt = Schema::new(backend).create_table_from_entity(snapshot::Entity);
    stmt.if_not_exists();
    db.execute(&stmt).await?;

    let mut stmt = Schema::new(backend).create_table_from_entity(snapshot_meta::Entity);
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
        // 13-02：并入 changed —— 漏掉会让该路径下启动跳过 ANALYZE（查询计划统计不更新）。
        changed |= ensure_migration(db, 1, &["SELECT 1"]).await?;
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

    // Migration 25: cron_job_logs 覆盖索引 (run_id, seq) —— 单 run 日志查询按
    // run_id 过滤 + seq 排序，复合索引直接覆盖；左前缀同时替代原单列
    // idx_cron_job_logs_run_id，故删除后者减少写放大（S6 低优先项）。
    changed |= ensure_migration(
        db,
        25,
        &[
            "CREATE INDEX IF NOT EXISTS idx_cron_job_logs_run_seq ON cron_job_logs (run_id, seq)",
            "DROP INDEX IF EXISTS idx_cron_job_logs_run_id",
        ],
    )
    .await?;

    // Migration 26: 统计快照表（request_log_snapshot + snapshot_meta，ADR-0021）。
    // request_log_snapshot 是 EAV 窄表：同一时间桶内 (duration_type,
    // start_time, entity_type, entity, metric_type) 唯一，UNIQUE 由复合索引
    // 承担（幂等 upsert 以它为前提）。meta 表存生成时区等键值。新库已由第一遍
    // create_table_from_entity 建表，此处 CREATE IF NOT EXISTS 兜底历史库。
    changed |= ensure_migration(
        db,
        26,
        &[
            "CREATE TABLE IF NOT EXISTS request_log_snapshot (\
             id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, \
             duration_type varchar NOT NULL, \
             start_time bigint NOT NULL, \
             end_time bigint NOT NULL, \
             entity_type varchar NOT NULL, \
             entity varchar NOT NULL, \
             metric_type varchar NOT NULL, \
             metric_value real NOT NULL)",
            "CREATE TABLE IF NOT EXISTS snapshot_meta (\
             key varchar PRIMARY KEY NOT NULL, \
             value varchar NOT NULL, \
             updated_at text NOT NULL)",
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_request_log_snapshot_bucket \
             ON request_log_snapshot (duration_type, start_time, entity_type, entity, metric_type)",
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
// in-transaction version guard turns a concurrent second migrator into a
// fail-fast error (DDL rolls back with the transaction; the DB is never left
// half-migrated) rather than silently double-applying. It does NOT serialize
// concurrent first-start migrations -- deployments run a single container at a
// time. The schema_migrations table is created before any versioned migration
// runs.

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
#[path = "db/tests.rs"]
mod tests;

//! db 模块单元测试（13-04：从 db.rs 迁出，保持主链文件低于行数约定）。

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

/// 新库从 0 迁移完必须带齐 Migration 26 的两张快照表与复合唯一索引
/// （幂等 upsert 以该索引为前提；重复行必须被 UNIQUE 拒绝）。
#[tokio::test]
async fn migration_26_creates_snapshot_tables_on_fresh_db() {
    let db = connect("sqlite::memory:").await.unwrap();
    migrate(&db).await.unwrap();

    assert!(
        column_exists(&db, "request_log_snapshot", "metric_value")
            .await
            .unwrap()
    );
    assert!(column_exists(&db, "snapshot_meta", "value").await.unwrap());
    let indexes = db
        .query_all_raw(Statement::from_string(
            db.get_database_backend(),
            "SELECT name FROM sqlite_master WHERE type = 'index' AND name = 'idx_request_log_snapshot_bucket'"
                .to_string(),
        ))
        .await
        .unwrap();
    assert!(!indexes.is_empty(), "快照复合唯一索引缺失");

    // 同桶同主体同指标重复行必须触发 UNIQUE 冲突。
    db.execute_unprepared(
        "INSERT INTO request_log_snapshot (duration_type, start_time, end_time, entity_type, entity, metric_type, metric_value) \
         VALUES ('hour', 1, 2, 'whole', '', 'request_count', 0)",
    )
    .await
    .unwrap();
    let dup = db
        .execute_unprepared(
            "INSERT INTO request_log_snapshot (duration_type, start_time, end_time, entity_type, entity, metric_type, metric_value) \
             VALUES ('hour', 1, 2, 'whole', '', 'request_count', 3)",
        )
        .await;
    assert!(
        dup.is_err() && crate::db::is_unique_violation(&dup.unwrap_err()),
        "重复快照行应报 UNIQUE 冲突"
    );
}

/// 历史库迁移：两表被删 + 版本记录移除后，migrate() 必须重建（幂等兜底）。
#[tokio::test]
async fn migration_26_rebuilds_tables_on_legacy_db() {
    let db = connect("sqlite::memory:").await.unwrap();
    migrate(&db).await.unwrap();

    db.execute_unprepared("DROP TABLE request_log_snapshot")
        .await
        .unwrap();
    db.execute_unprepared("DROP TABLE snapshot_meta")
        .await
        .unwrap();
    db.execute_unprepared("DELETE FROM schema_migrations WHERE version = 26")
        .await
        .unwrap();

    let changed = migrate(&db).await.unwrap();
    assert!(changed, "migrate 应报告有变更");
    assert!(
        column_exists(&db, "request_log_snapshot", "metric_value")
            .await
            .unwrap()
    );
    assert!(column_exists(&db, "snapshot_meta", "key").await.unwrap());
}

/// 13-06（T4）：迁移语句失败时整体回滚，且不写入版本号（下次启动可重试）。
/// 用破坏性 ALTER（对不存在的列 DROP）触发失败。
#[tokio::test]
async fn ensure_migration_rolls_back_and_skips_version_on_failure() {
    let db = connect("sqlite::memory:").await.unwrap();
    // 语句 1 合法（建表）→ 语句 2 非法（DROP 不存在的列）→ 整体应回滚。
    let result = ensure_migration(
        &db,
        9101,
        &[
            "CREATE TABLE rollback_probe (id INTEGER PRIMARY KEY)",
            "ALTER TABLE rollback_probe DROP COLUMN no_such_column",
        ],
    )
    .await;
    assert!(result.is_err(), "非法语句应报错");

    // 表不存在（第一条也被回滚）。
    let count_of = async |sql: &str| -> i64 {
        db.query_one_raw(Statement::from_string(db.get_database_backend(), sql))
            .await
            .unwrap()
            .and_then(|row| row.try_get::<i64>("", "v").ok())
            .unwrap_or(0)
    };
    let table_exists = count_of(
        "SELECT COUNT(*) AS v FROM sqlite_master WHERE type='table' AND name='rollback_probe'",
    )
    .await;
    assert_eq!(table_exists, 0, "失败迁移的第一条语句也应回滚");

    // 版本未记录：下次可重试。
    let version_count =
        count_of("SELECT COUNT(*) AS v FROM schema_migrations WHERE version = 9101").await;
    assert_eq!(version_count, 0, "失败迁移不得记录版本号");
}

/// 13-06：版本行已存在时迁移为 no-op（版本守卫幂等），且返回 changed=false。
#[tokio::test]
async fn ensure_migration_is_noop_when_version_recorded() {
    let db = connect("sqlite::memory:").await.unwrap();
    let first = ensure_migration(&db, 9102, &["SELECT 1"]).await.unwrap();
    assert!(first, "首次应记录为变更");
    let second = ensure_migration(&db, 9102, &["SELECT 1"]).await.unwrap();
    assert!(!second, "已记录版本应 no-op（changed=false）");
}

use llm_gateway::entity::provider;
use sea_orm::{ConnectionTrait, Database, DatabaseConnection, Schema, Statement};

#[tokio::test]
async fn provider_table_ddl_matches_expected() {
    let db: DatabaseConnection = Database::connect("sqlite::memory:").await.unwrap();
    let backend = db.get_database_backend();

    let mut stmt = Schema::new(backend).create_table_from_entity(provider::Entity);
    stmt.if_not_exists();
    db.execute(&stmt).await.unwrap();

    let rows = db
        .query_all_raw(Statement::from_string(
            backend,
            "PRAGMA table_info(provider)",
        ))
        .await
        .unwrap();
    let mut cols: Vec<(String, String, String)> = rows
        .into_iter()
        .map(|r| {
            (
                r.try_get::<String>("", "name").unwrap(),
                r.try_get::<String>("", "type").unwrap(),
                r.try_get::<String>("", "dflt_value").unwrap_or_default(),
            )
        })
        .collect();
    cols.sort_by(|a, b| a.0.cmp(&b.0));

    let expected = vec![
        ("api_key".to_string(), "varchar".to_string(), String::new()),
        ("base_url".to_string(), "varchar".to_string(), String::new()),
        (
            "billing_mode".to_string(),
            "INTEGER".to_string(),
            "'0'".to_string(),
        ),
        (
            "created_at".to_string(),
            "timestamp_with_timezone_text".to_string(),
            String::new(),
        ),
        (
            "custom_header".to_string(),
            "varchar".to_string(),
            "'{}'".to_string(),
        ),
        (
            "disabled_reason".to_string(),
            "varchar".to_string(),
            String::new(),
        ),
        (
            "enable".to_string(),
            "boolean".to_string(),
            "'1'".to_string(),
        ),
        (
            "extra".to_string(),
            "varchar".to_string(),
            "'{}'".to_string(),
        ),
        ("id".to_string(), "INTEGER".to_string(), String::new()),
        ("name".to_string(), "varchar".to_string(), String::new()),
        (
            "protocol_type".to_string(),
            "INTEGER".to_string(),
            "'0'".to_string(),
        ),
        (
            "proxy_addr".to_string(),
            "varchar".to_string(),
            "''".to_string(),
        ),
        (
            "proxy_enabled".to_string(),
            "boolean".to_string(),
            "'0'".to_string(),
        ),
        (
            "sort_order".to_string(),
            "INTEGER".to_string(),
            "'0'".to_string(),
        ),
        (
            "updated_at".to_string(),
            "timestamp_with_timezone_text".to_string(),
            String::new(),
        ),
    ];
    assert_eq!(cols, expected, "provider 表结构不符合预期");
}

/// 13-03：新旧库 schema 的系统性对齐守卫。
///
/// 新库由 `create_table_from_entity` 建表、老库由迁移链 ALTER/CREATE 收敛；两条
/// 路径没有任何系统性守卫（此前只有 provider 一张表逐列比对），双写必然漂移。
/// 本测试对快照两表 + request 表做「列名集合 + affinity」比对：类型名（如
/// `timestamp_with_timezone_text` vs `text`）不要求逐字一致（affinity 相同零行为
/// 差异），但列集合必须完全一致——缺列/多列是真实的功能性漂移。
#[tokio::test]
async fn snapshot_and_request_tables_have_matching_columns_between_paths() {
    use llm_gateway::entity::{request, snapshot, snapshot_meta};

    async fn columns(db: &DatabaseConnection, table: &str) -> Vec<(String, String)> {
        let rows = db
            .query_all_raw(Statement::from_string(
                db.get_database_backend(),
                format!("PRAGMA table_info({table})"),
            ))
            .await
            .unwrap();
        let mut cols: Vec<(String, String)> = rows
            .into_iter()
            .map(|r| {
                (
                    r.try_get::<String>("", "name").unwrap(),
                    r.try_get::<String>("", "type").unwrap().to_uppercase(),
                )
            })
            .collect();
        cols.sort_by(|a, b| a.0.cmp(&b.0));
        cols
    }

    /// SQLite 的 affinity 归类（列类型字符串 → 亲和类），用于容忍类型名漂移。
    fn affinity(decl: &str) -> &'static str {
        let d = decl.to_uppercase();
        if d.contains("INT") {
            "INTEGER"
        } else if d.contains("CHAR") || d.contains("CLOB") || d.contains("TEXT") {
            "TEXT"
        } else if d.contains("BLOB") || d.is_empty() {
            "BLOB"
        } else if d.contains("REAL") || d.contains("FLOA") || d.contains("DOUB") {
            "REAL"
        } else {
            "NUMERIC"
        }
    }

    async fn assert_columns_match(entity_db: &DatabaseConnection, table: &str) {
        // 老库路径：走真实启动路径（`db::connect` 内部执行完整迁移链）。
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("schema.db");
        let migrated: DatabaseConnection =
            llm_gateway::db::connect(&format!("sqlite:///{}?mode=rwc", path.display()))
                .await
                .unwrap();

        let entity_cols = columns(entity_db, table).await;
        let migrated_cols = columns(&migrated, table).await;

        let entity_names: Vec<&String> = entity_cols.iter().map(|(n, _)| n).collect();
        let migrated_names: Vec<&String> = migrated_cols.iter().map(|(n, _)| n).collect();
        assert_eq!(
            entity_names, migrated_names,
            "{table} 列集合在新库（实体建表）与老库（迁移链）之间不一致"
        );
        for ((name, entity_type), (_, migrated_type)) in entity_cols.iter().zip(&migrated_cols) {
            assert_eq!(
                affinity(entity_type),
                affinity(migrated_type),
                "{table}.{name} affinity 不一致：实体={entity_type} 迁移={migrated_type}"
            );
        }
    }

    // 实体路径库：仅建这三张表（对齐比对的参照侧）。
    let entity_db: DatabaseConnection = Database::connect("sqlite::memory:").await.unwrap();
    let backend = entity_db.get_database_backend();
    for stmt in [
        Schema::new(backend).create_table_from_entity(request::Entity),
        Schema::new(backend).create_table_from_entity(snapshot::Entity),
        Schema::new(backend).create_table_from_entity(snapshot_meta::Entity),
    ] {
        entity_db.execute(&stmt).await.unwrap();
    }

    for table in ["request", "request_log_snapshot", "snapshot_meta"] {
        assert_columns_match(&entity_db, table).await;
    }
}

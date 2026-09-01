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
            "status".to_string(),
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

mod common;

use axum::body::Body;
use axum::http::Request;
use sea_orm::{ActiveModelTrait, ConnectionTrait, EntityTrait, Set};
use tower::ServiceExt;

use llm_gateway::entity::setting;

async fn setup_app_with_setting() -> (axum::Router, sea_orm::DatabaseConnection) {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;

    let active = setting::ActiveModel {
        key: Set("site_name".to_string()),
        value: Set("Old Name".to_string()),
        r#type: Set(0),
        updated_at: Set(chrono::Utc::now()),
    };
    active.insert(&db).await.unwrap();

    scheduler.start().await.unwrap();

    let app = common::build_authed_app(db.clone(), scheduler, log_tx).await;
    (app, db)
}

#[tokio::test]
async fn test_list_settings_returns_200() {
    let (app, _db) = setup_app_with_setting().await;
    let request: Request<Body> = Request::builder()
        .method("GET")
        .uri("/api/settings")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), 200);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_str = String::from_utf8(body.to_vec()).unwrap();
    assert!(body_str.contains("site_name"));
    assert!(body_str.contains("Old Name"));
}

#[tokio::test]
async fn test_update_setting_returns_200_and_persists() {
    let (app, db) = setup_app_with_setting().await;
    let request: Request<Body> = Request::builder()
        .method("PUT")
        .uri("/api/settings/site_name")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"value":"New Name"}"#))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), 200);

    let model = setting::Entity::find_by_id("site_name")
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(model.value, "New Name");
}

#[tokio::test]
async fn test_update_setting_returns_404_for_missing_key() {
    let (app, _db) = setup_app_with_setting().await;
    let request: Request<Body> = Request::builder()
        .method("PUT")
        .uri("/api/settings/missing_key")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"value":"x"}"#))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), 404);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_str = String::from_utf8(body.to_vec()).unwrap();
    assert!(body_str.contains("NOT_FOUND"));
}

#[tokio::test]
async fn test_list_settings_returns_500_on_db_error() {
    let (app, db) = setup_app_with_setting().await;

    // Drop the setting table to force a DB error.
    db.execute_unprepared("DROP TABLE setting").await.unwrap();

    let request: Request<Body> = Request::builder()
        .method("GET")
        .uri("/api/settings")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), 500);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_str = String::from_utf8(body.to_vec()).unwrap();
    assert!(body_str.contains("DB_ERROR"));
}

#[tokio::test]
async fn test_update_setting_returns_500_on_db_error() {
    let (app, db) = setup_app_with_setting().await;

    // Drop the setting table to force a DB error.
    db.execute_unprepared("DROP TABLE setting").await.unwrap();

    let request: Request<Body> = Request::builder()
        .method("PUT")
        .uri("/api/settings/site_name")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"value":"New Name"}"#))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), 500);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_str = String::from_utf8(body.to_vec()).unwrap();
    assert!(body_str.contains("DB_ERROR"));
}

async fn insert_setting(db: &sea_orm::DatabaseConnection, key: &str, value: &str, r#type: i32) {
    let active = setting::ActiveModel {
        key: Set(key.to_string()),
        value: Set(value.to_string()),
        r#type: Set(r#type),
        updated_at: Set(chrono::Utc::now()),
    };
    active.insert(db).await.unwrap();
}

async fn put_setting(app: axum::Router, key: &str, value: &str) -> axum::response::Response {
    let request: Request<Body> = Request::builder()
        .method("PUT")
        .uri(format!("/api/settings/{key}"))
        .header("content-type", "application/json")
        .body(Body::from(format!(r#"{{"value":"{value}"}}"#)))
        .unwrap();
    app.oneshot(request).await.unwrap()
}

#[tokio::test]
async fn test_update_setting_int_rejects_non_integer() {
    let (app, db) = setup_app_with_setting().await;
    insert_setting(&db, "max_retries", "3", 2).await;

    let response = put_setting(app, "max_retries", "abc").await;
    assert_eq!(response.status(), 400);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_str = String::from_utf8(body.to_vec()).unwrap();
    assert!(body_str.contains("INVALID_INPUT"));

    // The stored value must be unchanged.
    let model = setting::Entity::find_by_id("max_retries")
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(model.value, "3");
}

#[tokio::test]
async fn test_update_setting_int_accepts_integer() {
    let (app, db) = setup_app_with_setting().await;
    insert_setting(&db, "max_retries", "3", 2).await;

    let response = put_setting(app, "max_retries", "5").await;
    assert_eq!(response.status(), 200);

    let model = setting::Entity::find_by_id("max_retries")
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(model.value, "5");
}

#[tokio::test]
async fn test_update_setting_bool_rejects_non_boolean() {
    let (app, db) = setup_app_with_setting().await;
    insert_setting(&db, "feature_enabled", "true", 3).await;

    let response = put_setting(app, "feature_enabled", "yes").await;
    assert_eq!(response.status(), 400);

    let model = setting::Entity::find_by_id("feature_enabled")
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(model.value, "true");
}

#[tokio::test]
async fn test_update_setting_float_accepts_number() {
    let (app, db) = setup_app_with_setting().await;
    insert_setting(&db, "ratio", "0.5", 1).await;

    let response = put_setting(app, "ratio", "1.5").await;
    assert_eq!(response.status(), 200);

    let model = setting::Entity::find_by_id("ratio")
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(model.value, "1.5");
}

#[tokio::test]
async fn test_update_setting_string_accepts_anything() {
    let (app, _db) = setup_app_with_setting().await;

    let response = put_setting(app, "site_name", "任意字符串 123").await;
    assert_eq!(response.status(), 200);
}

async fn delete_setting(app: axum::Router, key: &str) -> axum::response::Response {
    let request: Request<Body> = Request::builder()
        .method("DELETE")
        .uri(format!("/api/settings/{key}"))
        .body(Body::empty())
        .unwrap();
    app.oneshot(request).await.unwrap()
}

#[tokio::test]
async fn test_delete_setting_removes_row_and_persists() {
    let (app, db) = setup_app_with_setting().await;
    insert_setting(&db, "max_retries", "3", 2).await;

    let response = delete_setting(app, "max_retries").await;
    assert_eq!(response.status(), 200);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_str = String::from_utf8(body.to_vec()).unwrap();
    assert!(body_str.contains("\"code\":\"0\""));

    let model = setting::Entity::find_by_id("max_retries")
        .one(&db)
        .await
        .unwrap();
    assert!(model.is_none(), "删除后设置行应不存在");
}

#[tokio::test]
async fn test_delete_setting_returns_404_for_missing_key() {
    let (app, _db) = setup_app_with_setting().await;

    let response = delete_setting(app, "missing_key").await;
    assert_eq!(response.status(), 404);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_str = String::from_utf8(body.to_vec()).unwrap();
    assert!(body_str.contains("NOT_FOUND"));
}

#[tokio::test]
async fn test_delete_setting_protects_builtin_language_and_timezone() {
    let (app, db) = setup_app_with_setting().await;
    // 真实启动路径（AppSettings::load_from_db）会种入这两行；测试库需手动种入。
    insert_setting(&db, "language", "zh-CN", 0).await;
    insert_setting(&db, "timezone", "Asia/Shanghai", 0).await;

    let response = delete_setting(app.clone(), "language").await;
    assert_eq!(response.status(), 400);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_str = String::from_utf8(body.to_vec()).unwrap();
    assert!(body_str.contains("INVALID_INPUT"), "{body_str}");

    let response = delete_setting(app, "timezone").await;
    assert_eq!(response.status(), 400);

    // 受保护行必须仍在库中。
    assert!(
        setting::Entity::find_by_id("language")
            .one(&db)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        setting::Entity::find_by_id("timezone")
            .one(&db)
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn test_delete_setting_returns_500_on_db_error() {
    let (app, db) = setup_app_with_setting().await;

    // Drop the setting table to force a DB error.
    db.execute_unprepared("DROP TABLE setting").await.unwrap();

    let response = delete_setting(app, "site_name").await;
    assert_eq!(response.status(), 500);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_str = String::from_utf8(body.to_vec()).unwrap();
    assert!(body_str.contains("DB_ERROR"));
}

// ── downstream_request_header_allow_list（Json 类型 + 透传黑名单校验）──

async fn setup_app_with_allowlist_setting() -> (axum::Router, sea_orm::DatabaseConnection) {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    insert_setting(
        &db,
        llm_gateway::app_settings::KEY_DOWNSTREAM_REQUEST_HEADER_ALLOW_LIST,
        llm_gateway::app_settings::DEFAULT_DOWNSTREAM_REQUEST_HEADER_ALLOW_LIST,
        4,
    )
    .await;
    scheduler.start().await.unwrap();
    let app = common::build_authed_app(db.clone(), scheduler, log_tx).await;
    (app, db)
}

async fn allowlist_setting_value(db: &sea_orm::DatabaseConnection) -> String {
    setting::Entity::find_by_id(llm_gateway::app_settings::KEY_DOWNSTREAM_REQUEST_HEADER_ALLOW_LIST)
        .one(db)
        .await
        .unwrap()
        .unwrap()
        .value
}

#[tokio::test]
async fn test_update_allowlist_rejects_invalid_json() {
    let (app, db) = setup_app_with_allowlist_setting().await;

    let response = put_setting(app, "downstream_request_header_allow_list", r#"not-json"#).await;
    assert_eq!(response.status(), 400);
    assert_eq!(
        allowlist_setting_value(&db).await,
        llm_gateway::app_settings::DEFAULT_DOWNSTREAM_REQUEST_HEADER_ALLOW_LIST
    );
}

#[tokio::test]
async fn test_update_allowlist_rejects_non_array() {
    let (app, _db) = setup_app_with_allowlist_setting().await;

    // 合法 JSON 但不是字符串数组。
    let response = put_setting(app, "downstream_request_header_allow_list", r#"{"a":"b"}"#).await;
    assert_eq!(response.status(), 400);
}

#[tokio::test]
async fn test_update_allowlist_rejects_blacklisted_header() {
    let (app, _db) = setup_app_with_allowlist_setting().await;

    let response = put_setting(
        app,
        "downstream_request_header_allow_list",
        r#"[\"user-agent\", \"cookie\"]"#,
    )
    .await;
    assert_eq!(response.status(), 400);
}

#[tokio::test]
async fn test_update_allowlist_rejects_invalid_header_name() {
    let (app, _db) = setup_app_with_allowlist_setting().await;

    let response = put_setting(
        app,
        "downstream_request_header_allow_list",
        r#"[\"bad header!\"]"#,
    )
    .await;
    assert_eq!(response.status(), 400);
}

#[tokio::test]
async fn test_update_allowlist_accepts_valid_array_and_persists() {
    let (app, db) = setup_app_with_allowlist_setting().await;

    let response = put_setting(
        app,
        "downstream_request_header_allow_list",
        r#"[\"user-agent\", \"traceparent\"]"#,
    )
    .await;
    assert_eq!(response.status(), 200);
    assert_eq!(
        allowlist_setting_value(&db).await,
        r#"["user-agent", "traceparent"]"#
    );
}

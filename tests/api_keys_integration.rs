mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sea_orm::EntityTrait;
use serde_json::Value;
use tower::ServiceExt;

use llm_gateway::crypto::ENCRYPTION_KEY_ENV;

const TEST_KEY: &str = "integration-test-key";

async fn setup_app() -> (axum::Router, sea_orm::DatabaseConnection) {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    let app = common::build_authed_app(db.clone(), scheduler, log_tx).await;
    (app, db)
}

async fn send_json(app: &axum::Router, method: &str, uri: &str, body: &str) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

async fn create_key(app: &axum::Router, name: &str) -> (StatusCode, Value) {
    let body = serde_json::json!({ "name": name }).to_string();
    send_json(app, "POST", "/api/api-keys", &body).await
}

/// 断言明文 key 符合 `lg-` + 32 位 hex 的服务端生成格式。
fn assert_valid_key_format(key: &str) {
    assert!(key.starts_with("lg-"), "key 应以 lg- 前缀开头: {key}");
    let body = &key["lg-".len()..];
    assert_eq!(body.len(), 32, "key 随机段应为 32 位: {key}");
    assert!(
        body.bytes().all(|b| b.is_ascii_hexdigit()),
        "key 随机段应为 hex: {key}"
    );
}

#[tokio::test]
async fn test_create_api_key_generates_encrypted_key_and_masks_response() {
    temp_env::async_with_vars([(ENCRYPTION_KEY_ENV, Some(TEST_KEY))], async {
        let (app, db) = setup_app().await;
        let (status, body) = create_key(&app, "my-laptop").await;

        assert_eq!(status, StatusCode::CREATED);
        let data = &body["data"];
        assert_eq!(data["name"], "my-laptop");
        assert_eq!(data["enable"], true);
        // 掩码格式：lg- + **** + 末 4 位，共 11 字符。
        let masked = data["keyMasked"].as_str().unwrap();
        assert!(masked.starts_with("lg-****"), "掩码格式不符: {masked}");
        assert_eq!(masked.len(), 11);
        assert!(data.get("key").is_none(), "创建响应不得返回明文 key");

        // 数据库里必须是加密后的密文，且明文符合生成格式。
        let model =
            llm_gateway::entity::api_key::Entity::find_by_id(data["id"].as_i64().unwrap() as i32)
                .one(&db)
                .await
                .unwrap()
                .unwrap();
        assert!(model.key.starts_with("enc:v1:"), "key 应以密文落库");
        let plain = llm_gateway::crypto::decrypt(&model.key).unwrap();
        assert_valid_key_format(&plain);
    })
    .await;
}

#[tokio::test]
async fn test_list_api_keys_masks_key() {
    temp_env::async_with_vars([(ENCRYPTION_KEY_ENV, Some(TEST_KEY))], async {
        let (app, _db) = setup_app().await;
        create_key(&app, "key-a").await;
        create_key(&app, "key-b").await;

        let (status, body) = send_json(&app, "GET", "/api/api-keys", "").await;
        assert_eq!(status, StatusCode::OK);
        // 列表包含公共种子的 itest-key，这里只断言本测试创建的两个。
        let items: Vec<&Value> = body["data"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|item| matches!(item["name"].as_str(), Some("key-a") | Some("key-b")))
            .collect();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["name"], "key-a");
        assert_eq!(items[1]["name"], "key-b");
        for item in items {
            assert!(item["keyMasked"].as_str().unwrap().starts_with("lg-****"));
            assert!(item.get("key").is_none(), "列表响应不得返回明文 key");
        }
    })
    .await;
}

#[tokio::test]
async fn test_detail_returns_plaintext_key() {
    temp_env::async_with_vars([(ENCRYPTION_KEY_ENV, Some(TEST_KEY))], async {
        let (app, _db) = setup_app().await;
        let (_, created) = create_key(&app, "detail-check").await;
        let id = created["data"]["id"].as_i64().unwrap();

        let (status, body) = send_json(&app, "GET", &format!("/api/api-keys/{id}"), "").await;
        assert_eq!(status, StatusCode::OK);
        let data = &body["data"];
        assert_eq!(data["name"], "detail-check");
        assert!(data.get("keyMasked").is_some());
        let plain = data["key"].as_str().unwrap();
        assert_valid_key_format(plain);
    })
    .await;
}

#[tokio::test]
async fn test_detail_returns_404_for_missing_key() {
    let (app, _db) = setup_app().await;
    let (status, body) = send_json(&app, "GET", "/api/api-keys/999", "").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "NOT_FOUND");
}

#[tokio::test]
async fn test_create_api_key_rejects_blank_name() {
    let (app, _db) = setup_app().await;
    let (status, body) = create_key(&app, "   ").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "INVALID_INPUT");
    assert_eq!(body["msg"], "名称不能为空");
}

#[tokio::test]
async fn test_create_api_key_rejects_duplicate_name() {
    temp_env::async_with_vars([(ENCRYPTION_KEY_ENV, Some(TEST_KEY))], async {
        let (app, _db) = setup_app().await;
        let (first_status, _) = create_key(&app, "dup").await;
        assert_eq!(first_status, StatusCode::CREATED);

        let (status, body) = create_key(&app, "dup").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["code"], "INVALID_INPUT");
        assert_eq!(body["msg"], "同名 API Key 已存在，名称需要唯一");
    })
    .await;
}

#[tokio::test]
async fn test_update_api_key_toggles_enable() {
    temp_env::async_with_vars([(ENCRYPTION_KEY_ENV, Some(TEST_KEY))], async {
        let (app, _db) = setup_app().await;
        let (_, created) = create_key(&app, "toggle").await;
        let id = created["data"]["id"].as_i64().unwrap();
        let original_key = {
            let (_, detail) = send_json(&app, "GET", &format!("/api/api-keys/{id}"), "").await;
            detail["data"]["key"].as_str().unwrap().to_string()
        };

        // 禁用。
        let (status, body) = send_json(
            &app,
            "PUT",
            &format!("/api/api-keys/{id}"),
            r#"{"enable":false}"#,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["enable"], false);

        // 重新启用。
        let (status, body) = send_json(
            &app,
            "PUT",
            &format!("/api/api-keys/{id}"),
            r#"{"enable":true}"#,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["enable"], true);

        // 启停不影响密钥本身。
        let (_, detail) = send_json(&app, "GET", &format!("/api/api-keys/{id}"), "").await;
        assert_eq!(detail["data"]["key"].as_str().unwrap(), original_key);
    })
    .await;
}

#[tokio::test]
async fn test_update_missing_api_key_returns_404() {
    let (app, _db) = setup_app().await;
    let (status, body) = send_json(&app, "PUT", "/api/api-keys/999", r#"{"enable":false}"#).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "NOT_FOUND");
}

#[tokio::test]
async fn test_delete_api_key() {
    temp_env::async_with_vars([(ENCRYPTION_KEY_ENV, Some(TEST_KEY))], async {
        let (app, _db) = setup_app().await;
        let (_, created) = create_key(&app, "to-delete").await;
        let id = created["data"]["id"].as_i64().unwrap();

        let (status, _) = send_json(&app, "DELETE", &format!("/api/api-keys/{id}"), "").await;
        assert_eq!(status, StatusCode::OK);

        // 删除后详情 404，重复删除也 404。
        let (status, _) = send_json(&app, "GET", &format!("/api/api-keys/{id}"), "").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = send_json(&app, "DELETE", &format!("/api/api-keys/{id}"), "").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    })
    .await;
}

#[tokio::test]
async fn test_create_api_key_stores_plaintext_when_encryption_not_configured() {
    temp_env::async_with_vars([(ENCRYPTION_KEY_ENV, None::<&str>)], async {
        let (app, db) = setup_app().await;
        let (status, body) = create_key(&app, "plain-store").await;
        assert_eq!(status, StatusCode::CREATED);

        // 未配置加密密钥时退化为明文落库（开发环境行为）。
        let model = llm_gateway::entity::api_key::Entity::find_by_id(
            body["data"]["id"].as_i64().unwrap() as i32,
        )
        .one(&db)
        .await
        .unwrap()
        .unwrap();
        assert!(!model.key.starts_with("enc:v1:"), "未配置密钥时应明文落库");
        assert_valid_key_format(&model.key);

        // 详情仍返回明文。
        let id = body["data"]["id"].as_i64().unwrap();
        let (_, detail) = send_json(&app, "GET", &format!("/api/api-keys/{id}"), "").await;
        assert_eq!(detail["data"]["key"], model.key);
    })
    .await;
}

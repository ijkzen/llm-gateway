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
    let app = common::build_app(db.clone(), scheduler, log_tx);
    (app, db)
}

async fn send_json(
    app: &axum::Router,
    method: &str,
    uri: &str,
    body: &str,
) -> (StatusCode, Value) {
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

fn create_body(name: &str, base_url: &str, api_key: &str, extra: &str) -> String {
    serde_json::json!({
        "name": name,
        "enable": true,
        "baseUrl": base_url,
        "apiKey": api_key,
        "protocolType": 0,
        "billingMode": 0,
        "customHeader": "{}",
        "extra": extra,
    })
    .to_string()
}

#[tokio::test]
async fn test_create_provider_encrypts_api_key_and_masks_response() {
    temp_env::async_with_vars([(ENCRYPTION_KEY_ENV, Some(TEST_KEY))], async {
        let (app, db) = setup_app().await;
        let (status, body) = send_json(
            &app,
            "POST",
            "/api/providers",
            &create_body("DeepSeek", "https://api.deepseek.com", "sk-secret-1234", r#"{}"#),
        )
        .await;

        assert_eq!(status, StatusCode::CREATED);
        let data = &body["data"];
        assert_eq!(data["name"], "DeepSeek");
        assert_eq!(data["apiKeyMasked"], "sk-****1234");
        assert!(data.get("apiKey").is_none(), "列表/创建响应不得返回明文 api_key");

        // 数据库里必须是加密后的密文。
        let model = llm_gateway::entity::provider::Entity::find_by_id(data["id"].as_i64().unwrap() as i32)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert!(model.api_key.starts_with("enc:v1:"), "api_key 应以密文落库");
        assert_eq!(llm_gateway::crypto::decrypt(&model.api_key).unwrap(), "sk-secret-1234");
    })
    .await;
}

#[tokio::test]
async fn test_create_provider_rejects_empty_api_key() {
    let (app, _db) = setup_app().await;
    let (status, body) = send_json(
        &app,
        "POST",
        "/api/providers",
        &create_body("NoKey", "https://example.com/v1", "", r#"{}"#),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "INVALID_INPUT");
    assert_eq!(body["msg"], "API Key 不能为空");
}

#[tokio::test]
async fn test_create_provider_rejects_usage_extra_with_empty_required_fields() {
    let (app, _db) = setup_app().await;
    let extra = r#"{"cookie_cloud_server":"","uuid":"","password":"","domain":"","usage":true,"usage_type":1}"#;
    let (status, body) = send_json(
        &app,
        "POST",
        "/api/providers",
        &create_body("AliPlan", "https://coding.aliyun.com/v1", "sk-abc", extra),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "INVALID_INPUT");
    let msg = body["msg"].as_str().unwrap();
    assert!(msg.contains("用量查询已开启"));
    assert!(msg.contains("cookie_cloud_server"));
}

#[tokio::test]
async fn test_create_provider_with_filled_usage_extra_succeeds() {
    let (app, _db) = setup_app().await;
    let extra = r#"{"cookie_cloud_server":"server","uuid":"u-1","password":"p-1","domain":"d-1","usage":true,"usage_type":1}"#;
    let (status, body) = send_json(
        &app,
        "POST",
        "/api/providers",
        &create_body("AliPlanOk", "https://coding.aliyun.com/v1", "sk-abc", extra),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "usage 字段填写完整应创建成功: {body}");
}

#[tokio::test]
async fn test_create_provider_rejects_duplicate_name() {
    let (app, _db) = setup_app().await;
    let body1 = create_body("Dup", "https://api.dup.com/v1", "sk-1", r#"{}"#);
    let (status1, _) = send_json(&app, "POST", "/api/providers", &body1).await;
    assert_eq!(status1, StatusCode::CREATED);

    let (status2, body2) = send_json(&app, "POST", "/api/providers", &body1).await;
    assert_eq!(status2, StatusCode::BAD_REQUEST);
    assert!(body2["msg"].as_str().unwrap().contains("名称"));
}

#[tokio::test]
async fn test_get_provider_detail_returns_plaintext_api_key() {
    temp_env::async_with_vars([(ENCRYPTION_KEY_ENV, Some(TEST_KEY))], async {
        let (app, _db) = setup_app().await;
        let (_, created) = send_json(
            &app,
            "POST",
            "/api/providers",
            &create_body("Detail", "https://api.detail.com/v1", "sk-plain-9", r#"{}"#),
        )
        .await;
        let id = created["data"]["id"].as_i64().unwrap();

        let (status, body) = send_json(&app, "GET", &format!("/api/providers/{id}"), "{}").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["apiKey"], "sk-plain-9");
        assert_eq!(body["data"]["apiKeyMasked"], "sk-****in-9");
    })
    .await;
}

#[tokio::test]
async fn test_update_provider_keeps_key_when_empty_and_overwrites_when_filled() {
    temp_env::async_with_vars([(ENCRYPTION_KEY_ENV, Some(TEST_KEY))], async {
        let (app, db) = setup_app().await;
        let (_, created) = send_json(
            &app,
            "POST",
            "/api/providers",
            &create_body("Upd", "https://api.upd.com/v1", "sk-original", r#"{}"#),
        )
        .await;
        let id = created["data"]["id"].as_i64().unwrap() as i32;

        // 留空 api_key 更新其它字段，密钥保持不变。
        let (status, _) = send_json(
            &app,
            "PUT",
            &format!("/api/providers/{id}"),
            r#"{"enable":false,"apiKey":""}"#,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let model = llm_gateway::entity::provider::Entity::find_by_id(id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert!(!model.enable);
        assert_eq!(llm_gateway::crypto::decrypt(&model.api_key).unwrap(), "sk-original");

        // 填写新 api_key 覆盖旧密钥。
        let (status, _) = send_json(
            &app,
            "PUT",
            &format!("/api/providers/{id}"),
            r#"{"apiKey":"sk-new-key"}"#,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let model = llm_gateway::entity::provider::Entity::find_by_id(id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(llm_gateway::crypto::decrypt(&model.api_key).unwrap(), "sk-new-key");
    })
    .await;
}

#[tokio::test]
async fn test_delete_provider_then_404() {
    let (app, _db) = setup_app().await;
    let (_, created) = send_json(
        &app,
        "POST",
        "/api/providers",
        &create_body("Del", "https://api.del.com/v1", "sk-del", r#"{}"#),
    )
    .await;
    let id = created["data"]["id"].as_i64().unwrap();

    let (status, _) = send_json(&app, "DELETE", &format!("/api/providers/{id}"), "{}").await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = send_json(&app, "GET", &format!("/api/providers/{id}"), "{}").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "NOT_FOUND");
}

#[tokio::test]
async fn test_match_template_found_and_not_found() {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    // 集成测试用内存库，需要先种入模板种子数据。
    llm_gateway::provider_template::upsert_templates(&db).await.unwrap();
    let app = common::build_app(db.clone(), scheduler, log_tx);
    // 命中：种子模板中存在 DeepSeek。
    let (status, body) = send_json(
        &app,
        "POST",
        "/api/provider-templates/match",
        r#"{"baseUrl":"https://api.deepseek.com/v1"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["name"], "DeepSeek");

    // 未命中。
    let (status, body) = send_json(
        &app,
        "POST",
        "/api/provider-templates/match",
        r#"{"baseUrl":"https://no-such-host.example.com/v1"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["msg"], "未找到匹配的模板");

    // 空 Base URL。
    let (status, body) = send_json(
        &app,
        "POST",
        "/api/provider-templates/match",
        r#"{"baseUrl":""}"#,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["msg"], "Base URL 不能为空");
}

#[tokio::test]
async fn test_create_provider_rejects_bad_extra_json() {
    let (app, _db) = setup_app().await;
    let (status, body) = send_json(
        &app,
        "POST",
        "/api/providers",
        &create_body("Bad", "https://api.bad.com/v1", "sk-1", r#"{not-json}"#),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["msg"].as_str().unwrap().contains("额外字段"));
}

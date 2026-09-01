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

/// 创建一个名称与 Base URL 一致的 Provider，返回其 id。
async fn create_named_provider(app: &axum::Router, name: &str) -> i64 {
    let (status, body) = send_json(
        app,
        "POST",
        "/api/providers",
        &create_body(
            name,
            &format!("https://{name}.example.com/v1"),
            "sk-1",
            r#"{}"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "创建 {name} 失败: {body}");
    body["data"]["id"].as_i64().unwrap()
}

#[tokio::test]
async fn test_create_provider_encrypts_api_key_and_masks_response() {
    temp_env::async_with_vars([(ENCRYPTION_KEY_ENV, Some(TEST_KEY))], async {
        let (app, db) = setup_app().await;
        let (status, body) = send_json(
            &app,
            "POST",
            "/api/providers",
            &create_body(
                "DeepSeek",
                "https://api.deepseek.com",
                "sk-secret-1234",
                r#"{}"#,
            ),
        )
        .await;

        assert_eq!(status, StatusCode::CREATED);
        let data = &body["data"];
        assert_eq!(data["name"], "DeepSeek");
        assert_eq!(data["apiKeyMasked"], "sk-****1234");
        assert!(
            data.get("apiKey").is_none(),
            "列表/创建响应不得返回明文 api_key"
        );

        // 数据库里必须是加密后的密文。
        let model =
            llm_gateway::entity::provider::Entity::find_by_id(data["id"].as_i64().unwrap() as i32)
                .one(&db)
                .await
                .unwrap()
                .unwrap();
        assert!(model.api_key.starts_with("enc:v1:"), "api_key 应以密文落库");
        assert_eq!(
            llm_gateway::crypto::decrypt(&model.api_key).unwrap(),
            "sk-secret-1234"
        );
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

    assert_eq!(
        status,
        StatusCode::CREATED,
        "usage 字段填写完整应创建成功: {body}"
    );
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
async fn test_get_provider_detail_does_not_return_plaintext_api_key() {
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
        assert!(
            body["data"].get("apiKey").is_none(),
            "详情接口不得返回明文 api_key"
        );
        assert_eq!(body["data"]["apiKeyMasked"], "sk-****in-9");
    })
    .await;
}

#[tokio::test]
async fn test_get_provider_api_key_endpoint_returns_plaintext() {
    temp_env::async_with_vars([(ENCRYPTION_KEY_ENV, Some(TEST_KEY))], async {
        let (app, _db) = setup_app().await;
        let (_, created) = send_json(
            &app,
            "POST",
            "/api/providers",
            &create_body("ApiKey", "https://api.apikey.com/v1", "sk-plain-9", r#"{}"#),
        )
        .await;
        let id = created["data"]["id"].as_i64().unwrap();

        let (status, body) =
            send_json(&app, "GET", &format!("/api/providers/{id}/api-key"), "{}").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["data"]["apiKey"], "sk-plain-9");
    })
    .await;
}

#[tokio::test]
async fn test_get_provider_api_key_endpoint_not_found() {
    temp_env::async_with_vars([(ENCRYPTION_KEY_ENV, Some(TEST_KEY))], async {
        let (app, _db) = setup_app().await;
        let (status, _body) = send_json(&app, "GET", "/api/providers/99999/api-key", "{}").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
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
        assert_eq!(
            llm_gateway::crypto::decrypt(&model.api_key).unwrap(),
            "sk-original"
        );

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
        assert_eq!(
            llm_gateway::crypto::decrypt(&model.api_key).unwrap(),
            "sk-new-key"
        );
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
    llm_gateway::provider_template::upsert_templates(&db)
        .await
        .unwrap();
    let app = common::build_authed_app(db.clone(), scheduler, log_tx).await;
    // 命中：种子模板中存在 DeepSeek，data 是模板列表。
    let (status, body) = send_json(
        &app,
        "POST",
        "/api/provider-templates/match",
        r#"{"baseUrl":"https://api.deepseek.com/v1"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["data"].is_array());
    assert_eq!(body["data"][0]["name"], "DeepSeek");

    // 同一 host 返回全部命中（api.stepfun.com 有按量与 Step Plan 两个模板）。
    let (status, body) = send_json(
        &app,
        "POST",
        "/api/provider-templates/match",
        r#"{"baseUrl":"https://api.stepfun.com/v1"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["data"].as_array().unwrap().len() >= 2);

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

#[tokio::test]
async fn test_list_providers_returns_insert_order_by_default() {
    let (app, _db) = setup_app().await;
    for name in ["Alpha", "Bravo", "Charlie"] {
        create_named_provider(&app, name).await;
    }

    // 未重排时全部 sort_order 为 0，按 id 升序（即插入顺序）。
    let (status, body) = send_json(&app, "GET", "/api/providers", "{}").await;
    assert_eq!(status, StatusCode::OK);
    let names: Vec<&str> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Alpha", "Bravo", "Charlie"]);
}

#[tokio::test]
async fn test_reorder_providers_updates_list_order() {
    let (app, db) = setup_app().await;
    let mut ids: Vec<i32> = Vec::new();
    for name in ["Alpha", "Bravo", "Charlie"] {
        ids.push(create_named_provider(&app, name).await as i32);
    }

    // 倒序重排：Charlie、Alpha、Bravo。
    let reordered = vec![ids[2], ids[0], ids[1]];
    let (status, _) = send_json(
        &app,
        "PUT",
        "/api/providers/reorder",
        &serde_json::json!({ "ids": reordered }).to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = send_json(&app, "GET", "/api/providers", "{}").await;
    assert_eq!(status, StatusCode::OK);
    let got: Vec<i32> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["id"].as_i64().unwrap() as i32)
        .collect();
    assert_eq!(got, reordered);

    // sort_order 按数组下标落库。
    for (index, id) in reordered.iter().enumerate() {
        let model = llm_gateway::entity::provider::Entity::find_by_id(*id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(model.sort_order, index as i32);
    }
}

#[tokio::test]
async fn test_reorder_providers_rolls_back_when_id_missing() {
    let (app, _db) = setup_app().await;
    let mut ids: Vec<i32> = Vec::new();
    for name in ["Alpha", "Bravo"] {
        ids.push(create_named_provider(&app, name).await as i32);
    }

    // 列表中混入不存在的 id → 整体拒绝，顺序保持不变（原子回滚）。
    let (status, body) = send_json(
        &app,
        "PUT",
        "/api/providers/reorder",
        &serde_json::json!({ "ids": [ids[1], 999, ids[0]] }).to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body["msg"].as_str().unwrap().contains("999"));

    let (_, body) = send_json(&app, "GET", "/api/providers", "{}").await;
    let names: Vec<&str> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Alpha", "Bravo"]);
}

#[tokio::test]
async fn test_reorder_providers_rejects_empty_and_duplicate_ids() {
    let (app, _db) = setup_app().await;
    let id = create_named_provider(&app, "Solo").await;

    // 空列表。
    let (status, body) = send_json(&app, "PUT", "/api/providers/reorder", r#"{"ids":[]}"#).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["msg"].as_str().unwrap().contains("不能为空"));

    // 重复 id。
    let (status, body) = send_json(
        &app,
        "PUT",
        "/api/providers/reorder",
        &serde_json::json!({ "ids": [id, id] }).to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["msg"].as_str().unwrap().contains("重复"));
}

/// 创建带网络代理的供应商：proxyEnabled + proxyAddr 落库并返回。
#[tokio::test]
async fn test_create_provider_with_proxy() {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    let app = common::build_authed_app(db.clone(), scheduler, log_tx).await;

    let body = serde_json::json!({
        "name": "proxy-provider",
        "enable": true,
        "baseUrl": "https://api.example.com/v1",
        "apiKey": "sk-1",
        "protocolType": 0,
        "billingMode": 0,
        "customHeader": "{}",
        "extra": "{}",
        "proxyEnabled": true,
        "proxyAddr": "http://127.0.0.1:7890",
    })
    .to_string();
    let (status, body) = send_json(&app, "POST", "/api/providers", &body).await;
    assert_eq!(status, StatusCode::CREATED, "创建失败: {body}");
    assert_eq!(body["data"]["proxyEnabled"], true);
    assert_eq!(body["data"]["proxyAddr"], "http://127.0.0.1:7890");

    // 详情返回 proxy 字段。
    let id = body["data"]["id"].as_i64().unwrap();
    let (status, body) = send_json(&app, "GET", &format!("/api/providers/{id}"), "{}").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["proxyEnabled"], true);
    assert_eq!(body["data"]["proxyAddr"], "http://127.0.0.1:7890");
}

/// 开启网络代理但地址为空 → 400。
#[tokio::test]
async fn test_create_provider_proxy_enabled_requires_addr() {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    let app = common::build_authed_app(db.clone(), scheduler, log_tx).await;

    let body = serde_json::json!({
        "name": "proxy-empty",
        "enable": true,
        "baseUrl": "https://api.example.com/v1",
        "apiKey": "sk-1",
        "protocolType": 0,
        "billingMode": 0,
        "customHeader": "{}",
        "extra": "{}",
        "proxyEnabled": true,
        "proxyAddr": "",
    })
    .to_string();
    let (status, body) = send_json(&app, "POST", "/api/providers", &body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["msg"].as_str().unwrap().contains("代理地址"),
        "应提示代理地址必填: {body}"
    );
}

/// 代理地址非 http:// 开头 → 400。
#[tokio::test]
async fn test_create_provider_proxy_addr_must_be_http() {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    let app = common::build_authed_app(db.clone(), scheduler, log_tx).await;

    let body = serde_json::json!({
        "name": "proxy-https",
        "enable": true,
        "baseUrl": "https://api.example.com/v1",
        "apiKey": "sk-1",
        "protocolType": 0,
        "billingMode": 0,
        "customHeader": "{}",
        "extra": "{}",
        "proxyEnabled": true,
        "proxyAddr": "https://127.0.0.1:7890",
    })
    .to_string();
    let (status, body) = send_json(&app, "POST", "/api/providers", &body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["msg"].as_str().unwrap().contains("http://"),
        "应提示 http:// 前缀: {body}"
    );
}

/// 编辑供应商时更新 proxy 字段（未传则保持原值）。
#[tokio::test]
async fn test_update_provider_proxy() {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    let app = common::build_authed_app(db.clone(), scheduler, log_tx).await;

    // 先创建无代理的供应商。
    let body = serde_json::json!({
        "name": "proxy-update",
        "enable": true,
        "baseUrl": "https://api.example.com/v1",
        "apiKey": "sk-1",
        "protocolType": 0,
        "billingMode": 0,
        "customHeader": "{}",
        "extra": "{}",
    })
    .to_string();
    let (status, body) = send_json(&app, "POST", "/api/providers", &body).await;
    assert_eq!(status, StatusCode::CREATED);
    let id = body["data"]["id"].as_i64().unwrap();

    // 编辑：开代理 + 地址。
    let body = serde_json::json!({
        "proxyEnabled": true,
        "proxyAddr": "http://10.0.0.1:8080",
    })
    .to_string();
    let (status, body) = send_json(&app, "PUT", &format!("/api/providers/{id}"), &body).await;
    assert_eq!(status, StatusCode::OK, "更新失败: {body}");
    assert_eq!(body["data"]["proxyEnabled"], true);
    assert_eq!(body["data"]["proxyAddr"], "http://10.0.0.1:8080");
}

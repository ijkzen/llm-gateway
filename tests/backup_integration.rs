mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sea_orm::{ActiveModelTrait, EntityTrait, Set};
use serde_json::Value;
use tower::ServiceExt;

use llm_gateway::crypto::ENCRYPTION_KEY_ENV;
use llm_gateway::entity::{api_key, provider, provider_model, virtual_model, virtual_model_item};

const TEST_KEY: &str = "backup-test-encryption-key";

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

fn provider_body(name: &str, api_key: &str) -> String {
    serde_json::json!({
        "name": name,
        "enable": true,
        "baseUrl": format!("https://{name}.example.com/v1"),
        "apiKey": api_key,
        "protocolType": 0,
        "billingMode": 0,
        "customHeader": "{}",
        "extra": "{}",
    })
    .to_string()
}

async fn create_provider(app: &axum::Router, name: &str) -> i32 {
    let (status, body) = send_json(
        app,
        "POST",
        "/api/providers",
        &provider_body(name, "sk-original"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "create provider failed: {body}"
    );
    body["data"]["id"].as_i64().unwrap() as i32
}

async fn create_model(app: &axum::Router, provider_id: i32, model_id: &str) -> i32 {
    let body = serde_json::json!({
        "providerModelId": model_id,
        "contextLength": 1000,
        "maxOutputTokens": 1000,
        "reasoning": false,
        "toolUse": true,
        "imageUnderstand": false,
        "videoUnderstand": false,
    });
    let (status, body) = send_json(
        app,
        "POST",
        &format!("/api/providers/{provider_id}/models"),
        &body.to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "create model failed: {body}");
    body["data"]["modelId"].as_i64().unwrap() as i32
}

/// 直接在库里造一条虚拟模型（含成员），返回 (虚拟模型 id, 成员引用).
async fn create_virtual_model_with_item(
    db: &sea_orm::DatabaseConnection,
    display_id: &str,
    model_id: i32,
) -> i32 {
    let now = chrono::Utc::now();
    let vm = virtual_model::ActiveModel {
        display_id: Set(display_id.to_string()),
        enable: Set(true),
        load_balancing_strategy: Set(0),
        fallback_strategy: Set(0),
        interface_type: Set(0),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap();
    virtual_model_item::ActiveModel {
        virtual_model_id: Set(vm.virtual_model_id),
        model_id: Set(model_id),
        enable: Set(true),
        cascade_disabled: Set(false),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap();
    vm.virtual_model_id
}

async fn export_backup(app: &axum::Router) -> Value {
    let (status, body) = send_json(app, "GET", "/api/backup/export", "").await;
    assert_eq!(status, StatusCode::OK, "export failed: {body}");
    body["data"].clone()
}

async fn count_rows(db: &sea_orm::DatabaseConnection) -> (usize, usize, usize, usize) {
    let providers = provider::Entity::find().all(db).await.unwrap().len();
    let models = provider_model::Entity::find().all(db).await.unwrap().len();
    let vms = virtual_model::Entity::find().all(db).await.unwrap().len();
    let items = virtual_model_item::Entity::find()
        .all(db)
        .await
        .unwrap()
        .len();
    (providers, models, vms, items)
}

/// 在测试库里直接写一行设置（测试库不自动种核心设置种子行）。
async fn insert_setting(db: &sea_orm::DatabaseConnection, key: &str, value: &str, type_: i32) {
    let now = chrono::Utc::now();
    llm_gateway::entity::setting::ActiveModel {
        key: Set(key.to_string()),
        value: Set(value.to_string()),
        r#type: Set(type_),
        updated_at: Set(now),
    }
    .insert(db)
    .await
    .unwrap();
}

#[tokio::test]
async fn export_contains_all_config_with_plaintext_keys() {
    temp_env::async_with_vars([(ENCRYPTION_KEY_ENV, Some(TEST_KEY))], async {
        let (app, _db) = setup_app().await;
        let pid = create_provider(&app, "export-pro").await;
        let mid = create_model(&app, pid, "m-1").await;
        create_virtual_model_with_item(&_db, "export-vm", mid).await;

        let data = export_backup(&app).await;
        assert_eq!(data["version"], 1);
        assert_eq!(data["providers"].as_array().unwrap().len(), 1);
        let p = &data["providers"][0];
        assert_eq!(p["name"], "export-pro");
        assert_eq!(p["apiKey"], "sk-original", "apiKey 应为明文");
        assert_eq!(p["models"].as_array().unwrap().len(), 1);
        assert_eq!(p["models"][0]["providerModelId"], "m-1");
        assert_eq!(data["virtualModels"].as_array().unwrap().len(), 1);
        assert_eq!(
            data["virtualModels"][0]["items"][0]["providerName"],
            "export-pro"
        );
        assert_eq!(
            data["virtualModels"][0]["items"][0]["providerModelId"],
            "m-1"
        );
    })
    .await;
}

#[tokio::test]
async fn export_includes_api_keys_and_settings() {
    temp_env::async_with_vars([(ENCRYPTION_KEY_ENV, Some(TEST_KEY))], async {
        let (app, db) = setup_app().await;
        create_provider(&app, "export-ak").await;
        // 手动种两个核心设置行，验证导出/恢复覆盖到设置。
        insert_setting(&db, "language", "zh-CN", 0).await;
        insert_setting(&db, "timezone", "Asia/Shanghai", 0).await;
        // build_authed_app 种入 itest-key。
        let data = export_backup(&app).await;
        assert_eq!(data["apiKeys"].as_array().unwrap().len(), 1);
        let k = &data["apiKeys"][0];
        assert_eq!(k["name"], "itest-key");
        assert_eq!(k["key"], common::TEST_API_KEY_PLAIN, "API Key 应为明文");
        let keys: Vec<&str> = data["settings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["key"].as_str().unwrap())
            .collect();
        assert!(keys.contains(&"language"));
        assert!(keys.contains(&"timezone"));
    })
    .await;
}

#[tokio::test]
async fn import_full_replace_roundtrip() {
    temp_env::async_with_vars([(ENCRYPTION_KEY_ENV, Some(TEST_KEY))], async {
        // 先建一份源数据并导出。
        let (app_src, db_src) = setup_app().await;
        let pid = create_provider(&app_src, "src-pro").await;
        let mid = create_model(&app_src, pid, "m-1").await;
        create_virtual_model_with_item(&db_src, "src-vm", mid).await;
        let exported = export_backup(&app_src).await;

        // 目标库：先有另一批旧数据。
        let (app_dst, db_dst) = setup_app().await;
        let old_pid = create_provider(&app_dst, "old-pro").await;
        let old_mid = create_model(&app_dst, old_pid, "old-m").await;
        create_virtual_model_with_item(&db_dst, "old-vm", old_mid).await;

        let (status, body) = send_json(
            &app_dst,
            "POST",
            "/api/backup/import",
            &exported.to_string(),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "import failed: {body}");

        // 旧数据整体替换为新数据。
        let (providers, models, vms, items) = count_rows(&db_dst).await;
        assert_eq!((providers, models, vms, items), (1, 1, 1, 1));
        let p = provider::Entity::find()
            .one(&db_dst)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(p.name, "src-pro");
        let vm = virtual_model::Entity::find()
            .one(&db_dst)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(vm.display_id, "src-vm");
        let item = virtual_model_item::Entity::find()
            .one(&db_dst)
            .await
            .unwrap()
            .unwrap();
        // 成员指向新插入的模型（model_id 自增 ≠ 源库 model_id 且属新 provider）。
        let new_model = provider_model::Entity::find_by_id(item.model_id)
            .one(&db_dst)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(new_model.provider_model_id, "m-1");

        // API Key 保留（导入未删除库里的 itest-key 之外又重插一份同名的？不——
        // 导入前删除了全部 api_key，导入用备份里的 itest-key 重建）。
        let keys = api_key::Entity::find().all(&db_dst).await.unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].name, "itest-key");
        let plain = llm_gateway::crypto::decrypt(&keys[0].key).unwrap();
        assert_eq!(plain, common::TEST_API_KEY_PLAIN);
        // key_hash 重算后仍可被 Bearer 鉴权命中（断言库中 hash 正确）。
        assert_eq!(
            keys[0].key_hash.as_deref(),
            Some(llm_gateway::auth::hash_token(common::TEST_API_KEY_PLAIN).as_str())
        );
    })
    .await;
}

#[tokio::test]
async fn import_preserves_unknown_settings_and_overwrites_known() {
    temp_env::async_with_vars([(ENCRYPTION_KEY_ENV, Some(TEST_KEY))], async {
        let (app, db) = setup_app().await;
        // 库里已有：timezone（备份里没有，应保留）+ site_name=old（备份里有，应覆盖）。
        insert_setting(&db, "timezone", "Asia/Shanghai", 0).await;
        insert_setting(&db, "site_name", "old-name", 0).await;

        // 备份只含 language=en + site_name=new-gw。
        let exported = serde_json::json!({
            "version": 1,
            "exportedAt": "2026-09-07T00:00:00Z",
            "providers": [],
            "virtualModels": [],
            "apiKeys": [{
                "name": "itest-key",
                "key": common::TEST_API_KEY_PLAIN,
                "enable": true
            }],
            "settings": [
                { "key": "language", "value": "en", "type": "String" },
                { "key": "site_name", "value": "new-gw", "type": "String" }
            ]
        });
        let (status, body) =
            send_json(&app, "POST", "/api/backup/import", &exported.to_string()).await;
        assert_eq!(status, StatusCode::OK, "import failed: {body}");

        let all = llm_gateway::entity::setting::Entity::find()
            .all(&db)
            .await
            .unwrap();
        let map: std::collections::HashMap<&str, &str> = all
            .iter()
            .map(|s| (s.key.as_str(), s.value.as_str()))
            .collect();
        assert_eq!(map.get("language"), Some(&"en"));
        assert_eq!(map.get("site_name"), Some(&"new-gw"));
        // 备份里没有的键保留（timezone 未被删）。
        assert_eq!(map.get("timezone"), Some(&"Asia/Shanghai"));
    })
    .await;
}

#[tokio::test]
async fn import_rejects_invalid_json_and_keeps_data() {
    temp_env::async_with_vars([(ENCRYPTION_KEY_ENV, Some(TEST_KEY))], async {
        let (app, db) = setup_app().await;
        create_provider(&app, "keep-pro").await;

        let (status, body) = send_json(&app, "POST", "/api/backup/import", "not json").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body["msg"].as_str().unwrap().contains("不是合法的 JSON"));

        // 版本不支持。
        let bad_version =
            r#"{"version":99,"providers":[],"virtualModels":[],"apiKeys":[],"settings":[]}"#;
        let (status, body) = send_json(&app, "POST", "/api/backup/import", bad_version).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body["msg"].as_str().unwrap().contains("版本不支持"));

        // 数据未被清空。
        let (providers, _, _, _) = count_rows(&db).await;
        assert_eq!(providers, 1);
    })
    .await;
}

#[tokio::test]
async fn import_rejects_bad_ref_and_keeps_data() {
    temp_env::async_with_vars([(ENCRYPTION_KEY_ENV, Some(TEST_KEY))], async {
        let (app, db) = setup_app().await;
        create_provider(&app, "keep2-pro").await;

        // 成员引用不存在的模型。
        let bad_ref = serde_json::json!({
            "version": 1,
            "exportedAt": "2026-09-07T00:00:00Z",
            "providers": [{
                "name": "x", "enable": true, "baseUrl": "https://x.example.com/v1",
                "apiKey": "k", "customHeader": "{}", "protocolType": 0,
                "billingMode": 0, "extra": "{}", "sortOrder": 0,
                "proxyEnabled": false, "proxyAddr": "", "disabledReason": null,
                "models": []
            }],
            "virtualModels": [{
                "displayId": "vm-x", "enable": true, "loadBalancingStrategy": 0,
                "fallbackStrategy": 0, "interfaceType": 0,
                "items": [{ "providerName": "x", "providerModelId": "nope", "enable": true, "cascadeDisabled": false }]
            }],
            "apiKeys": [], "settings": []
        });
        let (status, body) = send_json(&app, "POST", "/api/backup/import", &bad_ref.to_string()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body["msg"].as_str().unwrap().contains("引用的模型不存在"));

        let (providers, _, _, _) = count_rows(&db).await;
        assert_eq!(providers, 1, "导入失败后原数据应保留");
    })
    .await;
}

/// 备份导入应受会话保护：未认证（无 cookie）时 401。
#[tokio::test]
async fn backup_endpoints_require_auth() {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    let app = common::build_app(db, scheduler, log_tx);
    for (method, uri) in [
        ("GET", "/api/backup/export"),
        ("POST", "/api/backup/import"),
    ] {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{method} {uri}"
        );
    }
}

/// 设置值级校验在路由层复用设置页口径：Int 设置非法值应 400，库数据保留。
#[tokio::test]
async fn import_rejects_bad_setting_value() {
    temp_env::async_with_vars([(ENCRYPTION_KEY_ENV, Some(TEST_KEY))], async {
        let (app, db) = setup_app().await;
        create_provider(&app, "keep-set").await;

        let bad = serde_json::json!({
            "version": 1,
            "exportedAt": "2026-09-07T00:00:00Z",
            "providers": [],
            "virtualModels": [],
            "apiKeys": [],
            "settings": [{ "key": "max_consecutive_failures", "value": "abc", "type": "Int" }]
        });
        let (status, body) = send_json(&app, "POST", "/api/backup/import", &bad.to_string()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body["msg"].as_str().unwrap().contains("必须是有效的整数"),
            "{}",
            body
        );

        let (providers, _, _, _) = count_rows(&db).await;
        assert_eq!(providers, 1, "导入失败后原数据应保留");
    })
    .await;
}

/// 值级校验覆盖 max_consecutive_failures ≥ 1（设置页同口径）。
#[tokio::test]
async fn import_rejects_zero_max_consecutive_failures() {
    temp_env::async_with_vars([(ENCRYPTION_KEY_ENV, Some(TEST_KEY))], async {
        let (app, _db) = setup_app().await;

        let bad = serde_json::json!({
            "version": 1,
            "exportedAt": "2026-09-07T00:00:00Z",
            "providers": [],
            "virtualModels": [],
            "apiKeys": [],
            "settings": [{ "key": "max_consecutive_failures", "value": "0", "type": "Int" }]
        });
        let (status, body) = send_json(&app, "POST", "/api/backup/import", &bad.to_string()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body["msg"].as_str().unwrap().contains("正整数"), "{}", body);
    })
    .await;
}

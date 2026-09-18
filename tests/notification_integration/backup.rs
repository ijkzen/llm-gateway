use super::*;

// 备份集成：导出含明文凭据、缺字段不清配置、解不开时不导出降级凭据。

/// 导出含通知配置，且 appSecret 为明文（与其他凭据口径一致）。
#[tokio::test]
async fn backup_export_includes_plaintext_notification_config() {
    let (app, _db) = setup_app().await;
    save_config(&app, "ou_backup", "cli-secret-abcd1234").await;

    let (status, body) = call(&app, "GET", EXPORT_URI, None).await;
    assert_eq!(status, StatusCode::OK);

    let channels = body["data"]["notificationChannels"]
        .as_array()
        .expect("导出应带 notificationChannels 字段");
    assert_eq!(channels.len(), 1);
    assert_eq!(channels[0]["channel"], "feishu");
    assert_eq!(channels[0]["appId"], "cli_mock");
    assert_eq!(
        channels[0]["appSecret"], "cli-secret-abcd1234",
        "备份必须能脱离原库密钥恢复，凭据导出为明文"
    );
    assert_eq!(channels[0]["receiver"], "ou_backup");
    assert_eq!(channels[0]["receiverType"], "open_id");
    assert_eq!(channels[0]["enable"], true);
}

/// 导入**早于本特性**的备份（缺 notificationChannels 字段）→ 现有配置不被清掉。
#[tokio::test]
async fn backup_import_without_field_keeps_existing_config() {
    let (app, _db) = setup_app().await;
    save_config(&app, "ou_keep", "keep-secret-1234").await;

    let (status, body) = import_backup(&app, minimal_backup(None)).await;
    assert_eq!(status, StatusCode::OK, "缺字段的旧备份应能正常导入：{body}");

    let (_, config) = call(&app, "GET", URI, None).await;
    assert_eq!(config["data"]["configured"], true, "旧备份不该清掉配置");
    assert_eq!(config["data"]["appId"], "cli_mock");
    assert_eq!(config["data"]["receiver"], "ou_keep");
    assert_eq!(
        config["data"]["appSecretMasked"], "kee****1234",
        "凭据应原样保留"
    );
}

/// 导入带 notificationChannels 字段的备份 → 整体替换。
#[tokio::test]
async fn backup_import_with_field_replaces_config() {
    let (app, _db) = setup_app().await;
    save_config(&app, "ou_old", "old-secret-1234").await;

    let (status, body) = import_backup(
        &app,
        minimal_backup(Some(json!([{
            "channel": "feishu",
            "appId": "cli_restored",
            "appSecret": "new-secret-5678",
            "receiverType": "email",
            "receiver": "ops@example.com",
            "enable": false,
        }]))),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "导入应成功：{body}");

    let (_, config) = call(&app, "GET", URI, None).await;
    assert_eq!(config["data"]["appId"], "cli_restored");
    assert_eq!(config["data"]["receiver"], "ops@example.com");
    assert_eq!(config["data"]["receiverType"], "email");
    assert_eq!(config["data"]["enable"], false);
    assert_eq!(config["data"]["appSecretMasked"], "new****5678");
}

/// 通知配置的校验与业务口径一致：接收者类型非法 / 空字段一律 400。
#[tokio::test]
async fn backup_import_rejects_invalid_notification_config() {
    let (app, _db) = setup_app().await;

    for bad in [
        json!([{"channel": "feishu", "appId": "cli_x", "appSecret": "s-1234567",
                "receiverType": "chat_id", "receiver": "oc_x", "enable": true}]),
        json!([{"channel": "", "appId": "cli_x", "appSecret": "s-1234567",
                "receiverType": "open_id", "receiver": "ou_x", "enable": true}]),
    ] {
        let (status, body) = import_backup(&app, minimal_backup(Some(bad))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "非法配置应被拒：{body}");
    }
}

/// 密钥轮换导致渠道配置解不开时：导出**不产出降级空凭据**（那会是一份自己都
/// 导入不了的文件），改为整段不导出（`None` = 导入侧不触碰现有配置）。
#[tokio::test]
async fn backup_export_omits_undecryptable_channel() {
    let (app, db) = setup_app().await;
    notification_channel::ActiveModel {
        channel: Set("feishu".to_string()),
        config: Set("enc:v1:bm90LXJlYWwtY2lwaGVydGV4dA==".to_string()),
        enable: Set(true),
        last_error: Set(String::new()),
        last_sent_at: Set(None),
        updated_at: Set(chrono::Utc::now()),
    }
    .insert(&db)
    .await
    .unwrap();

    let (status, body) = call(&app, "GET", EXPORT_URI, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body["data"]["notificationChannels"].is_null(),
        "解不开的渠道不该导出降级空凭据：{body}"
    );
    assert_eq!(body["data"]["decryptFailures"], 1, "仍应计入解密失败计数");

    // 关键：自己导出的文件必须能通过自己的导入校验。
    let (status, imported) = import_backup(&app, body["data"].clone()).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "自己导出的文件必须能导入：{imported}"
    );
}

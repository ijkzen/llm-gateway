use super::*;

// 通知渠道配置读写：掩码展示与「缺省 appSecret 即不覆盖」语义。

/// 未配置时返回 `configured:false` 与空字段（不是 404——「尚未配置」是正常状态）。
#[tokio::test]
async fn unconfigured_channel_reports_not_configured() {
    let (app, _db) = setup_app().await;

    let (status, body) = call(&app, "GET", URI, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["code"], "0");
    assert_eq!(body["data"]["configured"], false);
    assert_eq!(body["data"]["appId"], "");
    assert_eq!(body["data"]["appSecretMasked"], "");
    assert_eq!(body["data"]["receiver"], "");
}

/// 保存后读取：appId 明文、appSecret 掩码、接收者明文。
#[tokio::test]
async fn save_then_read_masks_secret_and_returns_receiver() {
    let (app, _db) = setup_app().await;

    let (status, _) = call(
        &app,
        "PUT",
        URI,
        Some(json!({
            "appId": "cli_a1b2c3d4",
            "appSecret": "cli-secret-abcd1234",
            "receiverType": "open_id",
            "receiver": "ou_creator",
            "enable": true,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = call(&app, "GET", URI, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["configured"], true);
    assert_eq!(body["data"]["appId"], "cli_a1b2c3d4");
    assert_eq!(body["data"]["receiver"], "ou_creator");
    assert_eq!(body["data"]["receiverType"], "open_id");
    assert_eq!(body["data"]["enable"], true);
    // 掩码保留前 3 后 4，且绝不回显明文。
    assert_eq!(body["data"]["appSecretMasked"], "cli****1234");
    assert!(
        !body["data"].to_string().contains("cli-secret-abcd1234"),
        "响应不得包含明文 appSecret：{}",
        body["data"]
    );
}

/// 编辑时不提交 appSecret → 库中原凭据保持不变（掩码仍对应原 secret）。
#[tokio::test]
async fn update_without_secret_keeps_stored_credential() {
    let (app, _db) = setup_app().await;

    call(
        &app,
        "PUT",
        URI,
        Some(json!({
            "appId": "cli_orig",
            "appSecret": "cli-secret-abcd1234",
            "receiverType": "open_id",
            "receiver": "ou_first",
            "enable": true,
        })),
    )
    .await;

    // 只改接收者，不提交 appSecret。
    let (status, _) = call(
        &app,
        "PUT",
        URI,
        Some(json!({
            "appId": "cli_orig",
            "receiverType": "open_id",
            "receiver": "ou_second",
            "enable": true,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, body) = call(&app, "GET", URI, None).await;
    assert_eq!(body["data"]["receiver"], "ou_second");
    assert_eq!(
        body["data"]["appSecretMasked"], "cli****1234",
        "缺省 appSecret 不应清空或覆盖原凭据"
    );
}

/// 首次创建必须提供 appSecret（没有可沿用的原值）。
#[tokio::test]
async fn first_create_requires_secret() {
    let (app, _db) = setup_app().await;

    let (status, body) = call(
        &app,
        "PUT",
        URI,
        Some(json!({
            "appId": "cli_new",
            "receiverType": "open_id",
            "receiver": "ou_x",
            "enable": true,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_ne!(body["code"], "0");
}

/// 接收者类型只接受 open_id / email。
#[tokio::test]
async fn rejects_invalid_receiver_type() {
    let (app, _db) = setup_app().await;

    let (status, body) = call(
        &app,
        "PUT",
        URI,
        Some(json!({
            "appId": "cli_x",
            "appSecret": "secret-value-1234",
            "receiverType": "chat_id",
            "receiver": "oc_x",
            "enable": true,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_ne!(body["code"], "0");
}

/// appId 与接收者不得为空。
#[tokio::test]
async fn rejects_empty_app_id_and_receiver() {
    let (app, _db) = setup_app().await;

    let (status, _) = call(
        &app,
        "PUT",
        URI,
        Some(json!({
            "appId": "",
            "appSecret": "secret-value-1234",
            "receiverType": "open_id",
            "receiver": "ou_x",
            "enable": true,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = call(
        &app,
        "PUT",
        URI,
        Some(json!({
            "appId": "cli_x",
            "appSecret": "secret-value-1234",
            "receiverType": "open_id",
            "receiver": "",
            "enable": true,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

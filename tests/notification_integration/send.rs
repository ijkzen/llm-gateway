use super::*;

// 测试发送：对 mock 飞书的真实投递、凭据回落与错误码映射。

/// 测试发送：取 token 后按 receive_id_type 发一条文本消息，`content` 是 JSON 字符串。
#[tokio::test]
async fn test_send_delivers_text_message() {
    let _guard = SEND_LOCK.lock().await;
    let (base, mock) = spawn_feishu_mock().await;

    temp_env::async_with_vars(
        [("LLM_GATEWAY_FEISHU_BASE_URL", Some(base.as_str()))],
        async {
            let (app, _db) = setup_app().await;

            let (status, body) = call(
                &app,
                "POST",
                TEST_URI,
                Some(json!({
                    "appId": "cli_direct",
                    "appSecret": "cli-secret-abcd1234",
                    "receiverType": "open_id",
                    "receiver": "ou_tester",
                })),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "测试发送应成功：{body}");
            assert_eq!(body["code"], "0");

            let tokens = mock.token_bodies();
            assert_eq!(tokens.len(), 1, "应取一次 tenant_access_token");
            assert_eq!(tokens[0]["app_id"], "cli_direct");
            assert_eq!(tokens[0]["app_secret"], "cli-secret-abcd1234");

            let messages = mock.message_bodies();
            assert_eq!(messages.len(), 1);
            assert_eq!(messages[0]["receive_id"], "ou_tester");
            assert_eq!(messages[0]["msg_type"], "text");
            assert_eq!(
                mock.message_queries(),
                vec!["receive_id_type=open_id".to_string()]
            );

            // content 必须是 JSON **字符串**（飞书契约），解出来才是 {"text": "..."}。
            let content = messages[0]["content"]
                .as_str()
                .expect("content 必须是字符串而不是对象");
            let parsed: Value = serde_json::from_str(content).unwrap();
            assert!(
                parsed["text"].as_str().is_some_and(|t| !t.is_empty()),
                "content 里应带非空 text：{parsed}"
            );
        },
    )
    .await;
}

/// 测试发送不提交 appSecret 时回落库中已存值（刷新页面后直接测试）。
#[tokio::test]
async fn test_send_falls_back_to_stored_secret() {
    let _guard = SEND_LOCK.lock().await;
    let (base, mock) = spawn_feishu_mock().await;

    temp_env::async_with_vars(
        [("LLM_GATEWAY_FEISHU_BASE_URL", Some(base.as_str()))],
        async {
            let (app, _db) = setup_app().await;
            save_config(&app, "ou_stored", "stored-secret-9999").await;

            let (status, body) = call(
                &app,
                "POST",
                TEST_URI,
                Some(json!({
                    "appId": "cli_mock",
                    "receiverType": "open_id",
                    "receiver": "ou_stored",
                })),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "回落库中凭据应能发送：{body}");

            let tokens = mock.token_bodies();
            assert_eq!(
                tokens[0]["app_secret"], "stored-secret-9999",
                "缺省 appSecret 时应使用库中已存凭据"
            );
        },
    )
    .await;
}

/// 飞书业务错误码 → 中文提示（HTTP 200 + 非 0 code 也按失败处理）。
#[tokio::test]
async fn test_send_surfaces_feishu_error_code() {
    let _guard = SEND_LOCK.lock().await;
    let (base, mock) = spawn_feishu_mock().await;
    *mock.message_error_code.lock().unwrap() = Some(230027);

    temp_env::async_with_vars(
        [("LLM_GATEWAY_FEISHU_BASE_URL", Some(base.as_str()))],
        async {
            let (app, _db) = setup_app().await;
            save_config(&app, "ou_tester", "cli-secret-abcd1234").await;

            let (status, body) = call(
                &app,
                "POST",
                TEST_URI,
                Some(json!({
                    "appId": "cli_direct",
                    "appSecret": "cli-secret-abcd1234",
                    "receiverType": "open_id",
                    "receiver": "ou_tester",
                })),
            )
            .await;
            assert_ne!(status, StatusCode::OK);
            assert_ne!(body["code"], "0");
            let message = body["msg"].as_str().unwrap_or_default();
            assert!(
                message.contains("权限"),
                "230027 应映射为权限相关中文提示，实际：{message}"
            );

            // 失败结果落库：GET 应能看到 lastError。
            let (_, config) = call(&app, "GET", URI, None).await;
            assert!(
                config["data"]["lastError"]
                    .as_str()
                    .is_some_and(|e| !e.is_empty()),
                "发送失败应记录 lastError：{config}"
            );
        },
    )
    .await;
}

/// 发送成功回写 lastSentAt，且 lastError 清空。
#[tokio::test]
async fn test_send_records_success() {
    let _guard = SEND_LOCK.lock().await;
    let (base, _mock) = spawn_feishu_mock().await;

    temp_env::async_with_vars(
        [("LLM_GATEWAY_FEISHU_BASE_URL", Some(base.as_str()))],
        async {
            let (app, _db) = setup_app().await;
            save_config(&app, "ou_tester", "cli-secret-abcd1234").await;

            call(
                &app,
                "POST",
                TEST_URI,
                Some(json!({
                    "appId": "cli_direct",
                    "appSecret": "cli-secret-abcd1234",
                    "receiverType": "open_id",
                    "receiver": "ou_tester",
                })),
            )
            .await;

            let (_, config) = call(&app, "GET", URI, None).await;
            assert!(
                !config["data"]["lastSentAt"].is_null(),
                "发送成功应记录 lastSentAt：{config}"
            );
            assert_eq!(config["data"]["lastError"], "");
        },
    )
    .await;
}

/// 本地校验先行：缺少全部凭据时直接报错，不发起任何请求。
#[tokio::test]
async fn test_send_rejects_missing_credentials_without_calling_feishu() {
    let _guard = SEND_LOCK.lock().await;
    let (base, mock) = spawn_feishu_mock().await;

    temp_env::async_with_vars(
        [("LLM_GATEWAY_FEISHU_BASE_URL", Some(base.as_str()))],
        async {
            let (app, _db) = setup_app().await;

            let (status, body) = call(
                &app,
                "POST",
                TEST_URI,
                Some(json!({
                    "appId": "cli_direct",
                    "receiverType": "open_id",
                    "receiver": "ou_tester",
                })),
            )
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_ne!(body["code"], "0");
            assert!(mock.token_bodies().is_empty(), "本地校验失败不应请求飞书");
        },
    )
    .await;
}

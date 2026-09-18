use super::*;

// 扫码创建：begin 二维码内容、poll 状态流转、interval 节流。

/// begin 返回会话 id 与二维码内容（= `verification_uri_complete` 原样）。
#[tokio::test]
async fn registration_begin_returns_qr_url() {
    let _guard = SEND_LOCK.lock().await;
    let (base, mock) = spawn_feishu_mock().await;

    temp_env::async_with_vars(
        [("LLM_GATEWAY_FEISHU_BASE_URL", Some(base.as_str()))],
        async {
            let (app, _db) = setup_app().await;

            let (status, body) = call(&app, "POST", REGISTER_URI, None).await;
            assert_eq!(status, StatusCode::OK, "begin 应成功：{body}");
            assert!(
                body["data"]["sessionId"]
                    .as_str()
                    .is_some_and(|s| !s.is_empty()),
                "应返回 sessionId：{body}"
            );
            assert_eq!(
                body["data"]["qrUrl"], "https://open.feishu.cn/page/launcher?user_code=ABCD-1234",
                "二维码内容必须是 verification_uri_complete 原样（已含 user_code）"
            );
            assert_eq!(body["data"]["expiresIn"], 3600);

            // begin 请求体带齐飞书要求的四个参数。
            let begin_body = mock.registration_bodies.lock().unwrap()[0].clone();
            for expected in [
                "action=begin",
                "archetype=PersonalAgent",
                "auth_method=client_secret",
            ] {
                assert!(
                    begin_body.contains(expected),
                    "begin 请求体应含 {expected}，实际：{begin_body}"
                );
            }
        },
    )
    .await;
}

/// 轮询：HTTP 400 + authorization_pending → pending；脚本给成功 → 回填凭据。
#[tokio::test]
async fn registration_polls_until_success() {
    let _guard = SEND_LOCK.lock().await;
    let (base, mock) = spawn_feishu_mock().await;

    temp_env::async_with_vars(
        [("LLM_GATEWAY_FEISHU_BASE_URL", Some(base.as_str()))],
        async {
            let (app, _db) = setup_app().await;
            let (_, begun) = call(&app, "POST", REGISTER_URI, None).await;
            let session_id = begun["data"]["sessionId"].as_str().unwrap().to_string();
            let poll_uri = format!("{REGISTER_URI}/{session_id}");

            // 队列空 → mock 返回 HTTP 400 + authorization_pending。
            let (status, body) = call(&app, "GET", &poll_uri, None).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(
                body["data"]["status"], "pending",
                "400 + authorization_pending 必须解析为 pending（不能当硬错误）：{body}"
            );

            mock.queue_poll(json!({
                "client_id": "cli_created",
                "client_secret": "created-secret-7777",
                "user_info": {"open_id": "ou_scanner", "tenant_brand": "feishu"},
            }));

            // 等过 interval（mock 给 1s）后再轮询，拿到成功结果。
            tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
            let (_, body) = call(&app, "GET", &poll_uri, None).await;
            assert_eq!(body["data"]["status"], "success", "应拿到成功状态：{body}");
            assert_eq!(body["data"]["appId"], "cli_created");
            assert_eq!(body["data"]["appSecret"], "created-secret-7777");
            assert_eq!(body["data"]["receiver"], "ou_scanner");
            assert_eq!(body["data"]["receiverType"], "open_id");
        },
    )
    .await;
}

/// interval 内连续轮询：后端不重复打飞书（前端 2-3s 的节奏不会打爆上游）。
#[tokio::test]
async fn registration_poll_is_throttled_by_interval() {
    let _guard = SEND_LOCK.lock().await;
    let (base, mock) = spawn_feishu_mock().await;

    temp_env::async_with_vars(
        [("LLM_GATEWAY_FEISHU_BASE_URL", Some(base.as_str()))],
        async {
            let (app, _db) = setup_app().await;
            let (_, begun) = call(&app, "POST", REGISTER_URI, None).await;
            let session_id = begun["data"]["sessionId"].as_str().unwrap().to_string();
            let poll_uri = format!("{REGISTER_URI}/{session_id}");

            for _ in 0..3 {
                let (_, body) = call(&app, "GET", &poll_uri, None).await;
                assert_eq!(body["data"]["status"], "pending");
            }

            assert_eq!(
                mock.poll_count(),
                1,
                "interval（5s）内的重复轮询不应打到飞书"
            );
        },
    )
    .await;
}

/// 未知/过期会话 id → expired（正常状态，不是错误）。
#[tokio::test]
async fn registration_unknown_session_reports_expired() {
    let _guard = SEND_LOCK.lock().await;
    let (base, _mock) = spawn_feishu_mock().await;

    temp_env::async_with_vars(
        [("LLM_GATEWAY_FEISHU_BASE_URL", Some(base.as_str()))],
        async {
            let (app, _db) = setup_app().await;

            let (status, body) = call(
                &app,
                "GET",
                &format!("{REGISTER_URI}/no-such-session"),
                None,
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(body["data"]["status"], "expired");
        },
    )
    .await;
}

/// slow_down / access_denied / expired_token 各自的落点。
#[tokio::test]
async fn registration_surfaces_terminal_and_backoff_statuses() {
    let _guard = SEND_LOCK.lock().await;
    let (base, mock) = spawn_feishu_mock().await;

    temp_env::async_with_vars(
        [("LLM_GATEWAY_FEISHU_BASE_URL", Some(base.as_str()))],
        async {
            let (app, _db) = setup_app().await;
            let (_, begun) = call(&app, "POST", REGISTER_URI, None).await;
            let session_id = begun["data"]["sessionId"].as_str().unwrap().to_string();
            let poll_uri = format!("{REGISTER_URI}/{session_id}");

            mock.queue_poll(json!({"error": "slow_down"}));
            let (_, body) = call(&app, "GET", &poll_uri, None).await;
            assert_eq!(body["data"]["status"], "slow_down");

            // 重新发起会话（单槽位：新 begin 替换旧的）验证拒绝路径。
            let (_, begun) = call(&app, "POST", REGISTER_URI, None).await;
            let session_id = begun["data"]["sessionId"].as_str().unwrap().to_string();
            let poll_uri = format!("{REGISTER_URI}/{session_id}");
            mock.queue_poll(json!({"error": "access_denied"}));
            let (_, body) = call(&app, "GET", &poll_uri, None).await;
            assert_eq!(body["data"]["status"], "denied");

            let (_, begun) = call(&app, "POST", REGISTER_URI, None).await;
            let session_id = begun["data"]["sessionId"].as_str().unwrap().to_string();
            let poll_uri = format!("{REGISTER_URI}/{session_id}");
            mock.queue_poll(json!({"error": "expired_token"}));
            let (_, body) = call(&app, "GET", &poll_uri, None).await;
            assert_eq!(body["data"]["status"], "expired");
        },
    )
    .await;
}

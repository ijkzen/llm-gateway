use super::*;

#[tokio::test]
async fn sensenova_usage_full_chain_with_rotation_writeback() {
    let (mock_base, mock) = spawn_sensenova_mock().await;
    temp_env::async_with_vars([(OVERRIDE_ENV, Some(mock_base.as_str()))], async {
        let (app, db) = setup_app_with_db().await;
        let id = create_provider(
            &app,
            "SenseNova-订阅",
            "https://token.sensenova.cn/v1",
            r#"{"usage": true, "usage_type": 0, "refresh_token": "rt-1"}"#,
        )
        .await;

        let (status, body) = send(&app, "GET", &format!("/api/providers/{id}/usage"), None).await;
        assert_eq!(status, StatusCode::OK, "查询失败：{body}");
        let data = &body["data"];
        assert_eq!(data["kind"], "quota");
        assert_eq!(data["plan"], "Free Plan");
        // 每池独立窗口，label = 池名。
        let windows = data["windows"].as_array().unwrap();
        assert_eq!(windows.len(), 3);
        assert_eq!(windows[0]["window"], "five_hour");
        assert_eq!(windows[0]["label"], "通用积分池");
        assert_eq!(windows[0]["used"], 33586.3);
        assert_eq!(windows[0]["limit"], 60000.0);
        assert_eq!(windows[0]["unit"], "积分");
        assert_eq!(windows[1]["window"], "weekly");
        assert_eq!(windows[1]["label"], "通用积分池");
        assert_eq!(windows[2]["label"], "Flash-Lite积分池");
        assert_eq!(windows[2]["remainingPercent"], 0.01);
        // pool-usage 用续期得到的 access_token 调用。
        assert_eq!(
            mock.last_auth.lock().unwrap().as_deref(),
            Some("Bearer at-1")
        );

        // 轮换出的新 refresh_token 已写回 provider extra。
        let model = provider::Entity::find_by_id(id as i32)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        let extra: Value = serde_json::from_str(&model.extra).unwrap();
        assert_eq!(extra["refresh_token"], "rt-new");

        // 绕过缓存再查一次：续期用的是写回后的 rt-new（凭据链不断）。
        let (status, _) = send(
            &app,
            "GET",
            &format!("/api/providers/{id}/usage?refresh=1"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            mock.last_renewal_form
                .lock()
                .unwrap()
                .as_deref()
                .unwrap()
                .contains("refresh_token=rt-new")
        );
    })
    .await;
}

#[tokio::test]
async fn sensenova_platform_host_dispatch_and_missing_credential() {
    let (mock_base, _mock) = spawn_sensenova_mock().await;
    temp_env::async_with_vars([(OVERRIDE_ENV, Some(mock_base.as_str()))], async {
        let app = setup_app().await;
        // 控制台域名同样分发到 SenseNova fetcher。
        let id = create_provider(
            &app,
            "SenseNova-控制台域",
            "https://platform.sensenova.cn/v1",
            // 无 refresh_token 也无 username/password → 缺用户可维护凭据 username。
            r#"{"usage": true, "usage_type": 0}"#,
        )
        .await;
        let (status, body) = send(&app, "GET", &format!("/api/providers/{id}/usage"), None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body["msg"].as_str().unwrap().contains("username"));
    })
    .await;
}

#[tokio::test]
async fn sensenova_invalid_grant_maps_to_auth_error() {
    // 续期端点返回 200 + error 字段（refresh_token 失效）→ 走鉴权失败链路。
    let (mock_base, _mock) = spawn_sensenova_mock_with_renewal(serde_json::json!({
        "error": "invalid_grant",
        "error_description": "The refresh token is invalid"
    }))
    .await;
    temp_env::async_with_vars([(OVERRIDE_ENV, Some(mock_base.as_str()))], async {
        let app = setup_app().await;
        let id = create_provider(
            &app,
            "SenseNova-失效",
            "https://token.sensenova.cn/v1",
            r#"{"usage": true, "usage_type": 0, "refresh_token": "rt-dead"}"#,
        )
        .await;
        let (status, body) = send(&app, "GET", &format!("/api/providers/{id}/usage"), None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["msg"], "用量查询凭据无效或已过期");
    })
    .await;
}

/// 场景 1：refresh_token 失效（invalid_grant）且有账号密码 → 自动登录 → 写回新
/// refresh_token → 用新 token 续期查询成功。
#[tokio::test]
async fn sensenova_invalid_grant_self_heals_via_login_and_writes_back() {
    let _guard = SENSENOVA_LOGIN_LOCK.lock().await;
    llm_gateway::usage::fetchers::sensenova::reset_login_failures();
    let (mock_base, mock) = spawn_sensenova_login_mock().await;
    temp_env::async_with_vars([(OVERRIDE_ENV, Some(mock_base.as_str()))], async {
        let (app, db) = setup_app_with_db().await;
        let id = create_provider(
            &app,
            "SenseNova-自愈",
            "https://token.sensenova.cn/v1",
            r#"{"usage": true, "usage_type": 0, "refresh_token": "rt-dead", "username": "ijkzen", "password": "pw"}"#,
        )
        .await;

        let (status, body) = send(&app, "GET", &format!("/api/providers/{id}/usage"), None).await;
        assert_eq!(status, StatusCode::OK, "查询失败：{body}");
        // 登录确实发生了一次。
        assert_eq!(mock.login_hits.load(Ordering::SeqCst), 1);
        // 登录请求体包含 username 与 5 段 JWE 密码。
        let login_body = mock.last_login_body.lock().unwrap().clone().unwrap();
        let login_json: Value = serde_json::from_str(&login_body).unwrap();
        assert_eq!(login_json["username"], "ijkzen");
        assert_eq!(login_json["is_encrypt"], true);
        let jwe = login_json["password"].as_str().unwrap();
        assert_eq!(jwe.split('.').count(), 5, "密码应为 5 段 JWE");

        // 新 refresh_token 已写回 provider extra。
        let model = provider::Entity::find_by_id(id as i32)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        let extra: Value = serde_json::from_str(&model.extra).unwrap();
        assert_eq!(extra["refresh_token"], "rt-logged-in");
        assert_eq!(extra["username"], "ijkzen", "其余键保留");
        assert_eq!(extra["password"], "pw");
    })
    .await;
}

/// 场景 2：refresh_token 缺失、只有账号密码 → 直接登录引导写回并查询成功。
#[tokio::test]
async fn sensenova_missing_refresh_token_logs_in_with_credentials() {
    let _guard = SENSENOVA_LOGIN_LOCK.lock().await;
    llm_gateway::usage::fetchers::sensenova::reset_login_failures();
    let (mock_base, mock) = spawn_sensenova_login_mock().await;
    temp_env::async_with_vars([(OVERRIDE_ENV, Some(mock_base.as_str()))], async {
        let (app, db) = setup_app_with_db().await;
        let id = create_provider(
            &app,
            "SenseNova-仅账号密码",
            "https://token.sensenova.cn/v1",
            r#"{"usage": true, "usage_type": 0, "username": "ijkzen", "password": "pw"}"#,
        )
        .await;

        let (status, body) = send(&app, "GET", &format!("/api/providers/{id}/usage"), None).await;
        assert_eq!(status, StatusCode::OK, "查询失败：{body}");
        assert_eq!(mock.login_hits.load(Ordering::SeqCst), 1);
        let model = provider::Entity::find_by_id(id as i32)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        let extra: Value = serde_json::from_str(&model.extra).unwrap();
        assert_eq!(extra["refresh_token"], "rt-logged-in");
    })
    .await;
}

/// 场景 3：登录失败（账号或密码错误）→ Auth → 400「用量查询凭据无效或已过期」。
#[tokio::test]
async fn sensenova_login_failure_maps_to_auth_error() {
    let _guard = SENSENOVA_LOGIN_LOCK.lock().await;
    llm_gateway::usage::fetchers::sensenova::reset_login_failures();
    let (mock_base, mock) = spawn_sensenova_login_mock().await;
    temp_env::async_with_vars([(OVERRIDE_ENV, Some(mock_base.as_str()))], async {
        let app = setup_app().await;
        *mock.login_should_fail.lock().unwrap() = true;
        let id = create_provider(
            &app,
            "SenseNova-登录失败",
            "https://token.sensenova.cn/v1",
            r#"{"usage": true, "usage_type": 0, "refresh_token": "rt-dead", "username": "ijkzen", "password": "wrong"}"#,
        )
        .await;
        let (status, body) = send(&app, "GET", &format!("/api/providers/{id}/usage"), None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["msg"], "用量查询凭据无效或已过期");
    })
    .await;
}

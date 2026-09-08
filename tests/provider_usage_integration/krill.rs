use super::*;

#[tokio::test]
async fn krill_valid_jwt_queries_balance_without_login() {
    let (mock_base, mock) = spawn_krill_mock(
        vec![(StatusCode::OK, krill_balance_reply())],
        krill_login_reply(),
    )
    .await;
    temp_env::async_with_vars([(OVERRIDE_ENV, Some(mock_base.as_str()))], async {
        let app = setup_app().await;
        let id = create_provider(
            &app,
            "Krill-按量",
            "https://api-slb.krill-ai.net/v1",
            r#"{"usage":true,"usage_type":0,"email":"u@example.com","password":"pw","jwt":"jwt-old"}"#,
        )
        .await;

        let (status, body) = send(&app, "GET", &format!("/api/providers/{id}/usage"), None).await;
        assert_eq!(status, StatusCode::OK, "查询失败：{body}");
        assert_eq!(body["data"]["kind"], "balance");
        assert_eq!(body["data"]["balances"][0]["amount"], 25.0);
        assert_eq!(mock.subscription_hits.load(Ordering::SeqCst), 1);
        assert_eq!(mock.login_hits.load(Ordering::SeqCst), 0);
        assert_eq!(
            mock.auth_headers.lock().unwrap().as_slice(),
            &[Some("Bearer jwt-old".to_string())]
        );
    })
    .await;
}

#[tokio::test]
async fn krill_missing_jwt_logs_in_and_writes_encrypted_token() {
    use llm_gateway::crypto::ENCRYPTION_KEY_ENV;

    let (mock_base, mock) = spawn_krill_mock(
        vec![(StatusCode::OK, krill_balance_reply())],
        krill_login_reply(),
    )
    .await;
    temp_env::async_with_vars(
        [
            (OVERRIDE_ENV, Some(mock_base.as_str())),
            (ENCRYPTION_KEY_ENV, Some("krill-test-key")),
        ],
        async {
            let (app, db) = setup_app_with_db().await;
            let id = create_provider(
                &app,
                "Krill-首次登录",
                "https://api.krill-ai.net/v1",
                r#"{"usage":true,"usage_type":0,"email":"u@example.com","password":"pw","jwt":"","keep":"value"}"#,
            )
            .await;

            let (status, body) =
                send(&app, "GET", &format!("/api/providers/{id}/usage"), None).await;
            assert_eq!(status, StatusCode::OK, "查询失败：{body}");
            assert_eq!(mock.login_hits.load(Ordering::SeqCst), 1);
            assert_eq!(mock.subscription_hits.load(Ordering::SeqCst), 1);
            assert_eq!(
                mock.auth_headers.lock().unwrap().as_slice(),
                &[Some("Bearer jwt-new".to_string())]
            );

            let row = provider::Entity::find_by_id(id as i32)
                .one(&db)
                .await
                .unwrap()
                .unwrap();
            assert!(row.extra.starts_with("enc:v1:"));
            let extra: Value =
                serde_json::from_str(&llm_gateway::crypto::decrypt(&row.extra).unwrap()).unwrap();
            assert_eq!(extra["jwt"], "jwt-new");
            assert_eq!(extra["password"], "pw");
            assert_eq!(extra["keep"], "value");
        },
    )
    .await;
}

#[tokio::test]
async fn krill_auth_failure_logs_in_once_and_retries_once() {
    let auth_error = serde_json::json!({ "success": false, "code": 401, "message": "expired" });
    let (mock_base, mock) = spawn_krill_mock(
        vec![
            (StatusCode::OK, auth_error),
            (StatusCode::OK, krill_balance_reply()),
        ],
        krill_login_reply(),
    )
    .await;
    temp_env::async_with_vars([(OVERRIDE_ENV, Some(mock_base.as_str()))], async {
        let app = setup_app().await;
        let id = create_provider(
            &app,
            "Krill-JWT过期",
            "https://api.cdn-krill-ai.com/v1",
            r#"{"usage":true,"usage_type":0,"email":"u@example.com","password":"pw","jwt":"jwt-old"}"#,
        )
        .await;

        let (status, body) = send(&app, "GET", &format!("/api/providers/{id}/usage"), None).await;
        assert_eq!(status, StatusCode::OK, "查询失败：{body}");
        assert_eq!(mock.login_hits.load(Ordering::SeqCst), 1);
        assert_eq!(mock.subscription_hits.load(Ordering::SeqCst), 2);
        assert_eq!(
            mock.auth_headers.lock().unwrap().as_slice(),
            &[
                Some("Bearer jwt-old".to_string()),
                Some("Bearer jwt-new".to_string())
            ]
        );
    })
    .await;
}

#[tokio::test]
async fn krill_upstream_error_does_not_login() {
    let (mock_base, mock) = spawn_krill_mock(
        vec![(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "message": "down" }),
        )],
        krill_login_reply(),
    )
    .await;
    temp_env::async_with_vars([(OVERRIDE_ENV, Some(mock_base.as_str()))], async {
        let app = setup_app().await;
        let id = create_provider(
            &app,
            "Krill-上游故障",
            "https://api-slb.krill-ai.net/v1",
            r#"{"usage":true,"usage_type":0,"email":"u@example.com","password":"pw","jwt":"jwt-old"}"#,
        )
        .await;

        let (status, _) = send(&app, "GET", &format!("/api/providers/{id}/usage"), None).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(mock.subscription_hits.load(Ordering::SeqCst), 1);
        assert_eq!(mock.login_hits.load(Ordering::SeqCst), 0);
    })
    .await;
}

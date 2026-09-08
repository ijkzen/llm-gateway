use super::*;

#[tokio::test]
async fn usage_not_found_provider() {
    let app = setup_app().await;
    let (status, body) = send(&app, "GET", "/api/providers/999/usage", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "NOT_FOUND");
}

#[tokio::test]
async fn usage_not_enabled() {
    let app = setup_app().await;
    let id = create_provider(&app, "DS-无用量", "https://api.deepseek.com", "{}").await;
    let (status, body) = send(&app, "GET", &format!("/api/providers/{id}/usage"), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["msg"].as_str().unwrap().contains("未开启用量查询"));
}

#[tokio::test]
async fn usage_unsupported_host() {
    let app = setup_app().await;
    // 手动开启 usage 但 host 不在支持列表（自定义网关）。
    let id = create_provider(
        &app,
        "自定义网关",
        "https://gateway.example.com/v1",
        r#"{"usage": true, "usage_type": 1}"#,
    )
    .await;
    let (status, body) = send(&app, "GET", &format!("/api/providers/{id}/usage"), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["msg"].as_str().unwrap().contains("暂不支持"));
}

#[tokio::test]
async fn usage_success_cache_and_refresh() {
    let (mock_base, counter) = spawn_mock().await;
    temp_env::async_with_vars([(OVERRIDE_ENV, Some(mock_base.as_str()))], async {
        let app = setup_app().await;
        let id = create_provider(
            &app,
            "DeepSeek-用量",
            "https://api.deepseek.com",
            r#"{"usage": true, "usage_type": 0}"#,
        )
        .await;

        let (status, body) = send(&app, "GET", &format!("/api/providers/{id}/usage"), None).await;
        assert_eq!(status, StatusCode::OK, "查询失败：{body}");
        let data = &body["data"];
        assert_eq!(data["kind"], "balance");
        assert_eq!(data["providerId"], id);
        assert!(data["fetchedAt"].as_str().is_some());
        assert_eq!(data["balances"][0]["label"], "余额（CNY）");
        assert_eq!(data["balances"][0]["amount"], 110.0);
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        // 60s 缓存内第二次请求不打上游。
        let (status, _) = send(&app, "GET", &format!("/api/providers/{id}/usage"), None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        // ?refresh=1 绕过缓存。
        let (status, _) = send(
            &app,
            "GET",
            &format!("/api/providers/{id}/usage?refresh=1"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(counter.load(Ordering::SeqCst), 2);

        // 更新 provider 后缓存失效，下次查询重新打上游。
        let (status, _) = send(
            &app,
            "PUT",
            &format!("/api/providers/{id}"),
            Some(r#"{"name": "DeepSeek-用量改"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = send(&app, "GET", &format!("/api/providers/{id}/usage"), None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    })
    .await;
}

#[tokio::test]
async fn usage_db_cache_stale_after_10min_refetches() {
    let (mock_base, counter) = spawn_mock().await;
    temp_env::async_with_vars([(OVERRIDE_ENV, Some(mock_base.as_str()))], async {
        let (app, db) = setup_app_with_db().await;
        let id = create_provider(
            &app,
            "DeepSeek-过期",
            "https://api.deepseek.com",
            r#"{"usage": true, "usage_type": 0}"#,
        )
        .await;

        let (status, body) = send(&app, "GET", &format!("/api/providers/{id}/usage"), None).await;
        assert_eq!(status, StatusCode::OK, "查询失败：{body}");
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        // 把缓存行回拨到 11 分钟前 → 视为过期，下次请求需要重新抓取。
        let row = usage_cache::Entity::find()
            .filter(usage_cache::Column::ProviderId.eq(id))
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        let mut active: usage_cache::ActiveModel = row.into();
        active.fetched_at = Set(chrono::Utc::now() - chrono::Duration::minutes(11));
        active.update(&db).await.unwrap();

        let (status, _) = send(&app, "GET", &format!("/api/providers/{id}/usage"), None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(counter.load(Ordering::SeqCst), 2);

        // 重取后落库刷新 → 10 分钟内再次请求直接打缓存。
        let (status, _) = send(&app, "GET", &format!("/api/providers/{id}/usage"), None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    })
    .await;
}

#[tokio::test]
async fn refresh_all_usage_only_writes_usage_enabled_providers() {
    let (mock_base, counter) = spawn_mock().await;
    temp_env::async_with_vars([(OVERRIDE_ENV, Some(mock_base.as_str()))], async {
        let (db, _scheduler, _log_tx) = common::setup_db_and_scheduler().await;
        let now = chrono::Utc::now();
        // 已禁用的用量供应商也要被监测（停用后恢复依赖持续刷新）。
        let usage_provider = provider::ActiveModel {
            name: Set("DS-已禁用".to_string()),
            enable: Set(false),
            base_url: Set("https://api.deepseek.com".to_string()),
            api_key: Set(llm_gateway::crypto::encrypt("sk-x")),
            custom_header: Set("{}".to_string()),
            protocol_type: Set(0),
            billing_mode: Set(0),
            extra: Set(r#"{"usage": true, "usage_type": 0}"#.to_string()),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap()
        .id;
        provider::ActiveModel {
            name: Set("DS-无用量".to_string()),
            enable: Set(true),
            base_url: Set("https://api.deepseek.com".to_string()),
            api_key: Set(llm_gateway::crypto::encrypt("sk-x")),
            custom_header: Set("{}".to_string()),
            protocol_type: Set(0),
            billing_mode: Set(0),
            extra: Set("{}".to_string()),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let ok = llm_gateway::usage::persist::refresh_all_usage(&db)
            .await
            .unwrap();
        assert_eq!(ok, 1);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        assert!(
            usage_cache::Entity::find()
                .filter(usage_cache::Column::ProviderId.eq(usage_provider))
                .one(&db)
                .await
                .unwrap()
                .is_some()
        );
        assert_eq!(usage_cache::Entity::find().count(&db).await.unwrap(), 1);
    })
    .await;
}

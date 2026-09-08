use super::*;

/// provider 开启网络代理时，用量抓取经代理转发。
#[tokio::test]
async fn usage_goes_through_provider_proxy() {
    let (mock_base, target_counter) = spawn_mock().await;
    let (proxy_addr, connect_counter) = spawn_forward_proxy_usage().await;
    temp_env::async_with_vars([(OVERRIDE_ENV, Some(mock_base.as_str()))], async {
        let app = setup_app().await;
        let body = serde_json::json!({
            "name": "DeepSeek-代理",
            "enable": true,
            "baseUrl": "https://api.deepseek.com",
            "apiKey": "sk-usage-proxy",
            "protocolType": 0,
            "billingMode": 0,
            "customHeader": "{}",
            "extra": r#"{"usage": true, "usage_type": 0}"#,
            "proxyEnabled": true,
            "proxyAddr": proxy_addr,
        })
        .to_string();
        let (status, body) = send(&app, "POST", "/api/providers", Some(&body)).await;
        assert_eq!(status, StatusCode::CREATED, "创建失败：{body}");
        let id = body["data"]["id"].as_i64().unwrap();

        let (status, resp) = send(&app, "GET", &format!("/api/providers/{id}/usage"), None).await;
        assert_eq!(status, StatusCode::OK, "查询失败：{resp}");
        assert_eq!(resp["data"]["kind"], "balance");
        assert_eq!(
            connect_counter.load(Ordering::SeqCst),
            1,
            "用量抓取应经代理一次"
        );
        assert_eq!(
            target_counter.load(Ordering::SeqCst),
            1,
            "目标 mock 应收到 1 次请求"
        );
    })
    .await;
}

/// provider 未开启代理时用量抓取仍直连（不引入代理）。
#[tokio::test]
async fn usage_direct_without_proxy_still_works() {
    let (mock_base, target_counter) = spawn_mock().await;
    temp_env::async_with_vars([(OVERRIDE_ENV, Some(mock_base.as_str()))], async {
        let app = setup_app().await;
        let id = create_provider(
            &app,
            "DeepSeek-直连",
            "https://api.deepseek.com",
            r#"{"usage": true, "usage_type": 0}"#,
        )
        .await;
        let (status, body) = send(&app, "GET", &format!("/api/providers/{id}/usage"), None).await;
        assert_eq!(status, StatusCode::OK, "查询失败：{body}");
        assert_eq!(target_counter.load(Ordering::SeqCst), 1);
    })
    .await;
}

use super::*;

// 自动转换触发通知：五类自动转换发消息、手动启停与停用开关不发。

/// 真实走一次额度门控停用 → 飞书收到自动停用消息，正文含供应商名与原因。
#[tokio::test]
async fn auto_transition_sends_notification() {
    let _guard = SEND_LOCK.lock().await;
    let (base, mock) = spawn_feishu_mock().await;

    temp_env::async_with_vars(
        [("LLM_GATEWAY_FEISHU_BASE_URL", Some(base.as_str()))],
        async {
            let (app, db) = setup_app().await;
            save_config(&app, "ou_watcher", "cli-secret-abcd1234").await;
            let seeded = seed_subscription_provider(&db, "订阅供应商A").await;

            llm_gateway::usage::persist::apply_usage_gate(
                &db,
                &seeded,
                &exhausted_quota(seeded.id),
            )
            .await
            .unwrap();

            wait_for_messages(&mock, 1).await;
            let messages = mock.message_bodies();
            assert_eq!(messages.len(), 1, "自动停用应发出恰好一条通知");
            let content = messages[0]["content"].as_str().unwrap();
            let text = serde_json::from_str::<Value>(content).unwrap()["text"]
                .as_str()
                .unwrap()
                .to_string();
            assert!(text.contains("【供应商自动停用】"), "标题不符：{text}");
            assert!(text.contains("订阅供应商A"), "缺供应商名：{text}");
            assert!(text.contains("订阅额度耗尽"), "原因不符：{text}");
            assert_eq!(messages[0]["receive_id"], "ou_watcher");

            // 通知发送结果同样回写（面板据此展示最近一次发送情况）。
            let (_, config) = call(&app, "GET", URI, None).await;
            assert!(
                !config["data"]["lastSentAt"].is_null(),
                "自动通知成功应回写 lastSentAt：{config}"
            );
            assert_eq!(config["data"]["lastError"], "");
        },
    )
    .await;
}

/// 额度恢复自动启用同样发通知（自动启用的另一条路径）。
#[tokio::test]
async fn quota_recovery_sends_notification() {
    let _guard = SEND_LOCK.lock().await;
    let (base, mock) = spawn_feishu_mock().await;

    temp_env::async_with_vars(
        [("LLM_GATEWAY_FEISHU_BASE_URL", Some(base.as_str()))],
        async {
            let (app, db) = setup_app().await;
            save_config(&app, "ou_watcher", "cli-secret-abcd1234").await;
            let seeded = seed_subscription_provider(&db, "订阅供应商B").await;

            llm_gateway::usage::persist::apply_usage_gate(
                &db,
                &seeded,
                &exhausted_quota(seeded.id),
            )
            .await
            .unwrap();
            llm_gateway::usage::persist::apply_usage_gate(&db, &seeded, &healthy_quota(seeded.id))
                .await
                .unwrap();

            wait_for_messages(&mock, 2).await;
            let texts: Vec<String> = mock
                .message_bodies()
                .iter()
                .map(|m| {
                    serde_json::from_str::<Value>(m["content"].as_str().unwrap_or_default())
                        .ok()
                        .and_then(|v| v["text"].as_str().map(str::to_string))
                        .unwrap_or_default()
                })
                .collect();
            assert_eq!(texts.len(), 2, "停用与恢复各应发一条，实收：{texts:?}");

            // 两条通知是各自 spawn 出去的，到达顺序不保证——按内容断言而非按下标。
            let disable = texts
                .iter()
                .find(|text| text.contains("【供应商自动停用】"))
                .unwrap_or_else(|| panic!("缺自动停用通知：{texts:?}"));
            assert!(disable.contains("订阅额度耗尽"), "原因不符：{disable}");

            let recovery = texts
                .iter()
                .find(|text| text.contains("【供应商自动启用】"))
                .unwrap_or_else(|| panic!("缺自动启用通知：{texts:?}"));
            assert!(recovery.contains("订阅额度恢复"), "原因不符：{recovery}");
        },
    )
    .await;
}

/// 管理员手动停用**不发**通知：手动停用 A + 自动停用 B → 恰好只有 B 那一条。
#[tokio::test]
async fn manual_toggle_does_not_notify() {
    let _guard = SEND_LOCK.lock().await;
    let (base, mock) = spawn_feishu_mock().await;

    temp_env::async_with_vars(
        [("LLM_GATEWAY_FEISHU_BASE_URL", Some(base.as_str()))],
        async {
            let (app, db) = setup_app().await;
            save_config(&app, "ou_watcher", "cli-secret-abcd1234").await;

            let manual = seed_subscription_provider(&db, "手动停用供应商").await;
            llm_gateway::availability::disable_manual(&db, manual.id)
                .await
                .unwrap();

            let auto = seed_subscription_provider(&db, "自动停用供应商").await;
            llm_gateway::usage::persist::apply_usage_gate(&db, &auto, &exhausted_quota(auto.id))
                .await
                .unwrap();

            wait_for_messages(&mock, 1).await;
            let messages = mock.message_bodies();
            assert_eq!(messages.len(), 1, "手动停用不该发通知，只应有自动那一条");
            let content = messages[0]["content"].as_str().unwrap();
            assert!(
                content.contains("自动停用供应商"),
                "收到的应是自动那条：{content}"
            );
            assert!(
                !content.contains("手动停用供应商"),
                "手动停用的供应商不该出现：{content}"
            );
        },
    )
    .await;
}

/// 停用通知开关时不发送：直接 `await` 通知主体（不走 spawn），确定性断言。
#[tokio::test]
async fn disabled_channel_stays_silent() {
    let _guard = SEND_LOCK.lock().await;
    let (base, mock) = spawn_feishu_mock().await;

    temp_env::async_with_vars(
        [("LLM_GATEWAY_FEISHU_BASE_URL", Some(base.as_str()))],
        async {
            let (app, db) = setup_app().await;

            // 存一份 enable=false 的配置。
            let (status, _) = call(
                &app,
                "PUT",
                URI,
                Some(json!({
                    "appId": "cli_mock",
                    "appSecret": "cli-secret-abcd1234",
                    "receiverType": "open_id",
                    "receiver": "ou_watcher",
                    "enable": false,
                })),
            )
            .await;
            assert_eq!(status, StatusCode::OK);

            let seeded = seed_subscription_provider(&db, "静音期供应商").await;
            let result = llm_gateway::notification::notify::notify_provider_transition(
                &db,
                seeded.id,
                llm_gateway::notification::notify::Transition::QuotaDisabled {
                    label: "订阅额度".to_string(),
                },
            )
            .await;

            assert!(result.is_ok(), "停用态不该报错：{result:?}");
            assert!(
                mock.message_bodies().is_empty(),
                "通知开关关闭时不该发送任何消息"
            );
        },
    )
    .await;
}

/// 熔断停用（连续失败达阈值）同样触发通知，原因带失败次数。
#[tokio::test]
async fn failure_circuit_break_sends_notification() {
    let _guard = SEND_LOCK.lock().await;
    let (base, mock) = spawn_feishu_mock().await;

    temp_env::async_with_vars(
        [("LLM_GATEWAY_FEISHU_BASE_URL", Some(base.as_str()))],
        async {
            let (app, db) = setup_app().await;
            save_config(&app, "ou_watcher", "cli-secret-abcd1234").await;
            let seeded = seed_subscription_provider(&db, "熔断供应商").await;

            let counters = llm_gateway::availability::FailureCounter::default();
            // 阈值 3：连击到第 3 次才熔断。
            for attempt in 1..=3 {
                llm_gateway::availability::on_forward_failure(
                    &db,
                    &counters,
                    seeded.id,
                    3,
                    &format!("req-{attempt}"),
                )
                .await
                .unwrap();
            }

            wait_for_messages(&mock, 1).await;
            let messages = mock.message_bodies();
            assert_eq!(messages.len(), 1, "熔断应发出通知");
            let content = messages[0]["content"].as_str().unwrap();
            assert!(content.contains("【供应商自动停用】"), "{content}");
            assert!(content.contains("连续失败 3 次"), "原因应带次数：{content}");
        },
    )
    .await;
}

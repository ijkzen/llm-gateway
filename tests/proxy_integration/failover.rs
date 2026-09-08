use super::*;

#[tokio::test]
async fn failover_retries_next_member_on_429() {
    // 成员 A：返回 429；成员 B：OpenAI 成功。
    let fail_router = Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            (
                HttpStatus::TOO_MANY_REQUESTS,
                Json(json!({"error": {"message": "rate limited"}})),
            )
                .into_response()
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let fail_addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, fail_router).await.unwrap();
    });
    let fail_base = format!("http://{fail_addr}");

    let ok_base = spawn_mock(capture()).await;

    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    scheduler.start().await.unwrap();
    let app = common::build_authed_app(db.clone(), scheduler, log_tx).await;
    let provider_a = seed_provider(&db, "p-a", &fail_base, 0, 0).await;
    let model_a = seed_provider_model(&db, provider_a, "m-a").await;
    let provider_b = seed_provider(&db, "p-b", &ok_base, 0, 0).await;
    let model_b = seed_provider_model(&db, provider_b, "m-b").await;

    let vm = virtual_model::ActiveModel {
        display_id: Set("vm-fo".to_string()),
        enable: Set(true),
        // RoundRobin：成员顺序确定（A→B），保证 A 的 429 必被尝试后降级到 B。
        load_balancing_strategy: Set(2),
        fallback_strategy: Set(1), // RetryEnabledMembers
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    let vm = vm.insert(&db).await.unwrap();
    for model_id in [model_a, model_b] {
        virtual_model_item::ActiveModel {
            virtual_model_id: Set(vm.virtual_model_id),
            model_id: Set(model_id),
            enable: Set(true),
            created_at: Set(chrono::Utc::now()),
            updated_at: Set(chrono::Utc::now()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
    }

    let (status, text) = send_chat(&app, chat_body("vm-fo", false)).await;
    assert_eq!(status, 200, "应 failover 到成员 B：{text}");

    let rows = wait_for_records(&db, 2).await;
    // 降级失败行：成员 A 带 -1 后缀，success=false，fail_reason 记上游原因。
    let failed = rows.iter().find(|r| !r.success).expect("应有降级失败行");
    assert_eq!(failed.provider_id, provider_a);
    assert_eq!(failed.model_id, "m-a");
    assert!(
        failed
            .fail_reason
            .as_deref()
            .unwrap_or("")
            .contains("rate limited")
    );
    assert!(
        failed.request_id.ends_with("-1"),
        "降级失败行 request_id 应带 -1 后缀：{}",
        failed.request_id
    );
    // 最终成功行：成员 B，原始 request_id。
    let record = rows.iter().find(|r| r.success).expect("应有成功行");
    assert_eq!(record.provider_id, provider_b, "记录最终成功的成员");
    assert_eq!(record.model_id, "m-b");
    assert!(
        !record.request_id.ends_with("-1"),
        "成功行应为原始 request_id：{}",
        record.request_id
    );
}

#[tokio::test]
async fn failover_retries_next_member_on_400() {
    // 回归：上游 400（如额度耗尽 insufficient credits）也必须降级。
    // 成员 A：返回 400；成员 B：OpenAI 成功。
    let fail_router = Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            (
                HttpStatus::BAD_REQUEST,
                Json(json!({"error": {"message": "You have insufficient credits to make this request."}})),
            )
                .into_response()
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let fail_addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, fail_router).await.unwrap();
    });
    let fail_base = format!("http://{fail_addr}");

    let ok_base = spawn_mock(capture()).await;

    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    scheduler.start().await.unwrap();
    let app = common::build_authed_app(db.clone(), scheduler, log_tx).await;
    let provider_a = seed_provider(&db, "p-a", &fail_base, 0, 0).await;
    let model_a = seed_provider_model(&db, provider_a, "m-a").await;
    let provider_b = seed_provider(&db, "p-b", &ok_base, 0, 0).await;
    let model_b = seed_provider_model(&db, provider_b, "m-b").await;

    let vm = virtual_model::ActiveModel {
        display_id: Set("vm-fo400".to_string()),
        enable: Set(true),
        // RoundRobin：成员顺序确定（A→B），保证 A 的 400 必被尝试后降级到 B。
        load_balancing_strategy: Set(2),
        fallback_strategy: Set(1), // RetryEnabledMembers
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    let vm = vm.insert(&db).await.unwrap();
    for model_id in [model_a, model_b] {
        virtual_model_item::ActiveModel {
            virtual_model_id: Set(vm.virtual_model_id),
            model_id: Set(model_id),
            enable: Set(true),
            created_at: Set(chrono::Utc::now()),
            updated_at: Set(chrono::Utc::now()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
    }

    let (status, text) = send_chat(&app, chat_body("vm-fo400", false)).await;
    assert_eq!(status, 200, "400 也应 failover 到成员 B：{text}");

    let rows = wait_for_records(&db, 2).await;
    // 降级失败行：成员 A 带 -1 后缀，success=false，fail_reason 记上游原因。
    let failed = rows.iter().find(|r| !r.success).expect("应有降级失败行");
    assert_eq!(failed.provider_id, provider_a);
    assert_eq!(failed.model_id, "m-a");
    assert!(
        failed
            .fail_reason
            .as_deref()
            .unwrap_or("")
            .contains("insufficient credits")
    );
    assert!(
        failed.request_id.ends_with("-1"),
        "降级失败行 request_id 应带 -1 后缀：{}",
        failed.request_id
    );
    // 最终成功行：成员 B，原始 request_id。
    let record = rows.iter().find(|r| r.success).expect("应有成功行");
    assert_eq!(record.provider_id, provider_b, "记录最终成功的成员");
    assert_eq!(record.model_id, "m-b");
    assert!(
        !record.request_id.ends_with("-1"),
        "成功行应为原始 request_id：{}",
        record.request_id
    );
}

#[tokio::test]
async fn all_members_fail_records_each_attempt() {
    // 全部成员失败：A、B 均返回 429（fallback=1）。每个成员尝试各落一行：
    // 降级中失败行带 -1 后缀，最后失败行用原始 request_id。
    let fail_router = Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            (
                HttpStatus::TOO_MANY_REQUESTS,
                Json(json!({"error": {"message": "rate limited"}})),
            )
                .into_response()
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let fail_addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, fail_router).await.unwrap();
    });
    let fail_base = format!("http://{fail_addr}");

    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    scheduler.start().await.unwrap();
    let app = common::build_authed_app(db.clone(), scheduler, log_tx).await;
    let provider_a = seed_provider(&db, "p-a", &fail_base, 0, 0).await;
    let model_a = seed_provider_model(&db, provider_a, "m-a").await;
    let provider_b = seed_provider(&db, "p-b", &fail_base, 0, 0).await;
    let model_b = seed_provider_model(&db, provider_b, "m-b").await;

    let vm = virtual_model::ActiveModel {
        display_id: Set("vm-all-fail".to_string()),
        enable: Set(true),
        // RoundRobin：成员顺序确定（A→B），A 行必为降级中失败（-1 后缀）、
        // B 行为最后失败（原始 id），断言可绑定具体 provider。
        load_balancing_strategy: Set(2),
        fallback_strategy: Set(1), // RetryEnabledMembers
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    let vm = vm.insert(&db).await.unwrap();
    for model_id in [model_a, model_b] {
        virtual_model_item::ActiveModel {
            virtual_model_id: Set(vm.virtual_model_id),
            model_id: Set(model_id),
            enable: Set(true),
            created_at: Set(chrono::Utc::now()),
            updated_at: Set(chrono::Utc::now()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
    }

    let (status, _text) = send_chat(&app, chat_body("vm-all-fail", false)).await;
    assert_eq!(status, 429, "全败取最后成员的状态");

    let rows = wait_for_records(&db, 2).await;
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|r| !r.success), "全败所有行 success=false");

    // RoundRobin 下成员顺序确定（A→B）：A 行（降级中失败）带 -1 后缀，
    // B 行（最后失败）用原始 request_id。
    let first = rows.iter().find(|r| r.provider_id == provider_a).unwrap();
    assert_eq!(first.model_id, "m-a");
    assert!(
        first.request_id.ends_with("-1"),
        "A 行应带 -1 后缀：{}",
        first.request_id
    );
    let last = rows.iter().find(|r| r.provider_id == provider_b).unwrap();
    assert_eq!(last.model_id, "m-b");
    assert!(
        !last.request_id.ends_with("-1"),
        "B 行应为原始 request_id：{}",
        last.request_id
    );
}

#[tokio::test]
async fn fail_directly_returns_upstream_error_and_records() {
    let fail_router = Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            (
                HttpStatus::TOO_MANY_REQUESTS,
                Json(json!({"error": {"message": "rate limited"}})),
            )
                .into_response()
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let fail_addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, fail_router).await.unwrap();
    });
    let base = format!("http://{fail_addr}");

    let (app, db) = common_setup_with_member(&base, 0, 0, 0).await;
    // fallback 策略 0（FailDirectly）：把虚拟模型改成 fallback 0。
    let vm = virtual_model::Entity::find()
        .filter(virtual_model::Column::DisplayId.eq("vm-x"))
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    let mut active: virtual_model::ActiveModel = vm.into();
    active.fallback_strategy = Set(0);
    active.update(&db).await.unwrap();

    let (status, text) = send_chat(&app, chat_body("vm-x", false)).await;
    assert_eq!(status, 429);
    let body: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(body["error"]["message"], "rate limited");
    assert_eq!(body["error"]["type"], "invalid_request_error");

    let rows = wait_for_records(&db, 1).await;
    let record = &rows[0];
    assert_eq!(record.success, false);
    assert!(
        record
            .fail_reason
            .as_deref()
            .unwrap_or("")
            .contains("rate limited")
    );
}

#[tokio::test]
async fn unknown_model_is_404_without_record() {
    let base = spawn_mock(capture()).await;
    let (app, db) = common_setup_with_member(&base, 0, 0, 0).await;

    let (status, text) = send_chat(&app, chat_body("no-such-model", false)).await;
    assert_eq!(status, 404);
    let body: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(body["error"]["code"], "model_not_found");

    tokio::time::sleep(Duration::from_millis(300)).await;
    let rows = request::Entity::find().all(&db).await.unwrap();
    assert!(rows.is_empty(), "路由未命中不落表");
}

#[tokio::test]
async fn model_protocol_override_beats_provider_protocol() {
    // 供应商协议 = Anthropic(2)，但模型单独覆盖为 OpenAI Responses(1)：
    // 出站请求必须按 Responses 形状（/v1/responses + max_output_tokens），而非 Anthropic。
    // 协议判别以 body 形状为准（Responses 有 max_output_tokens 无 messages，Anthropic 反之，
    // 互斥可判别）；URL 路径由 mock 路由隐含验证——协议取错会打到不存在的路径而 404 失败。
    let captured = capture();
    let base = spawn_mock(captured.clone()).await;
    let (app, db) = common_setup_with_model_protocol(&base, 2, Some(1)).await;

    let (status, text) = send_chat(&app, chat_body("vm-x", false)).await;
    assert_eq!(status, 200, "{text}");
    let body: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(body["choices"][0]["message"]["content"], "你好");

    let upstream_bodies = captured.lock().unwrap();
    assert!(!upstream_bodies.is_empty(), "上游应收到请求");
    // Responses 形状：max_output_tokens 而非 Anthropic 的 max_tokens/messages。
    assert_eq!(upstream_bodies[0]["max_output_tokens"], 128);
    assert!(upstream_bodies[0].get("messages").is_none());

    let rows = wait_for_records(&db, 1).await;
    assert_eq!(rows[0].input_tokens, Some(12));
}

#[tokio::test]
async fn model_protocol_null_falls_back_to_provider_protocol() {
    // 模型未覆盖协议（None）→ 沿用供应商协议：供应商为 Anthropic(2) 时
    // 出站请求按 Anthropic 形状（/v1/messages + max_tokens），行为与现状一致。
    let captured = capture();
    let base = spawn_mock(captured.clone()).await;
    let (app, _db) = common_setup_with_model_protocol(&base, 2, None).await;

    let (status, text) = send_chat(&app, chat_body("vm-x", false)).await;
    assert_eq!(status, 200, "{text}");

    let upstream_bodies = captured.lock().unwrap();
    assert!(!upstream_bodies.is_empty(), "上游应收到请求");
    assert_eq!(upstream_bodies[0]["max_tokens"], 128);
    assert_eq!(upstream_bodies[0]["messages"][0]["role"], "user");
}

#[tokio::test]
async fn subscription_first_ranks_by_remaining_five_hour_usage() {
    use llm_gateway::usage::persist::write_usage_cache;
    use llm_gateway::usage::types::{UsageData, UsageKind, WindowKind};

    let captured_a = capture();
    let captured_b = capture();
    let base_a = spawn_mock(captured_a.clone()).await;
    let base_b = spawn_mock(captured_b.clone()).await;

    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    let provider_a = seed_provider(&db, "订阅-A-高5h余量", &base_a, 0, 1).await;
    let provider_b = seed_provider(&db, "订阅-B-低5h余量", &base_b, 0, 1).await;
    let model_a = seed_provider_model(&db, provider_a, "m-a").await;
    let model_b = seed_provider_model(&db, provider_b, "m-b").await;

    // 预置 10 分钟内的用量数据库缓存：A 的 5h 剩余（80%）高于 B（20%）。
    for (pid, remaining) in [(provider_a, 80.0), (provider_b, 20.0)] {
        let data = UsageData {
            provider_id: pid,
            fetched_at: chrono::Utc::now(),
            kind: UsageKind::Quota,
            plan: None,
            windows: vec![
                llm_gateway::usage::types::QuotaWindow::from_remaining_percent(
                    WindowKind::FiveHour,
                    remaining,
                    None,
                ),
                llm_gateway::usage::types::QuotaWindow::from_remaining_percent(
                    WindowKind::Weekly,
                    50.0,
                    None,
                ),
                llm_gateway::usage::types::QuotaWindow::unavailable(WindowKind::Monthly),
            ],
            balances: vec![],
        };
        write_usage_cache(&db, &data).await.unwrap();
    }

    scheduler.start().await.unwrap();
    let app = common::build_authed_app(db.clone(), scheduler, log_tx).await;

    let vm = virtual_model::ActiveModel {
        display_id: Set("vm-lb-q".to_string()),
        enable: Set(true),
        load_balancing_strategy: Set(0),
        fallback_strategy: Set(1),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    let vm = vm.insert(&db).await.unwrap();
    for model_id in [model_a, model_b] {
        virtual_model_item::ActiveModel {
            virtual_model_id: Set(vm.virtual_model_id),
            model_id: Set(model_id),
            enable: Set(true),
            created_at: Set(chrono::Utc::now()),
            updated_at: Set(chrono::Utc::now()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
    }

    let (status, text) = send_chat(
        &app,
        json!({"model": "vm-lb-q", "messages": [{"role": "user", "content": "hi"}]}),
    )
    .await;
    assert_eq!(status, 200, "转发失败：{text}");
    assert_eq!(
        captured_a.lock().unwrap().len(),
        1,
        "订阅制优先应选择 5h 剩余更高的供应商 A"
    );
    assert_eq!(captured_b.lock().unwrap().len(), 0, "供应商 B 不应被选到");
}

#[tokio::test]
async fn subscription_first_prefers_earlier_deadline() {
    use chrono::Duration;
    use llm_gateway::usage::persist::write_usage_cache;
    use llm_gateway::usage::types::{QuotaWindow, UsageData, UsageKind, WindowKind};

    let captured_a = capture();
    let captured_b = capture();
    let base_a = spawn_mock(captured_a.clone()).await;
    let base_b = spawn_mock(captured_b.clone()).await;

    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    let provider_a = seed_provider(&db, "订阅-A-5h余量高截止远", &base_a, 0, 1).await;
    let provider_b = seed_provider(&db, "订阅-B-5h余量低截止近", &base_b, 0, 1).await;
    let model_a = seed_provider_model(&db, provider_a, "m-a").await;
    let model_b = seed_provider_model(&db, provider_b, "m-b").await;

    // 预置 10 分钟内的用量缓存：A 5h 剩余 80（高）但周截止 5 天后；
    // B 5h 剩余 20（低）但周截止 1 天后 → 截止日期优先应选 B。
    let now = chrono::Utc::now();
    for (pid, five_hour, weekly_reset_in_days) in [(provider_a, 80.0, 5), (provider_b, 20.0, 1)] {
        write_usage_cache(
            &db,
            &UsageData {
                provider_id: pid,
                fetched_at: now,
                kind: UsageKind::Quota,
                plan: None,
                windows: vec![
                    QuotaWindow::from_remaining_percent(WindowKind::FiveHour, five_hour, None),
                    QuotaWindow::from_remaining_percent(
                        WindowKind::Weekly,
                        50.0,
                        Some(now + Duration::days(weekly_reset_in_days)),
                    ),
                    QuotaWindow::unavailable(WindowKind::Monthly),
                ],
                balances: vec![],
            },
        )
        .await
        .unwrap();
    }

    scheduler.start().await.unwrap();
    let app = common::build_authed_app(db.clone(), scheduler, log_tx).await;

    let vm = virtual_model::ActiveModel {
        display_id: Set("vm-lb-deadline".to_string()),
        enable: Set(true),
        load_balancing_strategy: Set(0),
        fallback_strategy: Set(1),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    let vm = vm.insert(&db).await.unwrap();
    for model_id in [model_a, model_b] {
        virtual_model_item::ActiveModel {
            virtual_model_id: Set(vm.virtual_model_id),
            model_id: Set(model_id),
            enable: Set(true),
            created_at: Set(chrono::Utc::now()),
            updated_at: Set(chrono::Utc::now()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
    }

    let (status, text) = send_chat(
        &app,
        json!({"model": "vm-lb-deadline", "messages": [{"role": "user", "content": "hi"}]}),
    )
    .await;
    assert_eq!(status, 200, "转发失败：{text}");
    assert_eq!(
        captured_b.lock().unwrap().len(),
        1,
        "截止日期优先应选择周截止更近的供应商 B"
    );
    assert_eq!(captured_a.lock().unwrap().len(), 0, "供应商 A 不应被选到");
}

/// 成员全部被额度剔除（订阅制窗口剩余为 0）时，chat 路径维持既有
/// 503 语义（error.code = no_available_members）——空候选由尝试核心统一守卫。
#[tokio::test]
async fn chat_quota_exhausted_returns_503() {
    let captured = capture();
    let base = spawn_mock(captured.clone()).await;
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    scheduler.start().await.unwrap();
    let app = common::build_authed_app(db.clone(), scheduler, log_tx).await;
    seed_exhausted_subscription(&db, &base, "p-chat-exhausted", 0, "vm-chat-exhausted", 0).await;

    let (status, text) = send_chat(
        &app,
        json!({"model": "vm-chat-exhausted", "messages": [{"role": "user", "content": "hi"}]}),
    )
    .await;
    assert_eq!(status, 503, "{text}");
    let parsed: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(parsed["error"]["code"], "no_available_members");
}

/// 回归（成员尝试核心）：成员全部被额度剔除时 /v1/messages 返回 503
/// 而非空候选 panic——旧 forward_native 对 ordered[0] 无守卫直接索引。
#[tokio::test]
async fn native_messages_quota_exhausted_returns_503_instead_of_panic() {
    let captured = capture();
    let base = spawn_mock(captured.clone()).await;
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    scheduler.start().await.unwrap();
    let app = common::build_authed_app(db.clone(), scheduler, log_tx).await;
    seed_exhausted_subscription(
        &db,
        &base,
        "p-native-exhausted",
        2,
        "vm-native-exhausted",
        2,
    )
    .await;

    let (status, text, _content_type) = send_native(
        &app,
        "/v1/messages",
        messages_body("vm-native-exhausted", false),
        &[],
    )
    .await;
    assert_eq!(status, 503, "额度耗尽应返回 503 而非 500 panic：{text}");
    let parsed: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(parsed["type"], "error", "应使用 Anthropic 原生错误信封");
    assert_eq!(parsed["error"]["type"], "api_error");
}

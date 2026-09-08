use super::*;

/// 测速按模型协议：供应商=Anthropic(2) + 模型覆盖=Responses(1) → 测试请求打到
/// /v1/responses（Responses mock 返回 200 说明走对协议；若回落 Anthropic 会打
/// /v1/messages 而 404 失败）。
#[tokio::test]
async fn test_model_test_uses_model_protocol_override() {
    let target = spawn_responses_mock().await;
    let (app, db) = setup_app().await;
    let (provider_id, model_id) = seed_provider_and_model_with_protocol(
        &db,
        &format!("tp-{}", chrono::Utc::now().timestamp_millis()),
        &target,
        2,       // 供应商协议 = Anthropic
        Some(1), // 模型覆盖 = OpenAI Responses
    )
    .await;

    let (status, body) = send_json(
        app,
        "POST",
        &format!("/api/providers/{provider_id}/models/{model_id}/test"),
        Value::Null,
    )
    .await;
    assert_eq!(status, 200, "测速应按模型覆盖协议（Responses）成功：{body}");
}

/// 测速回落供应商协议：模型未覆盖协议（None）→ 测试请求按供应商协议走
/// （供应商=Anthropic 时打 /v1/messages，成功说明回落正确）。
#[tokio::test]
async fn test_model_test_falls_back_to_provider_protocol() {
    let target = spawn_messages_mock().await;
    let (app, db) = setup_app().await;
    let (provider_id, model_id) = seed_provider_and_model_with_protocol(
        &db,
        &format!("tb-{}", chrono::Utc::now().timestamp_millis()),
        &target,
        2,    // 供应商协议 = Anthropic
        None, // 模型未覆盖 → 跟随供应商
    )
    .await;

    let (status, body) = send_json(
        app,
        "POST",
        &format!("/api/providers/{provider_id}/models/{model_id}/test"),
        Value::Null,
    )
    .await;
    assert_eq!(status, 200, "测速应回落供应商协议（Anthropic）成功：{body}");
}

/// 模型开了代理（供应商也开了另一个）→ 测试请求走模型代理。
#[tokio::test]
async fn test_model_test_goes_through_model_proxy() {
    let target = spawn_chat_mock().await;
    let (provider_proxy, provider_hits) = spawn_connect_proxy().await;
    let (model_proxy, model_hits) = spawn_connect_proxy().await;
    let (app, db) = setup_app().await;
    let (provider_id, model_id) = seed_provider_and_model(
        &db,
        &format!("pp-{}", chrono::Utc::now().timestamp_millis()),
        &target,
        Some(provider_proxy.clone()),
        Some(model_proxy.clone()),
    )
    .await;

    let (status, body) = send_json(
        app,
        "POST",
        &format!("/api/providers/{provider_id}/models/{model_id}/test"),
        Value::Null,
    )
    .await;
    assert_eq!(status, 200, "测试失败：{body}");
    let provider_hits = provider_hits.load(std::sync::atomic::Ordering::SeqCst);
    let model_hits = model_hits.load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(model_hits, 1, "模型代理应被命中");
    assert_eq!(provider_hits, 0, "供应商代理不应被命中（模型级优先）");
}

/// 模型未开代理、供应商开了 → 测试请求回落供应商代理。
#[tokio::test]
async fn test_model_test_falls_back_to_provider_proxy() {
    let target = spawn_chat_mock().await;
    let (provider_proxy, provider_hits) = spawn_connect_proxy().await;
    let (app, db) = setup_app().await;
    let (provider_id, model_id) = seed_provider_and_model(
        &db,
        &format!("fp-{}", chrono::Utc::now().timestamp_millis()),
        &target,
        Some(provider_proxy),
        None,
    )
    .await;

    let (status, body) = send_json(
        app,
        "POST",
        &format!("/api/providers/{provider_id}/models/{model_id}/test"),
        Value::Null,
    )
    .await;
    assert_eq!(status, 200, "测试失败：{body}");
    assert_eq!(
        provider_hits.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "模型未开代理时回落供应商代理"
    );
}

/// 模型与供应商都未开代理 → 测试请求直连。
#[tokio::test]
async fn test_model_test_direct_without_any_proxy() {
    let target = spawn_chat_mock().await;
    let (app, db) = setup_app().await;
    let (provider_id, model_id) = seed_provider_and_model(
        &db,
        &format!("dr-{}", chrono::Utc::now().timestamp_millis()),
        &target,
        None,
        None,
    )
    .await;

    let (status, body) = send_json(
        app,
        "POST",
        &format!("/api/providers/{provider_id}/models/{model_id}/test"),
        Value::Null,
    )
    .await;
    assert_eq!(status, 200, "直连测试失败：{body}");
}

/// 模型配了代理但供应商没配时，用量/刷新路径仍直连（不误用模型代理）。
/// 回归：模型级代理只影响转发与测速，不影响「刷新模型列表」。
#[tokio::test]
async fn test_refresh_ignores_model_proxy() {
    let target = spawn_models_mock().await;
    let (model_proxy, model_hits) = spawn_connect_proxy().await;
    let (app, db) = setup_app().await;
    let (provider_id, _model_id) = seed_provider_and_model(
        &db,
        &format!("mr-{}", chrono::Utc::now().timestamp_millis()),
        &target,
        None,
        Some(model_proxy),
    )
    .await;

    let (status, body) = send_json(
        app,
        "POST",
        &format!("/api/providers/{provider_id}/models/refresh"),
        Value::Null,
    )
    .await;
    assert_eq!(status, 200, "刷新失败：{body}");
    assert_eq!(
        model_hits.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "刷新模型列表只认供应商代理，不应走模型级代理"
    );
}

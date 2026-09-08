use super::*;

#[tokio::test]
async fn test_interface_type_crud_and_validation() {
    let (app, db) = setup_app().await;
    let p0 = seed_provider_with_protocol(&db, "p-typed-oc", 0).await;
    let m1 = seed_provider_model(&db, p0, "typed-model-1").await;
    let m2 = seed_provider_model(&db, p0, "typed-model-2").await;
    let p1 = seed_provider_with_protocol(&db, "p-typed-resp", 1).await;
    let m_resp = seed_provider_model(&db, p1, "typed-model-resp").await;

    // 缺省 interfaceType → 默认 0（OpenAI Compatible）。
    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/virtual-models",
        vm_payload("default-type", &[m1]),
    )
    .await;
    assert_eq!(status, 201);
    assert_eq!(body["data"]["interfaceType"], 0);

    // 显式 Responses 类型 + 更新为 Messages 类型。
    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/virtual-models",
        vm_payload_typed("resp-model", &[m_resp], 1),
    )
    .await;
    assert_eq!(status, 201, "body={body}");
    assert_eq!(body["data"]["interfaceType"], 1);
    let vm_id = body["data"]["virtualModelId"].as_i64().unwrap();

    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/virtual-models/{vm_id}"),
        json!({ "interfaceType": 2 }),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["data"]["interfaceType"], 2);

    // Full Compatible 与 Gemini 均合法（编号与协议类型对齐）。
    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/virtual-models/{vm_id}"),
        json!({ "interfaceType": 4 }),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["data"]["interfaceType"], 4);
    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/virtual-models/{vm_id}"),
        json!({ "interfaceType": 3 }),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["data"]["interfaceType"], 3);

    // 越界值拒绝。
    let (status, _) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/virtual-models/{vm_id}"),
        json!({ "interfaceType": 5 }),
    )
    .await;
    assert_eq!(status, 400);
    let (status, _) = send_json(
        app,
        "POST",
        "/api/virtual-models",
        vm_payload_typed("bad-type", &[m2], -1),
    )
    .await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn test_interface_type_filters_v1_surfaces() {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    scheduler.start().await.unwrap();
    let app = common::build_authed_app(db.clone(), scheduler, log_tx).await;

    let p0 = seed_provider_with_protocol(&db, "p-filter-oc", 0).await;
    let p1 = seed_provider_with_protocol(&db, "p-filter-resp", 1).await;
    let p2 = seed_provider_with_protocol(&db, "p-filter-msgs", 2).await;
    let m1 = seed_provider_model(&db, p0, "filter-model-1").await;
    let m2 = seed_provider_model(&db, p1, "filter-model-2").await;
    let m3 = seed_provider_model(&db, p2, "filter-model-3").await;

    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/virtual-models",
        vm_payload("chat-model", &[m1]),
    )
    .await;
    assert_eq!(status, 201, "body={body}");

    let (status, _) = send_json(
        app.clone(),
        "POST",
        "/api/virtual-models",
        vm_payload_typed("resp-only", &[m2], 1),
    )
    .await;
    assert_eq!(status, 201);
    let (status, _) = send_json(
        app.clone(),
        "POST",
        "/api/virtual-models",
        vm_payload_typed("msgs-only", &[m3], 2),
    )
    .await;
    assert_eq!(status, 201);

    // /v1/models 只返回 OpenAI Compatible / Full Compatible。
    let (status, body) = send_v1_json(app.clone(), "GET", "/v1/models", Value::Null).await;
    assert_eq!(status, 200);
    let ids: Vec<&str> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["chat-model"]);

    // 单查同样过滤。
    let (status, _) = send_v1_json(app.clone(), "GET", "/v1/models/resp-only", Value::Null).await;
    assert_eq!(status, 404);

    // chat/completions 拒绝 Responses/Messages 类型（模型不存在语义）。
    let (status, body) = send_v1_json(
        app.clone(),
        "POST",
        "/v1/chat/completions",
        json!({"model": "resp-only", "messages": [{"role": "user", "content": "hi"}]}),
    )
    .await;
    assert_eq!(status, 404);
    assert_eq!(body["error"]["code"], "model_not_found");
    let (status, _) = send_v1_json(
        app.clone(),
        "POST",
        "/v1/chat/completions",
        json!({"model": "msgs-only", "messages": [{"role": "user", "content": "hi"}]}),
    )
    .await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn test_restricted_interface_type_rejects_protocol_mismatched_members() {
    let (app, db) = setup_app().await;
    // Anthropic 协议供应商（协议 2）的成员。
    let p = seed_provider_with_protocol(&db, "p-anthropic", 2).await;
    let m1 = seed_provider_model(&db, p, "claude-x1").await;
    let m2 = seed_provider_model(&db, p, "claude-x2").await;
    let m3 = seed_provider_model(&db, p, "claude-x3").await;

    // OpenAI Compatible 类型（0）不能挂 Anthropic 成员。
    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/virtual-models",
        vm_payload_typed("oc-vm", &[m1], 0),
    )
    .await;
    assert_eq!(status, 400, "body={body}");

    // Responses 类型（1）同样拒绝。
    let (status, _) = send_json(
        app.clone(),
        "POST",
        "/api/virtual-models",
        vm_payload_typed("resp-vm", &[m1], 1),
    )
    .await;
    assert_eq!(status, 400);

    // Messages 类型（2）接受本协议成员。
    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/virtual-models",
        vm_payload_typed("msgs-vm", &[m2], 2),
    )
    .await;
    assert_eq!(status, 201, "body={body}");

    // Full Compatible（4）接受任意协议成员。
    let (status, _) = send_json(
        app.clone(),
        "POST",
        "/api/virtual-models",
        vm_payload_typed("full-vm", &[m3], 4),
    )
    .await;
    assert_eq!(status, 201, "body={body}");
}

#[tokio::test]
async fn test_interface_type_change_cascades_remove_mismatched_members() {
    let (app, db) = setup_app().await;
    let p = seed_provider_with_protocol(&db, "p-anthropic-2", 2).await;
    let m = seed_provider_model(&db, p, "claude-y").await;

    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/virtual-models",
        vm_payload_typed("full-vm-2", &[m], 4),
    )
    .await;
    assert_eq!(status, 201, "body={body}");
    let vm_id = body["data"]["virtualModelId"].as_i64().unwrap();

    // 改成 Messages 类型 → 不匹配成员为 0 个（本来就匹配），成员保留。
    // 改成 OpenAI Compatible 类型 → Anthropic 成员被硬删。
    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/virtual-models/{vm_id}"),
        json!({ "interfaceType": 0 }),
    )
    .await;
    assert_eq!(status, 200, "body={body}");
    assert_eq!(body["data"]["interfaceType"], 0);
    assert_eq!(body["data"]["items"].as_array().unwrap().len(), 0);

    let items = virtual_model_item::Entity::find()
        .filter(virtual_model_item::Column::VirtualModelId.eq(vm_id as i32))
        .all(&db)
        .await
        .unwrap();
    assert!(items.is_empty(), "不匹配成员应被硬删");
}

#[tokio::test]
async fn test_provider_model_protocol_change_cascades_remove_member() {
    let (app, db) = setup_app().await;
    // 供应商协议 0，模型级覆盖为 2 → 生效协议 2，可进 Messages 类型虚拟模型。
    let p = seed_provider_with_protocol(&db, "p-openai-2", 0).await;
    let m = seed_provider_model(&db, p, "dual-protocol").await;
    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/providers/{p}/models/{m}"),
        json!({
            "providerModelId": "dual-protocol",
            "contextLength": 128000,
            "maxOutputTokens": 4096,
            "protocolType": 2
        }),
    )
    .await;
    assert_eq!(status, 200, "body={body}");

    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/virtual-models",
        vm_payload_typed("msgs-vm-2", &[m], 2),
    )
    .await;
    assert_eq!(status, 201, "body={body}");

    // 模型级覆盖改为 0 → 生效协议 0，从 Messages 类型虚拟模型中移除。
    // （Upsert 请求为全量字段）
    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/providers/{p}/models/{m}"),
        json!({
            "providerModelId": "dual-protocol",
            "contextLength": 128000,
            "maxOutputTokens": 4096,
            "protocolType": 0
        }),
    )
    .await;
    assert_eq!(status, 200, "body={body}");

    let items = virtual_model_item::Entity::find()
        .filter(virtual_model_item::Column::ModelId.eq(m))
        .all(&db)
        .await
        .unwrap();
    assert!(items.is_empty(), "生效协议变更后成员应从受限虚拟模型移除");
}

#[tokio::test]
async fn test_provider_protocol_change_cascades_remove_member() {
    let (app, db) = setup_app().await;
    let p = seed_provider_with_protocol(&db, "p-anthropic-3", 2).await;
    let m = seed_provider_model(&db, p, "claude-z").await;

    let (status, _) = send_json(
        app.clone(),
        "POST",
        "/api/virtual-models",
        vm_payload_typed("msgs-vm-3", &[m], 2),
    )
    .await;
    assert_eq!(status, 201);

    // 供应商协议改为 0 → 成员生效协议 0，从 Messages 类型虚拟模型移除。
    let (status, _) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/providers/{p}"),
        json!({ "protocolType": 0 }),
    )
    .await;
    assert_eq!(status, 200);

    let items = virtual_model_item::Entity::find()
        .filter(virtual_model_item::Column::ModelId.eq(m))
        .all(&db)
        .await
        .unwrap();
    assert!(items.is_empty(), "供应商协议变更后成员应从受限虚拟模型移除");
}

#[tokio::test]
async fn test_full_compatible_vm_keeps_members_on_protocol_change() {
    let (app, db) = setup_app().await;
    let p = seed_provider_with_protocol(&db, "p-anthropic-4", 2).await;
    let m = seed_provider_model(&db, p, "claude-w").await;

    let (status, _) = send_json(
        app.clone(),
        "POST",
        "/api/virtual-models",
        vm_payload_typed("full-vm-3", &[m], 4),
    )
    .await;
    assert_eq!(status, 201);

    // 供应商协议变更 → Full Compatible 虚拟模型不移除成员。
    let (status, _) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/providers/{p}"),
        json!({ "protocolType": 0 }),
    )
    .await;
    assert_eq!(status, 200);

    let items = virtual_model_item::Entity::find()
        .filter(virtual_model_item::Column::ModelId.eq(m))
        .all(&db)
        .await
        .unwrap();
    assert_eq!(items.len(), 1, "Full Compatible 不移除成员");
}

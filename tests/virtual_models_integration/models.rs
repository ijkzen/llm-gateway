use super::*;

#[tokio::test]
async fn test_update_virtual_model_diffs_members_and_preserves_enable() {
    let (app, db) = setup_app().await;
    let p1 = seed_provider(&db, "p1").await;
    let a = seed_provider_model(&db, p1, "a").await;
    let b = seed_provider_model(&db, p1, "b").await;
    let c = seed_provider_model(&db, p1, "c").await;

    // a 启用、b 禁用。
    let payload = json!({
        "displayId": "vm1",
        "loadBalancingStrategy": 0,
        "fallbackStrategy": 0,
        "items": [
            {"modelId": a},
            {"modelId": b, "enable": false},
        ],
    });
    let (status, body) = send_json(app.clone(), "POST", "/api/virtual-models", payload).await;
    assert_eq!(status, 201);
    assert_eq!(body["data"]["items"][1]["enable"], false);
    let vm1 = body["data"]["virtualModelId"].as_i64().unwrap();

    // 更新成员为 [a, c]（b 移除、c 新增），同时修改 displayId 与策略。
    let mut payload = vm_payload("vm-renamed", &[a, c]);
    payload["loadBalancingStrategy"] = json!(2);
    payload["fallbackStrategy"] = json!(1);
    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/virtual-models/{vm1}"),
        payload,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["data"]["displayId"], "vm-renamed");
    assert_eq!(body["data"]["loadBalancingStrategy"], 2);
    assert_eq!(body["data"]["fallbackStrategy"], 1);
    let items = body["data"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 2, "b 应被移除");
    let a_item = items.iter().find(|it| it["modelId"] == a).unwrap();
    assert_eq!(a_item["enable"], true, "保留成员的 enable 不变");
    let c_item = items.iter().find(|it| it["modelId"] == c).unwrap();
    assert_eq!(c_item["enable"], true, "新增成员默认启用");

    // 只传 enable → 成员不变。
    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/virtual-models/{vm1}"),
        json!({"enable": false}),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["data"]["enable"], false);
    assert_eq!(body["data"]["items"].as_array().unwrap().len(), 2);

    // 请求里 items 为空 → 400。
    let (status, _) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/virtual-models/{vm1}"),
        json!({"enable": true, "items": []}),
    )
    .await;
    assert_eq!(status, 400);

    // b 已被移除，应可再映射到新虚拟模型。
    let (status, _) = send_json(app, "POST", "/api/virtual-models", vm_payload("vm2", &[b])).await;
    assert_eq!(status, 201);
}

#[tokio::test]
async fn test_update_missing_virtual_model_returns_404() {
    let (app, _db) = setup_app().await;
    let (status, _) = send_json(
        app.clone(),
        "PUT",
        "/api/virtual-models/999",
        json!({"enable": true}),
    )
    .await;
    assert_eq!(status, 404);
    let (status, _) = send_json(app, "GET", "/api/virtual-models/999", Value::Null).await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn test_delete_virtual_model_releases_members() {
    let (app, db) = setup_app().await;
    let p1 = seed_provider(&db, "p1").await;
    let a = seed_provider_model(&db, p1, "a").await;
    let b = seed_provider_model(&db, p1, "b").await;

    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/virtual-models",
        vm_payload("vm1", &[a]),
    )
    .await;
    assert_eq!(status, 201);
    let vm1 = body["data"]["virtualModelId"].as_i64().unwrap();
    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/virtual-models",
        vm_payload("vm2", &[b]),
    )
    .await;
    assert_eq!(status, 201);
    let vm2 = body["data"]["virtualModelId"].as_i64().unwrap();

    // a、b 均被占用 → 400。
    let (status, _) = send_json(
        app.clone(),
        "POST",
        "/api/virtual-models",
        vm_payload("vm3", &[a, b]),
    )
    .await;
    assert_eq!(status, 400);

    // 删除 vm1 释放 a；b 仍被占用 → 400。
    let (status, _) = send_json(
        app.clone(),
        "DELETE",
        &format!("/api/virtual-models/{vm1}"),
        Value::Null,
    )
    .await;
    assert_eq!(status, 200);
    let (status, _) = send_json(
        app.clone(),
        "POST",
        "/api/virtual-models",
        vm_payload("vm3", &[a, b]),
    )
    .await;
    assert_eq!(status, 400);

    // 删除 vm2 后 a、b 全部释放。
    let (status, _) = send_json(
        app.clone(),
        "DELETE",
        &format!("/api/virtual-models/{vm2}"),
        Value::Null,
    )
    .await;
    assert_eq!(status, 200);
    let items = virtual_model_item::Entity::find().all(&db).await.unwrap();
    assert!(items.is_empty(), "级联删除成员条目");

    let (status, _) = send_json(
        app.clone(),
        "POST",
        "/api/virtual-models",
        vm_payload("vm3", &[a, b]),
    )
    .await;
    assert_eq!(status, 201);

    // 重复删除 → 404。
    let (status, _) = send_json(
        app,
        "DELETE",
        &format!("/api/virtual-models/{vm2}"),
        Value::Null,
    )
    .await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn test_delete_provider_cascades_virtual_model_items() {
    let (app, db) = setup_app().await;
    let p1 = seed_provider(&db, "p1").await;
    let p2 = seed_provider(&db, "p2").await;
    let a = seed_provider_model(&db, p1, "a").await;
    let c = seed_provider_model(&db, p2, "c").await;

    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/virtual-models",
        vm_payload("vm1", &[a]),
    )
    .await;
    assert_eq!(status, 201);
    let vm1 = body["data"]["virtualModelId"].as_i64().unwrap();
    let (status, body) = send_json(
        app.clone(),
        "POST",
        "/api/virtual-models",
        vm_payload("vm2", &[c]),
    )
    .await;
    assert_eq!(status, 201);
    let vm2 = body["data"]["virtualModelId"].as_i64().unwrap();

    let (status, _) = send_json(
        app.clone(),
        "DELETE",
        &format!("/api/providers/{p1}"),
        Value::Null,
    )
    .await;
    assert_eq!(status, 200);

    // vm1 的成员被级联清理；vm2 不受影响。
    let (status, body) = send_json(
        app.clone(),
        "GET",
        &format!("/api/virtual-models/{vm1}"),
        Value::Null,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["data"]["items"].as_array().unwrap().len(), 0);
    let (status, body) = send_json(
        app.clone(),
        "GET",
        &format!("/api/virtual-models/{vm2}"),
        Value::Null,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["data"]["items"].as_array().unwrap().len(), 1);

    // 已删除供应商的模型不能再被映射（已不存在）。
    let (status, _) = send_json(app, "POST", "/api/virtual-models", vm_payload("vm3", &[a])).await;
    assert_eq!(status, 400);
}

/// 手动禁用供应商应级联停用其名下虚拟模型子模型，且成员排序把停用者沉底；
/// 重新启用后级联恢复（与用量额度门控 apply_usage_gate 语义一致）。
#[tokio::test]
async fn provider_disable_cascades_to_virtual_model_items_and_resorts() {
    let (app, db) = setup_app().await;
    // "a-model" 挂在将被禁用的供应商下，字母序本就排在前；"b-model" 挂在保留启用的供应商下。
    let p_keep = seed_provider(&db, "keep").await;
    let p_disable = seed_provider(&db, "disable").await;
    let m_alpha = seed_provider_model(&db, p_disable, "a-model").await;
    let m_beta = seed_provider_model(&db, p_keep, "b-model").await;

    let (status, _) = send_json(
        app.clone(),
        "POST",
        "/api/virtual-models",
        vm_payload("vm", &[m_alpha, m_beta]),
    )
    .await;
    assert_eq!(status, 201);

    // 手动禁用供应商（供应商详情卡片的启用开关即此接口）。
    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/providers/{p_disable}"),
        json!({ "enable": false }),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["code"], "0");

    // DB 层：该供应商名下全部虚拟模型子模型同步停用（不是只翻 provider.enable）。
    let disabled_item = virtual_model_item::Entity::find()
        .filter(virtual_model_item::Column::ModelId.eq(m_alpha))
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert!(!disabled_item.enable, "禁用供应商后子模型条目应被级联停用");

    // API 层：被禁用的成员沉底排在启用成员之后，且带 providerEnable=false。
    let (status, body) = send_json(app.clone(), "GET", "/api/virtual-models", Value::Null).await;
    assert_eq!(status, 200);
    let vms = body["data"].as_array().unwrap();
    let vm = vms.iter().find(|v| v["displayId"] == "vm").unwrap();
    let members = vm["items"].as_array().unwrap();
    assert_eq!(members.len(), 2);
    assert_eq!(members[0]["providerModelId"], "b-model");
    assert!(members[0]["enable"].as_bool().unwrap());
    assert!(members[0]["providerEnable"].as_bool().unwrap());
    assert_eq!(members[1]["providerModelId"], "a-model");
    assert!(!members[1]["enable"].as_bool().unwrap());
    assert!(!members[1]["providerEnable"].as_bool().unwrap());

    // 重新启用 → 级联恢复子模型。
    let (status, _) = send_json(
        app,
        "PUT",
        &format!("/api/providers/{p_disable}"),
        json!({ "enable": true }),
    )
    .await;
    assert_eq!(status, 200);
    let reenabled = virtual_model_item::Entity::find()
        .filter(virtual_model_item::Column::ModelId.eq(m_alpha))
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert!(reenabled.enable, "重新启用供应商后子模型条目应被级联恢复");
}

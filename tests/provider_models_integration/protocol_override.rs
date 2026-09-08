use super::*;

// ─── 模型单独选择协议：CRUD 与校验 ────────────────────────────────────────

/// 创建/更新/批量创建支持模型级协议字段（protocolType：null=跟随供应商 / 0..=3=覆盖），
/// 列表响应回显；缺省为 null。
#[tokio::test]
async fn test_model_protocol_crud_roundtrip() {
    let (app, db) = setup_app().await;
    let provider_id = seed_provider(&db, "p-protocol-crud").await;

    // 创建带协议覆盖 → 响应回显。
    let mut payload = model_payload("protocol-model");
    payload["protocolType"] = json!(1); // OpenAI Responses
    let (status, body) = send_json(
        app.clone(),
        "POST",
        &format!("/api/providers/{provider_id}/models"),
        payload,
    )
    .await;
    assert_eq!(status, 201, "创建失败：{body}");
    assert_eq!(body["data"]["protocolType"], 1);
    let model_id = body["data"]["modelId"].as_i64().unwrap();

    // 未传 protocolType → 默认 null（跟随供应商）。
    let (status, body) = send_json(
        app.clone(),
        "POST",
        &format!("/api/providers/{provider_id}/models"),
        model_payload("follow-model"),
    )
    .await;
    assert_eq!(status, 201, "创建失败：{body}");
    assert_eq!(body["data"]["protocolType"], Value::Null);
    let follow_id = body["data"]["modelId"].as_i64().unwrap();

    // GET 列表能取回两值。
    let (status, body) = send_json(
        app.clone(),
        "GET",
        &format!("/api/providers/{provider_id}/models"),
        Value::Null,
    )
    .await;
    assert_eq!(status, 200);
    let listed = body["data"].as_array().unwrap();
    assert_eq!(
        listed
            .iter()
            .find(|m| m["modelId"].as_i64() == Some(model_id))
            .unwrap()["protocolType"],
        1
    );
    assert_eq!(
        listed
            .iter()
            .find(|m| m["modelId"].as_i64() == Some(follow_id))
            .unwrap()["protocolType"],
        Value::Null
    );

    // 更新协议值。
    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/providers/{provider_id}/models/{model_id}"),
        json!({
            "providerModelId": "protocol-model",
            "contextLength": 128000,
            "maxOutputTokens": 4096,
            "protocolType": 3, // Gemini
        }),
    )
    .await;
    assert_eq!(status, 200, "更新失败：{body}");
    assert_eq!(body["data"]["protocolType"], 3);

    // 更新回 null（改回跟随供应商）。
    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/providers/{provider_id}/models/{model_id}"),
        json!({
            "providerModelId": "protocol-model",
            "contextLength": 128000,
            "maxOutputTokens": 4096,
            "protocolType": null,
        }),
    )
    .await;
    assert_eq!(status, 200, "更新失败：{body}");
    assert_eq!(body["data"]["protocolType"], Value::Null);

    // 批量创建：一个带协议覆盖、一个缺省。
    let (status, body) = send_json(
        app.clone(),
        "POST",
        &format!("/api/providers/{provider_id}/models/batch"),
        json!({
            "models": [
                {
                    "providerModelId": "batch-a",
                    "contextLength": 32000,
                    "maxOutputTokens": 2048,
                    "protocolType": 2,
                },
                {
                    "providerModelId": "batch-b",
                    "contextLength": 32000,
                    "maxOutputTokens": 2048,
                },
            ],
        }),
    )
    .await;
    assert_eq!(status, 201, "批量创建失败：{body}");
    let created = body["data"].as_array().unwrap();
    assert_eq!(created.len(), 2);
    assert_eq!(created[0]["protocolType"], 2);
    assert_eq!(
        created[1]["protocolType"],
        Value::Null,
        "未传协议默认跟随供应商"
    );
}

/// 模型级协议校验：非空值必须落在 0..=3，越界 → 400。
#[tokio::test]
async fn test_model_protocol_validation_errors() {
    let (app, db) = setup_app().await;
    let provider_id = seed_provider(&db, "p-protocol-validate").await;

    // 非法协议值（超出枚举范围）。
    let mut invalid = model_payload("m-bad");
    invalid["protocolType"] = json!(4);
    let (status, body) = send_json(
        app.clone(),
        "POST",
        &format!("/api/providers/{provider_id}/models"),
        invalid,
    )
    .await;
    assert_eq!(status, 400, "越界协议值应被拒绝：{body}");
    assert!(body["msg"].as_str().unwrap().contains("协议"));

    let mut negative = model_payload("m-neg");
    negative["protocolType"] = json!(-1);
    let (status, _) = send_json(
        app.clone(),
        "POST",
        &format!("/api/providers/{provider_id}/models"),
        negative,
    )
    .await;
    assert_eq!(status, 400, "负数协议值应被拒绝");

    // 更新时非法协议值同样拒绝。
    let (status, body) = send_json(
        app.clone(),
        "POST",
        &format!("/api/providers/{provider_id}/models"),
        model_payload("ok-model"),
    )
    .await;
    assert_eq!(status, 201);
    let model_id = body["data"]["modelId"].as_i64().unwrap();
    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/providers/{provider_id}/models/{model_id}"),
        json!({
            "providerModelId": "ok-model",
            "contextLength": 128000,
            "maxOutputTokens": 4096,
            "protocolType": 4,
        }),
    )
    .await;
    assert_eq!(status, 400, "更新时越界协议值应被拒绝：{body}");
}

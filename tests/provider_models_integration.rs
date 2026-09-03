mod common;

use axum::body::Body;
use axum::http::Request;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, Set};
use serde_json::{Value, json};
use tower::ServiceExt;

use llm_gateway::crypto;
use llm_gateway::entity::provider;
use llm_gateway::entity::provider_model;
use llm_gateway::entity::virtual_model_item;

/// 建一个测试 Provider（api_key 加密存储），返回其 id。
async fn seed_provider(db: &sea_orm::DatabaseConnection, name: &str) -> i32 {
    let active = provider::ActiveModel {
        name: Set(name.to_string()),
        enable: Set(true),
        base_url: Set("https://api.example.com/v1".to_string()),
        api_key: Set(crypto::encrypt("sk-test")),
        custom_header: Set("{}".to_string()),
        protocol_type: Set(0),
        billing_mode: Set(0),
        extra: Set("{}".to_string()),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    active.insert(db).await.unwrap().id
}

async fn setup_app() -> (axum::Router, sea_orm::DatabaseConnection) {
    let (db, scheduler, log_tx) = common::setup_db_and_scheduler().await;
    scheduler.start().await.unwrap();
    let app = common::build_authed_app(db.clone(), scheduler, log_tx).await;
    (app, db)
}

async fn send_json(app: axum::Router, method: &str, uri: &str, body: Value) -> (u16, Value) {
    let request: Request<Body> = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    let status = response.status().as_u16();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let parsed: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, parsed)
}

fn model_payload(id: &str) -> Value {
    json!({
        "providerModelId": id,
        "contextLength": 128000,
        "maxOutputTokens": 4096,
        "reasoning": true,
        "toolUse": true,
        "imageUnderstand": false,
        "videoUnderstand": false,
    })
}

#[tokio::test]
async fn test_create_and_list_provider_models() {
    let (app, db) = setup_app().await;
    let provider_id = seed_provider(&db, "p1").await;

    let (status, body) = send_json(
        app.clone(),
        "POST",
        &format!("/api/providers/{provider_id}/models"),
        model_payload("gpt-4o"),
    )
    .await;
    assert_eq!(status, 201);
    assert_eq!(body["code"], "0");
    assert_eq!(body["data"]["providerModelId"], "gpt-4o");
    assert_eq!(body["data"]["contextLength"], 128000);
    assert_eq!(body["data"]["reasoning"], true);

    let (status, body) = send_json(
        app,
        "GET",
        &format!("/api/providers/{provider_id}/models"),
        Value::Null,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["data"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn test_create_model_validations() {
    let (app, db) = setup_app().await;
    let provider_id = seed_provider(&db, "p1").await;

    let mut empty_id = model_payload("  ");
    empty_id["providerModelId"] = json!("  ");
    let (status, body) = send_json(
        app.clone(),
        "POST",
        &format!("/api/providers/{provider_id}/models"),
        empty_id,
    )
    .await;
    assert_eq!(status, 400);
    assert!(body["msg"].as_str().unwrap().contains("模型 ID"));

    let mut zero_context = model_payload("gpt-4o");
    zero_context["contextLength"] = json!(0);
    let (status, body) = send_json(
        app.clone(),
        "POST",
        &format!("/api/providers/{provider_id}/models"),
        zero_context,
    )
    .await;
    assert_eq!(status, 400);
    assert!(body["msg"].as_str().unwrap().contains("上下文长度"));

    let mut zero_output = model_payload("gpt-4o");
    zero_output["maxOutputTokens"] = json!(-1);
    let (status, _) = send_json(
        app.clone(),
        "POST",
        &format!("/api/providers/{provider_id}/models"),
        zero_output,
    )
    .await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn test_create_model_returns_404_for_missing_provider() {
    let (app, _db) = setup_app().await;
    let (status, _) = send_json(
        app,
        "POST",
        "/api/providers/999/models",
        model_payload("gpt-4o"),
    )
    .await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn test_duplicate_model_id_rejected_within_provider_allowed_across_providers() {
    let (app, db) = setup_app().await;
    let p1 = seed_provider(&db, "p1").await;
    let p2 = seed_provider(&db, "p2").await;

    let (status, _) = send_json(
        app.clone(),
        "POST",
        &format!("/api/providers/{p1}/models"),
        model_payload("gpt-4o"),
    )
    .await;
    assert_eq!(status, 201);

    let (status, body) = send_json(
        app.clone(),
        "POST",
        &format!("/api/providers/{p1}/models"),
        model_payload("gpt-4o"),
    )
    .await;
    assert_eq!(status, 400);
    assert!(body["msg"].as_str().unwrap().contains("已存在"));

    let (status, _) = send_json(
        app,
        "POST",
        &format!("/api/providers/{p2}/models"),
        model_payload("gpt-4o"),
    )
    .await;
    assert_eq!(status, 201);
}

#[tokio::test]
async fn test_update_and_delete_provider_model() {
    let (app, db) = setup_app().await;
    let provider_id = seed_provider(&db, "p1").await;
    let (status, created) = send_json(
        app.clone(),
        "POST",
        &format!("/api/providers/{provider_id}/models"),
        model_payload("gpt-4o"),
    )
    .await;
    assert_eq!(status, 201);
    let model_id = created["data"]["modelId"].as_i64().unwrap();

    let (status, body) = send_json(
        app.clone(),
        "PUT",
        &format!("/api/providers/{provider_id}/models/{model_id}"),
        json!({
            "providerModelId": "gpt-4o-2024",
            "contextLength": 256000,
            "maxOutputTokens": 8192,
            "reasoning": false,
            "toolUse": true,
            "imageUnderstand": true,
            "videoUnderstand": false,
        }),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["data"]["contextLength"], 256000);
    assert_eq!(body["data"]["providerModelId"], "gpt-4o-2024");

    let stored = provider_model::Entity::find_by_id(model_id as i32)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.max_output_tokens, 8192);
    assert!(stored.image_understand);
    assert!(!stored.reasoning);

    let (status, _) = send_json(
        app.clone(),
        "DELETE",
        &format!("/api/providers/{provider_id}/models/{model_id}"),
        Value::Null,
    )
    .await;
    assert_eq!(status, 200);
    assert!(
        provider_model::Entity::find_by_id(model_id as i32)
            .one(&db)
            .await
            .unwrap()
            .is_none()
    );

    let (status, _) = send_json(
        app,
        "DELETE",
        &format!("/api/providers/{provider_id}/models/{model_id}"),
        Value::Null,
    )
    .await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn test_update_model_unique_conflict() {
    let (app, db) = setup_app().await;
    let provider_id = seed_provider(&db, "p1").await;
    for id in ["a", "b"] {
        let (status, _) = send_json(
            app.clone(),
            "POST",
            &format!("/api/providers/{provider_id}/models"),
            model_payload(id),
        )
        .await;
        assert_eq!(status, 201, "create {id}");
    }
    let models = provider_model::Entity::find()
        .filter(provider_model::Column::ProviderId.eq(provider_id))
        .all(&db)
        .await
        .unwrap();
    let a = models.iter().find(|m| m.provider_model_id == "a").unwrap();

    let (status, _) = send_json(
        app,
        "PUT",
        &format!("/api/providers/{provider_id}/models/{}", a.model_id),
        model_payload("b"),
    )
    .await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn test_batch_create_skips_existing_and_dedupes() {
    let (app, db) = setup_app().await;
    let provider_id = seed_provider(&db, "p1").await;

    // 预置已存在的 gpt-4o。
    let (status, _) = send_json(
        app.clone(),
        "POST",
        &format!("/api/providers/{provider_id}/models"),
        model_payload("gpt-4o"),
    )
    .await;
    assert_eq!(status, 201);

    let payload = json!({
        "models": [
            model_payload("gpt-4o"),                    // 已存在 → 跳过
            model_payload("gpt-5"),                     // 新增
            model_payload("GPT-5"),                     // 批内尾段重复（忽略大小写）→ 去重
            model_payload("openai/o3"),                 // 新增（含厂商前缀）
        ],
    });
    let (status, body) = send_json(
        app.clone(),
        "POST",
        &format!("/api/providers/{provider_id}/models/batch"),
        payload,
    )
    .await;
    assert_eq!(status, 201);
    let created = body["data"].as_array().unwrap();
    assert_eq!(created.len(), 2, "只应插入 gpt-5 与 openai/o3");

    let all = provider_model::Entity::find()
        .filter(provider_model::Column::ProviderId.eq(provider_id))
        .all(&db)
        .await
        .unwrap();
    assert_eq!(all.len(), 3, "预置 1 + 新增 2");
}

#[tokio::test]
async fn test_batch_create_rejects_invalid_and_empty() {
    let (app, db) = setup_app().await;
    let provider_id = seed_provider(&db, "p1").await;

    let (status, _) = send_json(
        app.clone(),
        "POST",
        &format!("/api/providers/{provider_id}/models/batch"),
        json!({"models": []}),
    )
    .await;
    assert_eq!(status, 400);

    let mut bad = model_payload("x");
    bad["maxOutputTokens"] = json!(0);
    let (status, _) = send_json(
        app,
        "POST",
        &format!("/api/providers/{provider_id}/models/batch"),
        json!({"models": [bad]}),
    )
    .await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn test_list_all_provider_models_contains_provider_id() {
    let (app, db) = setup_app().await;
    let p1 = seed_provider(&db, "p1").await;
    let p2 = seed_provider(&db, "p2").await;
    for (pid, mid) in [(p1, "a"), (p1, "b"), (p2, "c")] {
        let (status, _) = send_json(
            app.clone(),
            "POST",
            &format!("/api/providers/{pid}/models"),
            model_payload(mid),
        )
        .await;
        assert_eq!(status, 201, "create {mid}");
    }

    let (status, body) = send_json(app, "GET", "/api/provider-models", Value::Null).await;
    assert_eq!(status, 200);
    let data = body["data"].as_array().unwrap();
    assert_eq!(data.len(), 3);
    assert!(data.iter().all(|m| m["providerId"].is_i64()));
}

#[tokio::test]
async fn test_delete_provider_cascades_models() {
    let (app, db) = setup_app().await;
    let p1 = seed_provider(&db, "p1").await;
    for mid in ["a", "b"] {
        let (status, _) = send_json(
            app.clone(),
            "POST",
            &format!("/api/providers/{p1}/models"),
            model_payload(mid),
        )
        .await;
        assert_eq!(status, 201, "create {mid}");
    }

    let (status, _) = send_json(
        app.clone(),
        "DELETE",
        &format!("/api/providers/{p1}"),
        Value::Null,
    )
    .await;
    assert_eq!(status, 200);

    let remaining = provider_model::Entity::find()
        .filter(provider_model::Column::ProviderId.eq(p1))
        .all(&db)
        .await
        .unwrap();
    assert!(remaining.is_empty(), "供应商删除后模型应级联硬删");

    let (status, body) = send_json(
        app,
        "GET",
        &format!("/api/providers/{p1}/models"),
        Value::Null,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["data"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn test_refresh_returns_502_for_unreachable_provider() {
    let (app, db) = setup_app().await;
    // 端口 1 的连接会立即被拒绝，用于验证错误透传路径（不做真实网络依赖）。
    let active = provider::ActiveModel {
        name: Set("unreachable".to_string()),
        enable: Set(true),
        base_url: Set("http://127.0.0.1:1".to_string()),
        api_key: Set(crypto::encrypt("sk-test")),
        custom_header: Set("{}".to_string()),
        protocol_type: Set(0),
        billing_mode: Set(0),
        extra: Set("{}".to_string()),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    let provider_id = active.insert(&db).await.unwrap().id;

    let (status, body) = send_json(
        app,
        "POST",
        &format!("/api/providers/{provider_id}/models/refresh"),
        Value::Null,
    )
    .await;
    assert_eq!(status, 502);
    assert!(body["msg"].as_str().unwrap().contains("Models 接口"));
}

#[tokio::test]
async fn test_refresh_returns_404_for_missing_provider() {
    let (app, _db) = setup_app().await;
    let (status, _) = send_json(
        app,
        "POST",
        "/api/providers/999/models/refresh",
        Value::Null,
    )
    .await;
    assert_eq!(status, 404);
}

/// 删除单个供应商模型应级联清理引用它的虚拟模型成员（不残留悬空 virtual_model_item），
/// 与删除供应商的级联语义一致。
#[tokio::test]
async fn test_delete_provider_model_cascades_virtual_model_items() {
    let (app, db) = setup_app().await;
    let provider_id = seed_provider(&db, "p1").await;
    let (status, created) = send_json(
        app.clone(),
        "POST",
        &format!("/api/providers/{provider_id}/models"),
        model_payload("gpt-4o"),
    )
    .await;
    assert_eq!(status, 201);
    let model_id = created["data"]["modelId"].as_i64().unwrap() as i32;

    // 建虚拟模型并挂载该模型。
    let (status, _) = send_json(
        app.clone(),
        "POST",
        "/api/virtual-models",
        json!({
            "displayId": "vm-gpt-4o",
            "loadBalancingStrategy": 3,
            "fallbackStrategy": 1,
            "items": [{"modelId": model_id}],
        }),
    )
    .await;
    assert_eq!(status, 201);

    // 删除模型后，引用它的虚拟模型成员应被级联清理（删除供应商同款事务语义）。
    let (status, _) = send_json(
        app.clone(),
        "DELETE",
        &format!("/api/providers/{provider_id}/models/{model_id}"),
        Value::Null,
    )
    .await;
    assert_eq!(status, 200);

    let orphan_count = virtual_model_item::Entity::find()
        .filter(virtual_model_item::Column::ModelId.eq(model_id))
        .count(&db)
        .await
        .unwrap();
    assert_eq!(orphan_count, 0, "删除供应商模型后不应残留悬空虚拟模型成员");

    // 虚拟模型详情返回的成员列表不再包含该模型。
    let (status, body) = send_json(app.clone(), "GET", "/api/virtual-models", Value::Null).await;
    assert_eq!(status, 200);
    let vms = body["data"].as_array().unwrap();
    let vm = vms.iter().find(|v| v["displayId"] == "vm-gpt-4o").unwrap();
    assert_eq!(vm["items"].as_array().unwrap().len(), 0);
}

// ─── 模型列表刷新走 provider 网络代理 ──────────────────────────────────────────
// 场景：provider 开启网络代理（proxyEnabled + proxyAddr）时，「刷新模型」请求
// 应经 CONNECT 代理转发到供应商 Models 接口，而不是直连。

/// CONNECT/正向代理 mock：收到 CONNECT（隧道）或 `METHOD http://host/path`
/// （http 正向代理，reqwest 对 http 目标走此形式）都转发到目标；统计请求次数。
async fn spawn_connect_proxy() -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let connect_count = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&connect_count);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (mut client, _) = match listener.accept().await {
                Ok(accepted) => accepted,
                Err(_) => break,
            };
            count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            tokio::spawn(async move {
                let mut buf = [0u8; 8192];
                let mut len = 0usize;
                loop {
                    let Ok(n) = client.read(&mut buf[len..]).await else {
                        return;
                    };
                    if n == 0 {
                        return;
                    }
                    len += n;
                    if buf[..len].windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                let head = String::from_utf8_lossy(&buf[..len]);
                let Some(first_line) = head.lines().next() else {
                    return;
                };
                // CONNECT host:port → 隧道。
                if let Some(target) = first_line
                    .strip_prefix("CONNECT ")
                    .and_then(|l| l.split_whitespace().next())
                {
                    let Ok(mut target_stream) = tokio::net::TcpStream::connect(target).await else {
                        return;
                    };
                    let _ = client
                        .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                        .await;
                    let (mut cr, mut cw) = client.split();
                    let (mut tr, mut tw) = target_stream.split();
                    let _ = tokio::join!(
                        tokio::io::copy(&mut cr, &mut tw),
                        tokio::io::copy(&mut tr, &mut cw)
                    );
                    return;
                }
                // 正向代理：`METHOD http://host/path HTTP/1.1` → 转发。
                let Some((method, rest)) = first_line.split_once(' ') else {
                    return;
                };
                let Some((abs_url, version)) = rest.rsplit_once(' ') else {
                    return;
                };
                let Some(parsed) = abs_url.strip_prefix("http://") else {
                    return;
                };
                let Some((host, path)) = parsed.split_once('/') else {
                    return;
                };
                let Ok(mut target_stream) = tokio::net::TcpStream::connect(host).await else {
                    return;
                };
                let rewritten = format!("{method} /{path} {version}\r\n");
                let tail = head.split_once("\r\n").map(|(_, t)| t).unwrap_or("");
                let mut headers = String::new();
                let mut has_host = false;
                for line in tail.lines() {
                    if line.to_ascii_lowercase().starts_with("host:") {
                        has_host = true;
                    }
                    headers.push_str(line);
                    headers.push_str("\r\n");
                }
                let _ = target_stream.write_all(rewritten.as_bytes()).await;
                if !has_host {
                    let _ = target_stream
                        .write_all(format!("Host: {host}\r\n").as_bytes())
                        .await;
                }
                let _ = target_stream.write_all(headers.as_bytes()).await;
                let _ = target_stream.write_all(b"\r\n").await;
                let (mut cr, mut cw) = client.split();
                let (mut tr, mut tw) = target_stream.split();
                let _ = tokio::join!(
                    tokio::io::copy(&mut cr, &mut tw),
                    tokio::io::copy(&mut tr, &mut cw)
                );
            });
        }
    });
    (format!("http://{addr}"), connect_count)
}

/// 目标 mock：返回 OpenAI 风格 models 列表。
async fn spawn_models_mock() -> String {
    let app = axum::Router::new().route(
        "/v1/models",
        axum::routing::get(|| async {
            axum::Json(json!({
                "object": "list",
                "data": [
                    { "id": "gpt-4o", "object": "model" },
                    { "id": "gpt-4o-mini", "object": "model" },
                ]
            }))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

/// 开启网络代理的 provider，「刷新模型」请求应经 CONNECT 代理到达目标。
#[tokio::test]
async fn test_refresh_models_goes_through_provider_proxy() {
    let target = spawn_models_mock().await;
    let (proxy_addr, connect_counter) = spawn_connect_proxy().await;
    let (app, db) = setup_app().await;

    let active = provider::ActiveModel {
        name: Set(format!(
            "proxy-refresh-{}",
            chrono::Utc::now().timestamp_millis()
        )),
        enable: Set(true),
        base_url: Set(format!("{target}/v1")),
        api_key: Set(crypto::encrypt("sk-test")),
        custom_header: Set("{}".to_string()),
        protocol_type: Set(0),
        billing_mode: Set(0),
        extra: Set("{}".to_string()),
        proxy_enabled: Set(true),
        proxy_addr: Set(proxy_addr),
        created_at: Set(chrono::Utc::now()),
        updated_at: Set(chrono::Utc::now()),
        ..Default::default()
    };
    let provider_id = active.insert(&db).await.unwrap().id;

    let (status, body) = send_json(
        app,
        "POST",
        &format!("/api/providers/{provider_id}/models/refresh"),
        Value::Null,
    )
    .await;
    assert_eq!(status, 200, "刷新失败：{body}");
    let ids: Vec<String> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["providerModelId"].as_str().unwrap().to_string())
        .collect();
    assert!(
        ids.contains(&"gpt-4o".to_string()),
        "应解析到 gpt-4o: {ids:?}"
    );
    assert_eq!(
        connect_counter.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "模型刷新应经 CONNECT 代理一次"
    );
}

/// 未开启代理的 provider，「刷新模型」仍直连成功。
#[tokio::test]
async fn test_refresh_models_direct_without_proxy() {
    let target = spawn_models_mock().await;
    let (app, db) = setup_app().await;
    let provider_id = seed_provider(&db, "direct-refresh").await;
    // 把 seed 的 base_url 指向本地 mock（seed 默认 api.example.com 不可达）。
    let row = provider::Entity::find_by_id(provider_id)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    let mut active: provider::ActiveModel = row.into();
    active.base_url = Set(format!("{target}/v1"));
    active.update(&db).await.unwrap();

    let (status, body) = send_json(
        app,
        "POST",
        &format!("/api/providers/{provider_id}/models/refresh"),
        Value::Null,
    )
    .await;
    assert_eq!(status, 200, "直连刷新失败：{body}");
}

use super::*;

use axum::extract::Path;
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::routing::get;

#[derive(Clone)]
struct OpenDesignMock {
    preflight: Arc<std::sync::Mutex<(StatusCode, Value)>>,
    wallet: Arc<std::sync::Mutex<(StatusCode, Value)>>,
    seen: Arc<std::sync::Mutex<Vec<String>>>,
}

fn header(headers: &HeaderMap, name: &str) -> String {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string()
}

async fn spawn_opendesign_mock(
    preflight: (StatusCode, Value),
    wallet: (StatusCode, Value),
) -> (String, OpenDesignMock) {
    let state = OpenDesignMock {
        preflight: Arc::new(std::sync::Mutex::new(preflight)),
        wallet: Arc::new(std::sync::Mutex::new(wallet)),
        seen: Arc::new(std::sync::Mutex::new(Vec::new())),
    };
    let preflight_state = state.clone();
    let wallet_state = state.clone();
    let app = axum::Router::new()
        .route(
            "/api/v1/workspaces/{workspace}/billing/preflight",
            get(move |headers: HeaderMap, Path(workspace): Path<String>| {
                let state = preflight_state.clone();
                async move {
                    state.seen.lock().unwrap().push(format!(
                        "preflight:{workspace}:{}:{}",
                        header(&headers, "authorization"),
                        header(&headers, "x-vela-workspace-id")
                    ));
                    let (status, body) = state.preflight.lock().unwrap().clone();
                    (status, Json(body)).into_response()
                }
            }),
        )
        .route(
            "/api/v1/workspaces/{workspace}/wallet/balance",
            get(move |headers: HeaderMap, Path(workspace): Path<String>| {
                let state = wallet_state.clone();
                async move {
                    state.seen.lock().unwrap().push(format!(
                        "wallet:{workspace}:{}:{}",
                        header(&headers, "authorization"),
                        header(&headers, "x-vela-workspace-id")
                    ));
                    let (status, body) = state.wallet.lock().unwrap().clone();
                    (status, Json(body)).into_response()
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{addr}"), state)
}

fn preflight_with_subscription() -> Value {
    serde_json::json!({
        "workspaceId": "ws-1",
        "workspaceMemberId": "member-1",
        "eligible": true,
        "balanceUsd": "3.50",
        "codingPlan": {
            "workspaceId": "ws-1",
            "workspaceMemberId": "member-1",
            "windows": [
                {
                    "policyId": "p5h",
                    "durationSeconds": 18000,
                    "usedCredits": "250",
                    "remainingCredits": "750",
                    "limitCredits": "1000",
                    "resetsAt": "2026-10-25T10:00:00Z"
                },
                {
                    "policyId": "pweek",
                    "durationSeconds": 604800,
                    "usedCredits": "0",
                    "remainingCredits": "100",
                    "limitCredits": "100",
                    "resetsAt": null
                }
            ]
        }
    })
}

fn wallet_body(amount: &str) -> Value {
    serde_json::json!({
        "billingScopeVersion": 2,
        "workspaceId": "ws-1",
        "workspaceMemberId": "member-1",
        "balanceUsd": amount
    })
}

const OPENDESIGN_EXTRA: &str =
    r#"{"workspace_id":"ws-1","control_key":"ck-1","usage":true,"usage_type":1}"#;

#[tokio::test]
async fn opendesign_usage_combines_subscription_windows_and_wallet() {
    let (mock_base, mock) = spawn_opendesign_mock(
        (StatusCode::OK, preflight_with_subscription()),
        (StatusCode::OK, wallet_body("12.34")),
    )
    .await;

    temp_env::async_with_vars([(OVERRIDE_ENV, Some(mock_base.as_str()))], async {
        let app = setup_app().await;
        let id = create_provider_with_billing(
            &app,
            "OpenDesign-用量",
            "https://amr-link.open-design.ai/v1",
            OPENDESIGN_EXTRA,
            1,
        )
        .await;

        let (status, body) = send(
            &app,
            "GET",
            &format!("/api/providers/{id}/usage?refresh=1"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "查询失败：{body}");
        let data = &body["data"];
        assert_eq!(data["kind"], "quota", "窗口 + 钱包仍以订阅形态展示：{data}");

        let windows = data["windows"].as_array().unwrap();
        assert_eq!(windows.len(), 3, "固定 5h/周/月三槽位：{data}");
        assert_eq!(windows[0]["window"], "five_hour");
        assert_eq!(windows[0]["available"].as_bool(), Some(true));
        assert_eq!(windows[0]["remainingPercent"], 75.0);
        assert!(windows[0]["resetsAt"].as_str().is_some());
        assert_eq!(windows[1]["window"], "weekly");
        assert_eq!(windows[1]["remainingPercent"], 100.0);
        assert_eq!(windows[2]["window"], "monthly");
        assert_eq!(windows[2]["available"].as_bool(), Some(false));

        let balances = data["balances"].as_array().unwrap();
        assert_eq!(balances.len(), 1, "钱包余额随窗口一起返回：{data}");
        assert_eq!(balances[0]["label"], "钱包余额");
        assert_eq!(balances[0]["amount"], 12.34);
        assert_eq!(balances[0]["currency"], "USD");

        let seen = mock.seen.lock().unwrap().join("\n");
        assert!(
            seen.contains("preflight:ws-1:Bearer ck-1:ws-1"),
            "preflight 应带 Control Key 与工作区头：{seen}"
        );
        assert!(
            seen.contains("wallet:ws-1:Bearer ck-1:ws-1"),
            "wallet 应带 Control Key 与工作区头：{seen}"
        );
    })
    .await;
}

#[tokio::test]
async fn opendesign_usage_falls_back_to_preflight_balance_when_wallet_fails() {
    let (mock_base, _mock) = spawn_opendesign_mock(
        (StatusCode::OK, preflight_with_subscription()),
        (StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({})),
    )
    .await;

    temp_env::async_with_vars([(OVERRIDE_ENV, Some(mock_base.as_str()))], async {
        let app = setup_app().await;
        let id = create_provider_with_billing(
            &app,
            "OpenDesign-钱包失败",
            "https://amr-link.open-design.ai/v1",
            OPENDESIGN_EXTRA,
            1,
        )
        .await;

        let (status, body) = send(
            &app,
            "GET",
            &format!("/api/providers/{id}/usage?refresh=1"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "单边失败不应整体失败：{body}");
        let data = &body["data"];
        assert_eq!(data["kind"], "quota");
        assert_eq!(data["windows"][0]["remainingPercent"], 75.0);
        assert_eq!(
            data["balances"][0]["amount"], 3.5,
            "钱包端点失败时回退 preflight.balanceUsd"
        );
    })
    .await;
}

#[tokio::test]
async fn opendesign_usage_returns_wallet_when_no_subscription() {
    let (mock_base, _mock) = spawn_opendesign_mock(
        (
            StatusCode::OK,
            serde_json::json!({
                "workspaceId": "ws-1",
                "eligible": false,
                "windows": [],
                "balanceUsd": "1.00"
            }),
        ),
        (StatusCode::OK, wallet_body("9.50")),
    )
    .await;

    temp_env::async_with_vars([(OVERRIDE_ENV, Some(mock_base.as_str()))], async {
        let app = setup_app().await;
        let id = create_provider_with_billing(
            &app,
            "OpenDesign-无套餐",
            "https://amr-link.open-design.ai/v1",
            OPENDESIGN_EXTRA,
            1,
        )
        .await;

        let (status, body) = send(
            &app,
            "GET",
            &format!("/api/providers/{id}/usage?refresh=1"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "查询失败：{body}");
        let data = &body["data"];
        assert_eq!(data["kind"], "balance");
        assert_eq!(data["balances"][0]["label"], "钱包余额");
        assert_eq!(data["balances"][0]["amount"], 9.5);
    })
    .await;
}

#[tokio::test]
async fn opendesign_usage_both_sides_401_is_client_error() {
    let (mock_base, _mock) = spawn_opendesign_mock(
        (StatusCode::UNAUTHORIZED, serde_json::json!({})),
        (StatusCode::UNAUTHORIZED, serde_json::json!({})),
    )
    .await;

    temp_env::async_with_vars([(OVERRIDE_ENV, Some(mock_base.as_str()))], async {
        let app = setup_app().await;
        let id = create_provider_with_billing(
            &app,
            "OpenDesign-凭据失效",
            "https://amr-link.open-design.ai/v1",
            OPENDESIGN_EXTRA,
            1,
        )
        .await;

        let (status, body) = send(
            &app,
            "GET",
            &format!("/api/providers/{id}/usage?refresh=1"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body["msg"].as_str().unwrap().contains("凭据无效"),
            "应映射为凭据错误：{body}"
        );
    })
    .await;
}

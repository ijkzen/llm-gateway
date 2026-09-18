//! 通知渠道配置路由（`/api/notification`）。
//!
//! 当前只有飞书一个渠道，路由按渠道分路径（`/feishu`），不建通道类型注册表。

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use serde::Deserialize;
use serde_json::json;

use crate::notification;
use crate::response::{self, Response};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/feishu", get(get_feishu).put(update_feishu))
        .route("/feishu/test", post(test_feishu))
        .route("/feishu/register", post(begin_registration))
        .route("/feishu/register/{session_id}", get(registration_status))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateFeishuRequest {
    app_id: String,
    /// 缺省/空串 = 沿用库中原凭据（编辑时不必重输）。
    #[serde(default)]
    app_secret: Option<String>,
    receiver_type: String,
    receiver: String,
    enable: bool,
}

async fn get_feishu(State(state): State<AppState>) -> impl IntoResponse {
    match notification::read_channel(&state.db).await {
        Ok(data) => (StatusCode::OK, Json(Response::success(data))),
        Err(e) => response::db_error(format!("读取通知配置失败：{e}")),
    }
}

async fn update_feishu(
    State(state): State<AppState>,
    Json(req): Json<UpdateFeishuRequest>,
) -> impl IntoResponse {
    if let Err(message) = notification::save_channel(
        &state.db,
        req.app_id,
        req.app_secret,
        req.receiver_type,
        req.receiver,
        req.enable,
    )
    .await
    {
        return response::bad_request(message);
    }

    match notification::read_channel(&state.db).await {
        Ok(data) => (StatusCode::OK, Json(Response::success(data))),
        Err(e) => response::db_error(format!("读取通知配置失败：{e}")),
    }
}

/// 测试发送：用请求体当前值真实发一条消息（不落库配置）。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TestFeishuRequest {
    app_id: String,
    #[serde(default)]
    app_secret: Option<String>,
    receiver_type: String,
    receiver: String,
}

async fn test_feishu(
    State(state): State<AppState>,
    Json(req): Json<TestFeishuRequest>,
) -> impl IntoResponse {
    match notification::send_test(
        &state.db,
        req.app_id,
        req.app_secret,
        req.receiver_type,
        req.receiver,
    )
    .await
    {
        Ok(()) => (
            StatusCode::OK,
            Json(Response::success(json!({ "ok": true }))),
        ),
        Err(message) => response::bad_request(message),
    }
}

/// 发起扫码创建会话，返回二维码内容（`verification_uri_complete` 原样）。
async fn begin_registration(State(state): State<AppState>) -> impl IntoResponse {
    match state.feishu_registration.begin().await {
        Ok(data) => (StatusCode::OK, Json(Response::success(data))),
        Err(message) => response::bad_request(message),
    }
}

/// 轮询扫码状态（终态带回凭据，供前端回填表单）。
async fn registration_status(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    match state.feishu_registration.poll(&session_id).await {
        Ok(data) => (StatusCode::OK, Json(Response::success(data))),
        Err(message) => response::bad_request(message),
    }
}

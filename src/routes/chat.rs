use axum::{
    Json, Router,
    extract::State,
    response::{IntoResponse, Response},
    routing::post,
};
use serde::Deserialize;

use crate::proxy;
use crate::response::bad_request;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().route("/completions", post(chat_completions))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChatRequestBody {
    provider_id: i32,
    model_id: i32,
    messages: Vec<serde_json::Value>,
}

/// POST /api/chat/completions：管理后台聊天直连入口（会话鉴权由 /api 中间件保证）。
async fn chat_completions(
    State(state): State<AppState>,
    Json(body): Json<ChatRequestBody>,
) -> Response {
    if body.messages.is_empty() {
        return bad_request::<()>("消息不能为空").into_response();
    }
    proxy::forward_chat_direct(&state, body.provider_id, body.model_id, body.messages).await
}

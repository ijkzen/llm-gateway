use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
use serde_json::{Value, json};

use crate::auth::{AuthedApiKey, openai_error};
use crate::entity::virtual_model;
use crate::proxy;
use crate::state::AppState;

/// OpenAI 兼容接口中模型的 owned_by 标识。
const OWNED_BY: &str = "llm-gateway";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/models", get(list_models))
        .route("/models/{display_id}", get(get_model))
        .route("/chat/completions", post(chat_completions))
}

/// POST /v1/chat/completions：OpenAI 兼容入口，转发到虚拟模型选中的上游成员。
async fn chat_completions(
    State(state): State<AppState>,
    Extension(api_key): Extension<AuthedApiKey>,
    Json(body): Json<Value>,
) -> Response {
    proxy::forward_chat(&state, api_key, body).await
}

/// OpenAI 格式的单个模型对象。
fn model_object(display_id: &str, created_at: chrono::DateTime<chrono::Utc>) -> Value {
    json!({
        "id": display_id,
        "object": "model",
        "created": created_at.timestamp(),
        "owned_by": OWNED_BY,
    })
}

/// GET /v1/models：返回全部启用的虚拟模型。
async fn list_models(State(state): State<AppState>) -> Response {
    match virtual_model::Entity::find()
        .filter(virtual_model::Column::Enable.eq(true))
        .order_by_asc(virtual_model::Column::VirtualModelId)
        .all(&state.db)
        .await
    {
        Ok(models) => {
            let data: Vec<Value> = models
                .iter()
                .map(|m| model_object(&m.display_id, m.created_at))
                .collect();
            (StatusCode::OK, Json(json!({ "object": "list", "data": data }))).into_response()
        }
        Err(e) => openai_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to list models: {e}"),
            "server_error",
            "internal_error",
        ),
    }
}

/// GET /v1/models/{display_id}：返回指定虚拟模型；不存在或已禁用按 404 处理。
async fn get_model(State(state): State<AppState>, Path(display_id): Path<String>) -> Response {
    let display_id = display_id.trim();
    match virtual_model::Entity::find()
        .filter(virtual_model::Column::DisplayId.eq(display_id))
        .filter(virtual_model::Column::Enable.eq(true))
        .one(&state.db)
        .await
    {
        Ok(Some(model)) => (StatusCode::OK, Json(model_object(&model.display_id, model.created_at))).into_response(),
        Ok(None) => openai_error(
            StatusCode::NOT_FOUND,
            format!("The model '{display_id}' does not exist"),
            "invalid_request_error",
            "model_not_found",
        ),
        Err(e) => openai_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to get model: {e}"),
            "server_error",
            "internal_error",
        ),
    }
}

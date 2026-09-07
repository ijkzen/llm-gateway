use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
use serde_json::{Value, json};

use crate::auth::{AuthedApiKey, openai_error};
use crate::entity::virtual_model::{self, CHAT_SERVED_TYPES};
use crate::proxy;
use crate::state::AppState;

/// OpenAI 兼容接口中模型的 owned_by 标识。
const OWNED_BY: &str = "llm-gateway";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/models", get(list_models))
        .route("/models/{display_id}", get(get_model))
        .route("/chat/completions", post(chat_completions))
        .route("/messages", post(passthrough_messages))
        .route("/responses", post(passthrough_responses))
}

/// POST /v1/chat/completions：OpenAI 兼容入口，转发到虚拟模型选中的上游成员。
async fn chat_completions(
    State(state): State<AppState>,
    Extension(api_key): Extension<AuthedApiKey>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    // 按 allowlist 选出透传的下游头（allowlist 来自设置项
    // `downstream_request_header_allow_list` 的进程内缓存，设置页更新后热生效；
    // 黑名单优先，凭据/框架/hop-by-hop 头已在此被排除）。同一请求多次
    // failover 出站共用同一快照。
    let allowlist = state.settings.downstream_header_allow_list().await;
    let forwarded = proxy::select_forwardable_headers(&headers, &allowlist);
    proxy::forward_chat(&state, api_key, body, forwarded).await
}

/// POST /v1/messages：Anthropic Messages 原生透传（仅 Messages 类型虚拟模型；
/// 鉴权接受 x-api-key 或 Bearer）。
async fn passthrough_messages(
    State(state): State<AppState>,
    Extension(api_key): Extension<AuthedApiKey>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    proxy::forward_native(
        &state,
        api_key,
        proxy::NativeEndpoint::AnthropicMessages,
        &headers,
        body,
    )
    .await
}

/// POST /v1/responses：OpenAI Responses 原生透传（仅 Responses 类型虚拟模型）。
async fn passthrough_responses(
    State(state): State<AppState>,
    Extension(api_key): Extension<AuthedApiKey>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    proxy::forward_native(
        &state,
        api_key,
        proxy::NativeEndpoint::OpenAiResponses,
        &headers,
        body,
    )
    .await
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

/// GET /v1/models：返回全部启用且接口类型可被 chat/completions 服务的虚拟模型
/// （OpenAI Compatible / Full Compatible；Responses/Messages 专用模型不暴露）。
async fn list_models(State(state): State<AppState>) -> Response {
    match virtual_model::Entity::find()
        .filter(virtual_model::Column::Enable.eq(true))
        .filter(virtual_model::Column::InterfaceType.is_in(CHAT_SERVED_TYPES))
        .order_by_asc(virtual_model::Column::VirtualModelId)
        .all(&state.db)
        .await
    {
        Ok(models) => {
            let data: Vec<Value> = models
                .iter()
                .map(|m| model_object(&m.display_id, m.created_at))
                .collect();
            (
                StatusCode::OK,
                Json(json!({ "object": "list", "data": data })),
            )
                .into_response()
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
        .filter(virtual_model::Column::InterfaceType.is_in(CHAT_SERVED_TYPES))
        .one(&state.db)
        .await
    {
        Ok(Some(model)) => (
            StatusCode::OK,
            Json(model_object(&model.display_id, model.created_at)),
        )
            .into_response(),
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

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post, put},
};
use rand::RngCore;
use sea_orm::{ActiveModelTrait, DatabaseConnection, DbErr, EntityTrait, QueryOrder, Set};
use serde::{Deserialize, Serialize};

use crate::auth::hash_token;
use crate::crypto;
use crate::entity::api_key::{self, ActiveModel, Entity};
use crate::i18n::Lang;
use crate::response::{self, Response};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_api_keys))
        .route("/", post(create_api_key))
        .route("/{id}", get(get_api_key_detail))
        .route("/{id}", put(update_api_key))
        .route("/{id}", delete(delete_api_key))
}

/// 生成形如 `lg-` + 32 位随机 hex 的明文密钥。
fn generate_api_key() -> String {
    let mut bytes = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("lg-{hex}")
}

/// 返回给前端的 ApiKey，key 始终脱敏展示（明文仅通过详情接口按需提供）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ApiKeyResponse {
    id: i32,
    name: String,
    /// 掩码后的 key，如 `lg-****abcd`；无法解密时为空字符串。
    key_masked: String,
    enable: bool,
    created_at: String,
    updated_at: String,
}

impl ApiKeyResponse {
    fn from_model(model: api_key::Model) -> Self {
        let key_masked = crypto::decrypt(&model.key)
            .map(|plain| crypto::mask(&plain))
            .unwrap_or_default();
        Self {
            id: model.id,
            name: model.name,
            key_masked,
            enable: model.enable,
            created_at: model.created_at.to_rfc3339(),
            updated_at: model.updated_at.to_rfc3339(),
        }
    }
}

/// 详情响应：在列表字段基础上附带解密后的明文 key。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ApiKeyDetailResponse {
    #[serde(flatten)]
    base: ApiKeyResponse,
    key: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateApiKeyRequest {
    name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateApiKeyRequest {
    enable: bool,
}

async fn list_api_keys(State(state): State<AppState>) -> impl IntoResponse {
    match Entity::find()
        .order_by_asc(api_key::Column::Id)
        .all(&state.db)
        .await
    {
        Ok(models) => {
            let response: Vec<ApiKeyResponse> =
                models.into_iter().map(ApiKeyResponse::from_model).collect();
            (StatusCode::OK, Json(Response::success(response)))
        }
        Err(e) => response::db_error(e.to_string()),
    }
}

async fn create_api_key(
    State(state): State<AppState>,
    Json(req): Json<CreateApiKeyRequest>,
) -> impl IntoResponse {
    let lang = state.settings.lang().await;
    let name = req.name.trim();
    if name.is_empty() {
        return response::bad_request(lang.tr("名称不能为空", "name cannot be empty"));
    }

    let plain = generate_api_key();
    let now = chrono::Utc::now();
    let active = ActiveModel {
        name: Set(name.to_string()),
        key: Set(crypto::encrypt(&plain)),
        key_hash: Set(Some(hash_token(&plain))),
        enable: Set(true),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };

    match active.insert(&state.db).await {
        Ok(model) => {
            let response = ApiKeyResponse::from_model(model);
            (StatusCode::CREATED, Json(Response::success(response)))
        }
        Err(e) if crate::db::is_unique_violation(&e) => {
            let msg = lang.tr(
                "同名 API Key 已存在，名称需要唯一",
                "an API key with the same name already exists; names must be unique",
            );
            response::bad_request(msg)
        }
        Err(e) => response::db_error(e.to_string()),
    }
}

async fn load_detail(
    db: &DatabaseConnection,
    id: i32,
) -> Result<Option<ApiKeyDetailResponse>, DbErr> {
    Ok(Entity::find_by_id(id).one(db).await?.map(|model| {
        let plain = crypto::decrypt(&model.key).unwrap_or_default();
        ApiKeyDetailResponse {
            key: plain,
            base: ApiKeyResponse::from_model(model),
        }
    }))
}

async fn get_api_key_detail(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> impl IntoResponse {
    let lang = state.settings.lang().await;
    match load_detail(&state.db, id).await {
        Ok(Some(detail)) => (StatusCode::OK, Json(Response::success(detail))).into_response(),
        Ok(None) => not_found_api_key(lang, id).into_response(),
        Err(e) => response::db_error::<()>(e.to_string()).into_response(),
    }
}

fn not_found_api_key(lang: Lang, id: i32) -> crate::response::ErrorResponse<()> {
    if lang == Lang::En {
        response::not_found(format!("API key {id} does not exist"))
    } else {
        response::not_found(format!("API Key {id} 不存在"))
    }
}

async fn update_api_key(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Json(req): Json<UpdateApiKeyRequest>,
) -> impl IntoResponse {
    let lang = state.settings.lang().await;
    let model = match Entity::find_by_id(id).one(&state.db).await {
        Ok(Some(model)) => model,
        Ok(None) => return not_found_api_key(lang, id).into_response(),
        Err(e) => return response::db_error::<()>(e.to_string()).into_response(),
    };

    let mut active: ActiveModel = model.into();
    active.enable = Set(req.enable);
    active.updated_at = Set(chrono::Utc::now());

    match active.update(&state.db).await {
        Ok(model) => {
            let response = ApiKeyResponse::from_model(model);
            (StatusCode::OK, Json(Response::success(response))).into_response()
        }
        Err(e) => response::db_error::<()>(e.to_string()).into_response(),
    }
}

async fn delete_api_key(State(state): State<AppState>, Path(id): Path<i32>) -> impl IntoResponse {
    let lang = state.settings.lang().await;
    match Entity::delete_by_id(id).exec(&state.db).await {
        Ok(result) if result.rows_affected > 0 => (StatusCode::OK, Json(Response::success(()))),
        Ok(_) => not_found_api_key(lang, id),
        Err(e) => response::db_error(e.to_string()),
    }
}

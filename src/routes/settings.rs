use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, put},
};
use sea_orm::{ActiveModelTrait, EntityTrait, Set};
use serde::{Deserialize, Serialize};

use crate::entity::setting;
use crate::response::{self, Response};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_settings))
        .route("/{key}", put(update_setting))
}

#[derive(Serialize)]
struct SettingResponse {
    key: String,
    value: String,
    r#type: String,
    updated_at: String,
}

impl From<setting::Model> for SettingResponse {
    fn from(model: setting::Model) -> Self {
        let type_str = match setting::SettingType::try_from(model.r#type) {
            Ok(t) => t.to_string(),
            Err(_) => "Unknown".to_string(),
        };
        Self {
            key: model.key,
            value: model.value,
            r#type: type_str,
            updated_at: model.updated_at.to_rfc3339(),
        }
    }
}

#[derive(Deserialize)]
struct UpdateSettingRequest {
    value: String,
}

/// Validates `value` against the setting's declared type. String (and
/// unknown legacy type values) accept anything.
fn validate_setting_value(setting_type: i32, value: &str) -> Result<(), &'static str> {
    match setting::SettingType::try_from(setting_type) {
        Ok(setting::SettingType::Int) => {
            if value.trim().parse::<i64>().is_err() {
                return Err("value 必须是有效的整数");
            }
        }
        Ok(setting::SettingType::Float) => {
            if value.trim().parse::<f64>().is_err() {
                return Err("value 必须是有效的数字");
            }
        }
        Ok(setting::SettingType::Bool) => {
            if !matches!(value, "true" | "false") {
                return Err("value 必须是 true 或 false");
            }
        }
        _ => {}
    }
    Ok(())
}

async fn list_settings(State(state): State<AppState>) -> impl IntoResponse {
    match setting::Entity::find().all(&state.db).await {
        Ok(settings) => {
            let response: Vec<SettingResponse> = settings.into_iter().map(Into::into).collect();
            (StatusCode::OK, Json(Response::success(response)))
        }
        Err(e) => response::db_error(e.to_string()),
    }
}

async fn update_setting(
    State(state): State<AppState>,
    Path(key): Path<String>,
    Json(req): Json<UpdateSettingRequest>,
) -> impl IntoResponse {
    match setting::Entity::find_by_id(&key).one(&state.db).await {
        Ok(Some(model)) => {
            if let Err(msg) = validate_setting_value(model.r#type, &req.value) {
                return response::bad_request(msg);
            }

            let mut active: setting::ActiveModel = model.into();
            active.value = Set(req.value);
            active.updated_at = Set(chrono::Utc::now());

            match active.update(&state.db).await {
                Ok(_) => (StatusCode::OK, Json(Response::success(()))),
                Err(e) => response::db_error(e.to_string()),
            }
        }
        Ok(None) => response::not_found(format!("设置 '{key}' 不存在")),
        Err(e) => response::db_error(e.to_string()),
    }
}

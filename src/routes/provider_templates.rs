use axum::{Json, Router, extract::State, response::IntoResponse, routing::post};
use serde::{Deserialize, Serialize};

use crate::entity::provider_template;
use crate::provider_template::find_by_domain;
use crate::response::{self, Response};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().route("/match", post(match_template))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MatchTemplateRequest {
    base_url: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TemplateResponse {
    name: String,
    base_url: String,
    protocol_type: i32,
    billing_mode: i32,
    extra: String,
}

impl From<provider_template::Model> for TemplateResponse {
    fn from(model: provider_template::Model) -> Self {
        Self {
            name: model.name,
            base_url: model.base_url,
            protocol_type: model.protocol_type,
            billing_mode: model.billing_mode,
            extra: model.extra,
        }
    }
}

/// 按 Base URL 匹配 provider 模板；未命中返回 404 与中文提示。
async fn match_template(
    State(state): State<AppState>,
    Json(req): Json<MatchTemplateRequest>,
) -> impl IntoResponse {
    let base_url = req.base_url.trim();
    if base_url.is_empty() {
        return response::bad_request("Base URL 不能为空");
    }
    match find_by_domain(&state.db, base_url).await {
        Ok(Some(template)) => {
            (axum::http::StatusCode::OK, Json(Response::success(TemplateResponse::from(template))))
        }
        Ok(None) => response::not_found("未找到匹配的模板"),
        Err(e) => response::db_error(e.to_string()),
    }
}

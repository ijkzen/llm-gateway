use axum::{Json, Router, extract::State, response::IntoResponse, routing::post};
use serde::{Deserialize, Serialize};

use crate::entity::provider_template;
use crate::provider_template::find_by_domain_all;
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
    /// 模板默认 custom_header（JSON；按 base_url host 从
    /// `provider_template::template_default_headers` 计算，无默认为 "{}"）。
    custom_header: String,
}

impl From<provider_template::Model> for TemplateResponse {
    fn from(model: provider_template::Model) -> Self {
        let host = crate::provider_template::host_of(&model.base_url).unwrap_or_default();
        let defaults = crate::provider_template::template_default_headers(&host);
        let custom_header = if defaults.is_empty() {
            "{}".to_string()
        } else {
            let map: serde_json::Map<String, serde_json::Value> = defaults
                .into_iter()
                .map(|(name, value)| (name.to_string(), serde_json::Value::String(value)))
                .collect();
            serde_json::Value::Object(map).to_string()
        };
        Self {
            name: model.name,
            base_url: model.base_url,
            protocol_type: model.protocol_type,
            billing_mode: model.billing_mode,
            extra: model.extra,
            custom_header,
        }
    }
}

/// 按 Base URL 匹配 provider 模板，返回全部命中（同一 host 可能有多个模板）；
/// 未命中返回 404。
async fn match_template(
    State(state): State<AppState>,
    Json(req): Json<MatchTemplateRequest>,
) -> impl IntoResponse {
    let lang = state.settings.lang().await;
    let base_url = req.base_url.trim();
    if base_url.is_empty() {
        return response::bad_request(lang.tr("Base URL 不能为空", "Base URL cannot be empty"));
    }
    match find_by_domain_all(&state.db, base_url).await {
        Ok(templates) if !templates.is_empty() => {
            let list: Vec<TemplateResponse> =
                templates.into_iter().map(TemplateResponse::from).collect();
            (axum::http::StatusCode::OK, Json(Response::success(list)))
        }
        Ok(_) => response::not_found(lang.tr("未找到匹配的模板", "no matching template found")),
        Err(e) => response::db_error(e.to_string()),
    }
}

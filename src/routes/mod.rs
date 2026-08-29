mod cron_jobs;
mod openai_compat;
mod provider_models;
mod provider_templates;
mod providers;
mod settings;
mod virtual_models;

use axum::Json;
use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::get;
use serde_json::json;

use crate::middleware;
use crate::state::AppState;

pub fn create_app() -> Router<AppState> {
    let router = Router::new()
        .route("/api/healthz", get(healthz))
        .nest("/api/cron-jobs", cron_jobs::routes())
        .nest("/api/settings", settings::routes())
        .nest("/api/providers", providers::routes())
        .nest("/api/provider-templates", provider_templates::routes())
        .nest("/api/provider-models", provider_models::global_routes())
        .nest("/api/virtual-models", virtual_models::routes())
        .nest("/v1", openai_compat::routes())
        .fallback(crate::static_assets::serve_asset)
        .layer(DefaultBodyLimit::max(5 * 1024 * 1024));

    middleware::apply(router)
}

async fn healthz() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
}

mod api_keys;
mod auth;
mod cron_jobs;
mod openai_compat;
mod provider_models;
mod provider_templates;
mod providers;
mod settings;
mod stats;
mod virtual_models;

use axum::Json;
use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::middleware;
use axum::routing::get;
use serde_json::json;

use crate::middleware as http_middleware;
use crate::state::AppState;

/// 组装完整应用路由（含登录拦截中间件）。
pub fn create_app(state: &AppState) -> Router {
    let router = Router::new()
        .route("/api/healthz", get(healthz))
        .nest("/api/auth", auth::routes())
        .nest("/api/cron-jobs", cron_jobs::routes())
        .nest("/api/settings", settings::routes())
        .nest("/api/providers", providers::routes())
        .nest("/api/provider-templates", provider_templates::routes())
        .nest("/api/provider-models", provider_models::global_routes())
        .nest("/api/virtual-models", virtual_models::routes())
        .nest("/api/api-keys", api_keys::routes())
        .nest("/api/stats", stats::routes())
        .nest("/v1", openai_compat::routes())
        .fallback(crate::static_assets::serve_asset)
        .layer(DefaultBodyLimit::max(5 * 1024 * 1024))
        .layer(middleware::from_fn_with_state(state.clone(), crate::auth::auth_middleware))
        .with_state(state.clone());

    http_middleware::apply(router)
}

async fn healthz() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
}

//! 对外只读接口（Bearer API Key 鉴权）：供应商 ID 列表与按 ID 查用量。
//!
//! 挂在 `/v1` 命名空间下，由 `auth_middleware` 对 `/v1/*` 统一做 Bearer 校验
//! （api_key 表），供脚本/第三方调用，不涉及 Cookie Session。用量查询与
//! 管理后台 `/api/providers/{id}/usage` 共享同一核心（`providers::provider_usage_response`），
//! 缓存/刷新/错误口径完全一致。

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
};
use sea_orm::{EntityTrait, QueryOrder};
use serde::Serialize;

use super::providers::{ProviderUsageQuery, provider_usage_response, refresh_forced};
use crate::entity::provider::{self, Entity};
use crate::response::{self, Response};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/providers", get(list_providers_open))
        .route("/providers/{id}/usage", get(get_provider_usage_open))
}

/// 对外暴露的供应商条目：仅 ID/名称/开关/计费模式/用量支持标记，
/// 不返回 api_key、base_url、custom_header、extra 等敏感配置。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderOpenResponse {
    id: i32,
    name: String,
    enable: bool,
    /// 付费模式：0=按量付费、1=订阅制。
    billing_mode: i32,
    /// extra.usage=true 时为 true，标记该供应商可调用用量查询接口。
    usage_supported: bool,
}

/// GET /v1/providers：列出全部供应商（含停用），供调用方拿到 id 后查询用量。
async fn list_providers_open(State(state): State<AppState>) -> impl IntoResponse {
    match Entity::find()
        .order_by_asc(provider::Column::SortOrder)
        .order_by_asc(provider::Column::Id)
        .all(&state.db)
        .await
    {
        Ok(models) => {
            let data: Vec<ProviderOpenResponse> = models
                .into_iter()
                .map(|m| ProviderOpenResponse {
                    id: m.id,
                    name: m.name,
                    enable: m.enable,
                    billing_mode: m.billing_mode,
                    usage_supported: crate::usage::usage_enabled(&m.extra),
                })
                .collect();
            (StatusCode::OK, Json(Response::success(data)))
        }
        Err(e) => response::db_error(e.to_string()),
    }
}

/// GET /v1/providers/{id}/usage：行为同管理后台用量接口
/// （10 分钟数据库缓存优先，`?refresh=1` 强制重取上游）。
async fn get_provider_usage_open(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Query(query): Query<ProviderUsageQuery>,
) -> impl IntoResponse {
    provider_usage_response(&state, id, refresh_forced(query.refresh.as_deref())).await
}

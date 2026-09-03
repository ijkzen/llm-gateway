use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post, put},
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QueryOrder,
    Set, TransactionTrait,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashSet;

use crate::crypto;
use crate::entity::provider;
use crate::entity::provider_model::{self, ActiveModel, Entity};
use crate::entity::virtual_model_item;
use crate::i18n::Lang;
use crate::provider_model::{catalog, refresh};
use crate::proxy;
use crate::response::{self, Response};
use crate::state::AppState;

/// 供应商详情弹窗与刷新共用的时间戳格式化。
fn rfc3339(value: chrono::DateTime<chrono::Utc>) -> String {
    value.to_rfc3339()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderModelResponse {
    model_id: i32,
    provider_id: i32,
    provider_model_id: String,
    context_length: i64,
    max_output_tokens: i64,
    reasoning: bool,
    tool_use: bool,
    image_understand: bool,
    video_understand: bool,
    created_at: String,
    updated_at: String,
}

impl ProviderModelResponse {
    fn from_model(model: provider_model::Model) -> Self {
        Self {
            model_id: model.model_id,
            provider_id: model.provider_id,
            provider_model_id: model.provider_model_id,
            context_length: model.context_length,
            max_output_tokens: model.max_output_tokens,
            reasoning: model.reasoning,
            tool_use: model.tool_use,
            image_understand: model.image_understand,
            video_understand: model.video_understand,
            created_at: rfc3339(model.created_at),
            updated_at: rfc3339(model.updated_at),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpsertProviderModelRequest {
    provider_model_id: String,
    context_length: i64,
    max_output_tokens: i64,
    #[serde(default)]
    reasoning: bool,
    #[serde(default)]
    tool_use: bool,
    #[serde(default)]
    image_understand: bool,
    #[serde(default)]
    video_understand: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BatchCreateRequest {
    models: Vec<UpsertProviderModelRequest>,
}

/// 刷新候选：match_state 为 `smart`（已智能填充）/ `partial`（信息不完整）/ `manual`（需手动填写）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RefreshCandidate {
    provider_model_id: String,
    match_state: &'static str,
    context_length: Option<i64>,
    max_output_tokens: Option<i64>,
    reasoning: bool,
    tool_use: bool,
    image_understand: bool,
    video_understand: bool,
}

/// 挂载在 `/api/providers/{provider_id}/models` 下的供应商作用域路由。
pub fn scoped_routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_provider_models))
        .route("/", post(create_provider_model))
        .route("/batch", post(batch_create_provider_models))
        .route("/refresh", post(refresh_provider_models))
        .route("/{model_id}", put(update_provider_model))
        .route("/{model_id}", delete(delete_provider_model))
        .route("/{model_id}/test", post(test_provider_model))
}

/// 挂载在 `/api/provider-models` 下的全局路由（页面按供应商分组渲染用）。
pub fn global_routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_all_provider_models))
        .route("/catalog/search", get(search_catalog))
}

/// 校验创建/更新字段，返回第一个错误消息（None 表示通过）。
fn validate_fields(req: &UpsertProviderModelRequest, lang: Lang) -> Option<String> {
    if req.provider_model_id.trim().is_empty() {
        return Some(
            lang.tr("模型 ID 不能为空", "model ID cannot be empty")
                .to_string(),
        );
    }
    if req.context_length <= 0 {
        return Some(
            lang.tr(
                "上下文长度必须为正整数",
                "context length must be a positive integer",
            )
            .to_string(),
        );
    }
    if req.max_output_tokens <= 0 {
        return Some(
            lang.tr(
                "最大输出必须为正整数",
                "max output tokens must be a positive integer",
            )
            .to_string(),
        );
    }
    None
}

async fn ensure_provider_exists(db: &DatabaseConnection, provider_id: i32) -> Result<bool, DbErr> {
    Ok(provider::Entity::find_by_id(provider_id)
        .one(db)
        .await?
        .is_some())
}

async fn list_provider_models(
    State(state): State<AppState>,
    Path(provider_id): Path<i32>,
) -> impl IntoResponse {
    match Entity::find()
        .filter(provider_model::Column::ProviderId.eq(provider_id))
        .order_by_asc(provider_model::Column::ModelId)
        .all(&state.db)
        .await
    {
        Ok(models) => {
            let response: Vec<ProviderModelResponse> = models
                .into_iter()
                .map(ProviderModelResponse::from_model)
                .collect();
            (StatusCode::OK, Json(Response::success(response)))
        }
        Err(e) => response::db_error(e.to_string()),
    }
}

async fn list_all_provider_models(State(state): State<AppState>) -> impl IntoResponse {
    match Entity::find()
        .order_by_asc(provider_model::Column::ProviderId)
        .order_by_asc(provider_model::Column::ModelId)
        .all(&state.db)
        .await
    {
        Ok(models) => {
            let response: Vec<ProviderModelResponse> = models
                .into_iter()
                .map(ProviderModelResponse::from_model)
                .collect();
            (StatusCode::OK, Json(Response::success(response)))
        }
        Err(e) => response::db_error(e.to_string()),
    }
}

/// 关键词搜索内嵌模型目录（手动添加时的联想下拉）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogSearchResponse {
    id: String,
    name: String,
    family: String,
    context_length: Option<i64>,
    max_output_tokens: Option<i64>,
    reasoning: bool,
    tool_use: bool,
    image_understand: bool,
    video_understand: bool,
}

impl From<catalog::CatalogCandidate> for CatalogSearchResponse {
    fn from(c: catalog::CatalogCandidate) -> Self {
        Self {
            id: c.id,
            name: c.name,
            family: c.family,
            context_length: c.entry.context_length,
            max_output_tokens: c.entry.max_output_tokens,
            reasoning: c.entry.reasoning,
            tool_use: c.entry.tool_use,
            image_understand: c.entry.image_understand,
            video_understand: c.entry.video_understand,
        }
    }
}

/// `GET /api/provider-models/catalog/search?q=<关键词>&limit=<N>`
async fn search_catalog(
    axum::extract::Query(query): axum::extract::Query<SearchCatalogQuery>,
) -> impl IntoResponse {
    let q = query.q.unwrap_or_default();
    let limit = query.limit.unwrap_or(8).clamp(1, 30);
    let hits: Vec<CatalogSearchResponse> = catalog::search(&q, limit)
        .into_iter()
        .map(CatalogSearchResponse::from)
        .collect();
    (StatusCode::OK, Json(Response::success(hits)))
}

#[derive(Deserialize)]
struct SearchCatalogQuery {
    q: Option<String>,
    limit: Option<usize>,
}

async fn create_provider_model(
    State(state): State<AppState>,
    Path(provider_id): Path<i32>,
    Json(req): Json<UpsertProviderModelRequest>,
) -> impl IntoResponse {
    let lang = state.settings.lang().await;
    if let Some(msg) = validate_fields(&req, lang) {
        return response::bad_request(msg);
    }
    match ensure_provider_exists(&state.db, provider_id).await {
        Ok(true) => {}
        Ok(false) => return not_found_provider(lang, provider_id),
        Err(e) => return response::db_error(e.to_string()),
    }

    let now = chrono::Utc::now();
    let active = ActiveModel {
        provider_id: Set(provider_id),
        provider_model_id: Set(req.provider_model_id.trim().to_string()),
        context_length: Set(req.context_length),
        max_output_tokens: Set(req.max_output_tokens),
        reasoning: Set(req.reasoning),
        tool_use: Set(req.tool_use),
        image_understand: Set(req.image_understand),
        video_understand: Set(req.video_understand),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };

    match active.insert(&state.db).await {
        Ok(model) => {
            tracing::info!(
                model_id = model.model_id,
                provider_id = model.provider_id,
                provider_model_id = %model.provider_model_id,
                context_length = model.context_length,
                max_output_tokens = model.max_output_tokens,
                reasoning = model.reasoning,
                tool_use = model.tool_use,
                image_understand = model.image_understand,
                video_understand = model.video_understand,
                "创建供应商模型",
            );
            let response = ProviderModelResponse::from_model(model);
            (StatusCode::CREATED, Json(Response::success(response)))
        }
        Err(e) if is_unique_violation(&e) => response::bad_request(duplicate_model_message(lang)),
        Err(e) => response::db_error(e.to_string()),
    }
}

async fn batch_create_provider_models(
    State(state): State<AppState>,
    Path(provider_id): Path<i32>,
    Json(req): Json<BatchCreateRequest>,
) -> impl IntoResponse {
    let lang = state.settings.lang().await;
    if req.models.is_empty() {
        return response::bad_request(lang.tr("至少选择一个模型", "select at least one model"));
    }
    for item in &req.models {
        if let Some(msg) = validate_fields(item, lang) {
            return response::bad_request(msg);
        }
    }
    match ensure_provider_exists(&state.db, provider_id).await {
        Ok(true) => {}
        Ok(false) => return not_found_provider(lang, provider_id),
        Err(e) => return response::db_error(e.to_string()),
    }

    // 批内按尾段去重（保留首个）；已存在于库中的（尾段忽略大小写）跳过。
    let mut seen: HashSet<String> = HashSet::new();
    let mut pending: Vec<&UpsertProviderModelRequest> = Vec::new();
    for item in &req.models {
        if seen.insert(tail_key(&item.provider_model_id)) {
            pending.push(item);
        }
    }
    let existing = match Entity::find()
        .filter(provider_model::Column::ProviderId.eq(provider_id))
        .all(&state.db)
        .await
    {
        Ok(models) => models
            .into_iter()
            .map(|m| tail_key(&m.provider_model_id))
            .collect::<HashSet<_>>(),
        Err(e) => return response::db_error(e.to_string()),
    };
    let pending: Vec<&UpsertProviderModelRequest> = pending
        .into_iter()
        .filter(|item| !existing.contains(&tail_key(&item.provider_model_id)))
        .collect();

    if pending.is_empty() {
        return (
            StatusCode::OK,
            Json(Response::success(Vec::<ProviderModelResponse>::new())),
        );
    }

    let txn = match state.db.begin().await {
        Ok(txn) => txn,
        Err(e) => return response::db_error(e.to_string()),
    };
    let now = chrono::Utc::now();
    let mut created: Vec<ProviderModelResponse> = Vec::with_capacity(pending.len());
    for item in pending {
        let active = ActiveModel {
            provider_id: Set(provider_id),
            provider_model_id: Set(item.provider_model_id.trim().to_string()),
            context_length: Set(item.context_length),
            max_output_tokens: Set(item.max_output_tokens),
            reasoning: Set(item.reasoning),
            tool_use: Set(item.tool_use),
            image_understand: Set(item.image_understand),
            video_understand: Set(item.video_understand),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        match active.insert(&txn).await {
            Ok(model) => created.push(ProviderModelResponse::from_model(model)),
            Err(e) => {
                let _ = txn.rollback().await;
                if is_unique_violation(&e) {
                    return response::bad_request(duplicate_model_message(lang));
                }
                return response::db_error(e.to_string());
            }
        }
    }
    match txn.commit().await {
        Ok(()) => {
            tracing::info!(
                provider_id,
                created_count = created.len(),
                model_ids = ?created
                    .iter()
                    .map(|m| m.model_id)
                    .collect::<Vec<_>>(),
                "批量创建供应商模型",
            );
            (StatusCode::CREATED, Json(Response::success(created)))
        }
        Err(e) => response::db_error(e.to_string()),
    }
}

async fn update_provider_model(
    State(state): State<AppState>,
    Path((provider_id, model_id)): Path<(i32, i32)>,
    Json(req): Json<UpsertProviderModelRequest>,
) -> impl IntoResponse {
    let lang = state.settings.lang().await;
    if let Some(msg) = validate_fields(&req, lang) {
        return response::bad_request(msg);
    }
    let model = match Entity::find()
        .filter(provider_model::Column::ProviderId.eq(provider_id))
        .filter(provider_model::Column::ModelId.eq(model_id))
        .one(&state.db)
        .await
    {
        Ok(Some(model)) => model,
        Ok(None) => return not_found_model(lang, model_id),
        Err(e) => return response::db_error(e.to_string()),
    };

    let mut active: ActiveModel = model.into();
    active.provider_model_id = Set(req.provider_model_id.trim().to_string());
    active.context_length = Set(req.context_length);
    active.max_output_tokens = Set(req.max_output_tokens);
    active.reasoning = Set(req.reasoning);
    active.tool_use = Set(req.tool_use);
    active.image_understand = Set(req.image_understand);
    active.video_understand = Set(req.video_understand);
    active.updated_at = Set(chrono::Utc::now());

    match active.update(&state.db).await {
        Ok(model) => {
            tracing::info!(
                model_id = model.model_id,
                provider_id = model.provider_id,
                provider_model_id = %model.provider_model_id,
                context_length = model.context_length,
                max_output_tokens = model.max_output_tokens,
                reasoning = model.reasoning,
                tool_use = model.tool_use,
                image_understand = model.image_understand,
                video_understand = model.video_understand,
                "更新供应商模型",
            );
            let response = ProviderModelResponse::from_model(model);
            (StatusCode::OK, Json(Response::success(response)))
        }
        Err(e) if is_unique_violation(&e) => response::bad_request(duplicate_model_message(lang)),
        Err(e) => response::db_error(e.to_string()),
    }
}

async fn delete_provider_model(
    State(state): State<AppState>,
    Path((provider_id, model_id)): Path<(i32, i32)>,
) -> impl IntoResponse {
    let lang = state.settings.lang().await;
    // 级联清理引用该模型的虚拟模型成员，避免 virtual_model_item 悬空；
    // 与删除供应商的事务模式一致，模型不存在时回滚不误删成员。
    let txn = match state.db.begin().await {
        Ok(txn) => txn,
        Err(e) => return response::db_error(e.to_string()),
    };
    let deleted_item_count = match virtual_model_item::Entity::delete_many()
        .filter(virtual_model_item::Column::ModelId.eq(model_id))
        .exec(&txn)
        .await
    {
        Ok(result) => result.rows_affected,
        Err(e) => return response::db_error(e.to_string()),
    };
    match Entity::delete_many()
        .filter(provider_model::Column::ProviderId.eq(provider_id))
        .filter(provider_model::Column::ModelId.eq(model_id))
        .exec(&txn)
        .await
    {
        Ok(result) if result.rows_affected > 0 => match txn.commit().await {
            Ok(()) => {
                tracing::info!(
                    provider_id,
                    model_id,
                    deleted_virtual_item_count = deleted_item_count,
                    "删除供应商模型（级联清理虚拟模型成员）"
                );
                (StatusCode::OK, Json(Response::success(())))
            }
            Err(e) => response::db_error(e.to_string()),
        },
        Ok(_) => {
            if let Err(e) = txn.rollback().await {
                tracing::warn!("回滚删除供应商模型失败：{e}");
            }
            not_found_model(lang, model_id)
        }
        Err(e) => response::db_error(e.to_string()),
    }
}

async fn refresh_provider_models(
    State(state): State<AppState>,
    Path(provider_id): Path<i32>,
) -> impl IntoResponse {
    let lang = state.settings.lang().await;
    let provider_model_data = match provider::Entity::find_by_id(provider_id)
        .one(&state.db)
        .await
    {
        Ok(Some(model)) => model,
        Ok(None) => return not_found_provider(lang, provider_id),
        Err(e) => return response::db_error(e.to_string()),
    };
    let api_key = match crypto::decrypt(&provider_model_data.api_key) {
        Ok(key) if !key.is_empty() => key,
        Ok(_) => {
            return response::bad_request(lang.tr(
                "该供应商未配置 API Key，无法刷新",
                "this provider has no API key configured; cannot refresh",
            ));
        }
        Err(_) => {
            return response::bad_request(lang.tr(
                "API Key 解密失败，请重新保存供应商密钥后再刷新",
                "failed to decrypt the API key; re-save the provider key and try again",
            ));
        }
    };

    let remote_ids = match refresh::fetch_remote_model_ids(
        &provider_model_data.base_url,
        provider_model_data.protocol_type,
        &api_key,
        &provider_model_data.custom_header,
    )
    .await
    {
        Ok(ids) => ids,
        Err(msg) => return response::scheduler_error(StatusCode::BAD_GATEWAY, msg),
    };

    // 已导入的模型按尾段忽略大小写排除，不再出现在候选中。
    let existing = match Entity::find()
        .filter(provider_model::Column::ProviderId.eq(provider_id))
        .all(&state.db)
        .await
    {
        Ok(models) => models
            .into_iter()
            .map(|m| tail_key(&m.provider_model_id))
            .collect::<HashSet<_>>(),
        Err(e) => return response::db_error(e.to_string()),
    };

    let mut seen: HashSet<String> = HashSet::new();
    let mut candidates: Vec<RefreshCandidate> = Vec::new();
    for id in remote_ids {
        let id = id.trim();
        if id.is_empty() {
            continue;
        }
        if existing.contains(&tail_key(id)) || !seen.insert(tail_key(id)) {
            continue;
        }
        let candidate = match catalog::find_by_suffix(id) {
            Some(entry) => RefreshCandidate {
                provider_model_id: id.to_string(),
                match_state: if entry.is_complete() {
                    "smart"
                } else {
                    "partial"
                },
                context_length: entry.context_length,
                max_output_tokens: entry.max_output_tokens,
                reasoning: entry.reasoning,
                tool_use: entry.tool_use,
                image_understand: entry.image_understand,
                video_understand: entry.video_understand,
            },
            None => RefreshCandidate {
                provider_model_id: id.to_string(),
                match_state: "manual",
                context_length: None,
                max_output_tokens: None,
                reasoning: false,
                tool_use: false,
                image_understand: false,
                video_understand: false,
            },
        };
        candidates.push(candidate);
    }

    // 排序：先按匹配状态（smart → partial → manual），组内再按尾段字典序。
    candidates.sort_by(|a, b| {
        let rank = |s: &str| match s {
            "smart" => 0,
            "partial" => 1,
            _ => 2,
        };
        rank(a.match_state)
            .cmp(&rank(b.match_state))
            .then_with(|| tail_key(&a.provider_model_id).cmp(&tail_key(&b.provider_model_id)))
    });

    (StatusCode::OK, Json(Response::success(candidates)))
}

/// POST /api/providers/{provider_id}/models/{model_id}/test
/// 手动构建一条最小化测试请求发往该模型的上游，验证模型可用性。
/// 成功/失败均写入 request 表（与正式流量同口径，计入数据面板）。
async fn test_provider_model(
    State(state): State<AppState>,
    Path((provider_id, model_id)): Path<(i32, i32)>,
) -> impl IntoResponse {
    let lang = state.settings.lang().await;

    let provider_row = match provider::Entity::find_by_id(provider_id)
        .one(&state.db)
        .await
    {
        Ok(Some(row)) => row,
        Ok(None) => return not_found_provider(lang, provider_id),
        Err(e) => return response::db_error(e.to_string()),
    };
    let model = match Entity::find()
        .filter(provider_model::Column::ProviderId.eq(provider_id))
        .filter(provider_model::Column::ModelId.eq(model_id))
        .one(&state.db)
        .await
    {
        Ok(Some(model)) => model,
        Ok(None) => return not_found_model(lang, model_id),
        Err(e) => return response::db_error(e.to_string()),
    };

    let api_key = match crypto::decrypt(&provider_row.api_key) {
        Ok(key) if !key.is_empty() => key,
        Ok(_) => {
            return response::bad_request(lang.tr(
                "该供应商未配置 API Key，无法测试",
                "this provider has no API key configured; cannot test",
            ));
        }
        Err(_) => {
            return response::bad_request(lang.tr(
                "API Key 解密失败，请重新保存供应商密钥后再测试",
                "failed to decrypt the API key; re-save the provider key and try again",
            ));
        }
    };

    match proxy::test_model(&state, &provider_row, &model, &api_key).await {
        Ok(duration_ms) => (
            StatusCode::OK,
            Json(Response::success(
                json!({ "ok": true, "duration_ms": duration_ms }),
            )),
        ),
        Err(message) => response::scheduler_error(StatusCode::BAD_GATEWAY, message),
    }
}

/// 模型 ID 的尾段归一化 key：按 `/` 取最后一段并转小写。
fn tail_key(model_id: &str) -> String {
    model_id
        .trim()
        .rsplit('/')
        .next()
        .unwrap_or("")
        .to_lowercase()
}

/// SQLite 唯一约束冲突（provider_model 的复合唯一索引）。
fn is_unique_violation(err: &DbErr) -> bool {
    err.to_string().contains("UNIQUE constraint failed")
}

fn not_found_provider<T>(lang: Lang, provider_id: i32) -> crate::response::ErrorResponse<T> {
    if lang == Lang::En {
        response::not_found(format!("provider {provider_id} does not exist"))
    } else {
        response::not_found(format!("Provider {provider_id} 不存在"))
    }
}

fn not_found_model<T>(lang: Lang, model_id: i32) -> crate::response::ErrorResponse<T> {
    if lang == Lang::En {
        response::not_found(format!("provider model {model_id} does not exist"))
    } else {
        response::not_found(format!("供应商模型 {model_id} 不存在"))
    }
}

fn duplicate_model_message(lang: Lang) -> String {
    lang.tr(
        "该供应商下已存在同名模型 ID",
        "a model with the same ID already exists under this provider",
    )
    .to_string()
}

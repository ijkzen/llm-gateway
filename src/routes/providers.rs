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
use serde_json::Value;

use crate::crypto;
use crate::entity::provider::{self, ActiveModel, Entity};
use crate::entity::provider_model;
use crate::entity::virtual_model_item;
use crate::response::{self, Response};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_providers))
        .route("/", post(create_provider))
        .route("/{id}", get(get_provider_detail))
        .route("/{id}", put(update_provider))
        .route("/{id}", delete(delete_provider))
        .nest(
            "/{provider_id}/models",
            crate::routes::provider_models::scoped_routes(),
        )
}

/// 模板 extra 中用于标记"是否支持用量查询"的字段与取值。
const EXTRA_USAGE_KEY: &str = "usage";

/// 返回给前端的 Provider，api_key 始终脱敏展示（明文仅通过解密动作按需提供）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderResponse {
    id: i32,
    name: String,
    enable: bool,
    base_url: String,
    /// 掩码后的 api_key，如 `sk-****abcd`；无法解密时为空字符串。
    api_key_masked: String,
    status: i32,
    protocol_type: i32,
    billing_mode: i32,
    custom_header: String,
    extra: String,
    created_at: String,
    updated_at: String,
}

impl ProviderResponse {
    fn from_model(model: provider::Model) -> Self {
        let api_key_masked = mask_api_key(&model.api_key);
        Self {
            id: model.id,
            name: model.name,
            enable: model.enable,
            base_url: model.base_url,
            api_key_masked,
            status: model.status,
            protocol_type: model.protocol_type,
            billing_mode: model.billing_mode,
            custom_header: model.custom_header,
            extra: model.extra,
            created_at: model.created_at.to_rfc3339(),
            updated_at: model.updated_at.to_rfc3339(),
        }
    }
}

/// 详情响应：在列表字段基础上附带解密后的明文 api_key。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderDetailResponse {
    #[serde(flatten)]
    base: ProviderResponse,
    api_key: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateProviderRequest {
    name: String,
    enable: bool,
    base_url: String,
    api_key: String,
    protocol_type: i32,
    billing_mode: i32,
    custom_header: String,
    extra: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateProviderRequest {
    name: Option<String>,
    enable: Option<bool>,
    base_url: Option<String>,
    /// 编辑时为空字符串表示不修改现有密钥。
    api_key: Option<String>,
    protocol_type: Option<i32>,
    billing_mode: Option<i32>,
    custom_header: Option<String>,
    extra: Option<String>,
}

/// 校验协议类型与付费模式是否在合法枚举范围内。
fn validate_protocol_billing(protocol_type: i32, billing_mode: i32) -> Option<String> {
    if !(0..=3).contains(&protocol_type) {
        return Some("协议类型不合法".to_string());
    }
    if !(0..=1).contains(&billing_mode) {
        return Some("付费类型不合法".to_string());
    }
    None
}

/// 校验创建/更新的公共字段，返回第一个错误消息（None 表示通过）。
fn validate_fields(
    name: &str,
    base_url: &str,
    custom_header: &str,
    extra: &str,
    api_key: Option<&str>,
) -> Option<String> {
    if name.trim().is_empty() {
        return Some("名称不能为空".to_string());
    }
    if base_url.trim().is_empty() {
        return Some("Base URL 不能为空".to_string());
    }
    if let Some(key) = api_key
        && key.trim().is_empty()
    {
        return Some("API Key 不能为空".to_string());
    }
    if let Some(err) = validate_json_field("自定义请求头", custom_header) {
        return Some(err);
    }
    if let Some(err) = validate_json_field("额外字段", extra) {
        return Some(err);
    }
    None
}

/// extra 校验：必须是合法 JSON 对象；当 usage 开启时，模板中值为空的
/// 推荐字段必须全部填写（允许 `usage`/`usage_type` 这类标记字段本身为空）。
fn validate_extra(extra: &str) -> Option<String> {
    let parsed = match serde_json::from_str::<Value>(extra) {
        Ok(Value::Object(map)) => map,
        Ok(_) => return Some("额外字段必须是 JSON 对象".to_string()),
        Err(e) => return Some(format!("额外字段不是合法的 JSON：{e}")),
    };

    let usage_enabled = parsed
        .get(EXTRA_USAGE_KEY)
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !usage_enabled {
        return None;
    }

    let empty_required: Vec<String> = parsed
        .iter()
        .filter(|(key, val)| {
            key.as_str() != EXTRA_USAGE_KEY
                && key.as_str() != "usage_type"
                && val.as_str().is_some_and(|s| s.trim().is_empty())
        })
        .map(|(key, _)| key.clone())
        .collect();

    if empty_required.is_empty() {
        None
    } else {
        Some(format!(
            "用量查询已开启，请填写以下字段：{}",
            empty_required.join("、")
        ))
    }
}

/// 校验字段值是否为合法 JSON（允许 `{}` 空对象）。
fn validate_json_field(label: &str, value: &str) -> Option<String> {
    if serde_json::from_str::<Value>(value).is_err() {
        Some(format!("{label}不是合法的 JSON"))
    } else {
        None
    }
}

/// 对 api_key 做掩码：保留前 3 位与后 4 位，中间用星号填充；
/// 解密失败（密钥变更等原因）时返回空字符串。
fn mask_api_key(stored: &str) -> String {
    let Ok(plain) = crypto::decrypt(stored) else {
        return String::new();
    };
    if plain.is_empty() {
        return String::new();
    }
    let bytes = plain.as_bytes();
    if bytes.len() <= 7 {
        return "*".repeat(bytes.len());
    }
    let head: String = plain.chars().take(3).collect();
    let tail: String = plain.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
    format!("{head}****{tail}")
}

async fn load_detail(db: &DatabaseConnection, id: i32) -> Result<Option<ProviderDetailResponse>, DbErr> {
    Ok(Entity::find_by_id(id)
        .one(db)
        .await?
        .map(|model| {
            let plain = crypto::decrypt(&model.api_key).unwrap_or_default();
            ProviderDetailResponse {
                api_key: plain,
                base: ProviderResponse::from_model(model),
            }
        }))
}

async fn list_providers(State(state): State<AppState>) -> impl IntoResponse {
    match Entity::find().order_by_asc(provider::Column::Id).all(&state.db).await {
        Ok(models) => {
            let response: Vec<ProviderResponse> =
                models.into_iter().map(ProviderResponse::from_model).collect();
            (StatusCode::OK, Json(Response::success(response)))
        }
        Err(e) => response::db_error(e.to_string()),
    }
}

async fn create_provider(
    State(state): State<AppState>,
    Json(req): Json<CreateProviderRequest>,
) -> impl IntoResponse {
    let api_key = req.api_key.trim();
    if let Some(msg) = validate_fields(
        &req.name,
        &req.base_url,
        &req.custom_header,
        &req.extra,
        Some(api_key),
    ) {
        return response::bad_request(msg);
    }
    if let Some(msg) = validate_extra(&req.extra) {
        return response::bad_request(msg);
    }
    if let Some(msg) = validate_protocol_billing(req.protocol_type, req.billing_mode) {
        return response::bad_request(msg);
    }

    let now = chrono::Utc::now();
    let active = ActiveModel {
        name: Set(req.name.trim().to_string()),
        enable: Set(req.enable),
        base_url: Set(req.base_url.trim().to_string()),
        api_key: Set(crypto::encrypt(api_key)),
        custom_header: Set(req.custom_header),
        extra: Set(req.extra),
        status: Set(0),
        protocol_type: Set(req.protocol_type),
        billing_mode: Set(req.billing_mode),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };

    match active.insert(&state.db).await {
        Ok(model) => {
            let response = ProviderResponse::from_model(model);
            (StatusCode::CREATED, Json(Response::success(response)))
        }
        Err(e) if is_unique_violation(&e) => {
            response::bad_request("同名 Provider 已存在，名称需要唯一")
        }
        Err(e) => response::db_error(e.to_string()),
    }
}

async fn update_provider(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Json(req): Json<UpdateProviderRequest>,
) -> impl IntoResponse {
    let model = match Entity::find_by_id(id).one(&state.db).await {
        Ok(Some(model)) => model,
        Ok(None) => return response::not_found(format!("Provider {id} 不存在")),
        Err(e) => return response::db_error(e.to_string()),
    };

    let name = req.name.unwrap_or(model.name.clone());
    let base_url = req.base_url.unwrap_or_else(|| model.base_url.clone());
    let custom_header = req.custom_header.unwrap_or_else(|| model.custom_header.clone());
    let extra = req.extra.unwrap_or_else(|| model.extra.clone());
    // 空字符串表示"不修改"，其余值覆盖。
    let new_api_key = req
        .api_key
        .filter(|k| !k.trim().is_empty())
        .map(|k| k.trim().to_string());
    let _api_key = new_api_key.clone().unwrap_or_else(|| model.api_key.clone());

    if let Some(msg) = validate_fields(&name, &base_url, &custom_header, &extra, None) {
        return response::bad_request(msg);
    }
    if let Some(msg) = validate_extra(&extra) {
        return response::bad_request(msg);
    }
    let protocol_type = req.protocol_type.unwrap_or(model.protocol_type);
    let billing_mode = req.billing_mode.unwrap_or(model.billing_mode);
    if let Some(msg) = validate_protocol_billing(protocol_type, billing_mode) {
        return response::bad_request(msg);
    }

    let mut active: ActiveModel = model.into();
    active.name = Set(name.trim().to_string());
    active.enable = Set(req.enable.unwrap_or(active.enable.unwrap()));
    active.base_url = Set(base_url.trim().to_string());
    active.protocol_type = Set(protocol_type);
    active.billing_mode = Set(billing_mode);
    // 仅在提交了新密钥时重新加密。
    if let Some(plain) = new_api_key {
        active.api_key = Set(crypto::encrypt(&plain));
    }
    active.custom_header = Set(custom_header);
    active.extra = Set(extra);
    active.updated_at = Set(chrono::Utc::now());

    match active.update(&state.db).await {
        Ok(model) => {
            let response = ProviderResponse::from_model(model);
            (StatusCode::OK, Json(Response::success(response)))
        }
        Err(e) if is_unique_violation(&e) => {
            response::bad_request("同名 Provider 已存在，名称需要唯一")
        }
        Err(e) => response::db_error(e.to_string()),
    }
}

async fn delete_provider(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> impl IntoResponse {
    // 级联硬删：同一事务内先删引用该供应商模型的虚拟模型成员（释放成员），
    // 再删该供应商名下全部模型，最后删供应商本身，避免 virtual_model_item 悬空。
    let txn = match state.db.begin().await {
        Ok(txn) => txn,
        Err(e) => return response::db_error(e.to_string()),
    };
    let model_ids: Vec<i32> = match provider_model::Entity::find()
        .filter(provider_model::Column::ProviderId.eq(id))
        .all(&txn)
        .await
    {
        Ok(models) => models.into_iter().map(|pm| pm.model_id).collect(),
        Err(e) => return response::db_error(e.to_string()),
    };
    if !model_ids.is_empty()
        && let Err(e) = virtual_model_item::Entity::delete_many()
            .filter(virtual_model_item::Column::ModelId.is_in(model_ids))
            .exec(&txn)
            .await
    {
        return response::db_error(e.to_string());
    }
    if let Err(e) = provider_model::Entity::delete_many()
        .filter(provider_model::Column::ProviderId.eq(id))
        .exec(&txn)
        .await
    {
        return response::db_error(e.to_string());
    }
    match Entity::delete_by_id(id).exec(&txn).await {
        Ok(result) if result.rows_affected > 0 => match txn.commit().await {
            Ok(()) => (StatusCode::OK, Json(Response::success(()))),
            Err(e) => response::db_error(e.to_string()),
        },
        Ok(_) => response::not_found(format!("Provider {id} 不存在")),
        Err(e) => response::db_error(e.to_string()),
    }
}

async fn get_provider_detail(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> impl IntoResponse {
    match load_detail(&state.db, id).await {
        Ok(Some(detail)) => (StatusCode::OK, Json(Response::success(detail))),
        Ok(None) => response::not_found(format!("Provider {id} 不存在")),
        Err(e) => response::db_error(e.to_string()),
    }
}

/// SQLite 唯一约束冲突（name UNIQUE）。
fn is_unique_violation(err: &DbErr) -> bool {
    err.to_string().contains("UNIQUE constraint failed")
}

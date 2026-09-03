use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post, put},
};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseConnection, DbBackend, DbErr, EntityTrait, QueryFilter,
    QueryOrder, Set, Statement, TransactionTrait,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

use crate::crypto;
use crate::entity::provider::{self, ActiveModel, Entity};
use crate::entity::provider_model;
use crate::entity::virtual_model_item;
use crate::i18n::Lang;
use crate::response::{self, Response};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_providers))
        .route("/", post(create_provider))
        .route("/reorder", put(reorder_providers))
        .route("/{id}", get(get_provider_detail))
        .route("/{id}", put(update_provider))
        .route("/{id}", delete(delete_provider))
        .route("/{id}/api-key", get(get_provider_api_key))
        .route("/{id}/usage", get(get_provider_usage))
        .route("/{id}/usage/estimate", get(get_provider_usage_estimate))
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
    protocol_type: i32,
    billing_mode: i32,
    custom_header: String,
    extra: String,
    /// 是否经网络代理转发该供应商请求。
    proxy_enabled: bool,
    /// HTTP 代理地址（如 `http://127.0.0.1:7890`）。
    proxy_addr: String,
    created_at: String,
    updated_at: String,
}

impl ProviderResponse {
    fn from_model(model: provider::Model) -> Self {
        let api_key_masked = mask_api_key(&model.api_key);
        let extra = crypto::decrypt_or_passthrough(&model.extra);
        Self {
            id: model.id,
            name: model.name,
            enable: model.enable,
            base_url: model.base_url,
            api_key_masked,
            protocol_type: model.protocol_type,
            billing_mode: model.billing_mode,
            custom_header: model.custom_header,
            extra,
            proxy_enabled: model.proxy_enabled,
            proxy_addr: model.proxy_addr,
            created_at: model.created_at.to_rfc3339(),
            updated_at: model.updated_at.to_rfc3339(),
        }
    }
}

/// 明文 API Key 响应：仅通过 `GET /api/providers/{id}/api-key` 按需返回。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderApiKeyResponse {
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
    #[serde(default)]
    proxy_enabled: bool,
    #[serde(default)]
    proxy_addr: String,
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
    /// 列表排序权重（越小越靠前），批量重排请使用 PUT /reorder。
    sort_order: Option<i32>,
    #[serde(default)]
    proxy_enabled: Option<bool>,
    #[serde(default)]
    proxy_addr: Option<String>,
}

/// 校验协议类型与付费模式是否在合法枚举范围内。
fn validate_protocol_billing(protocol_type: i32, billing_mode: i32, lang: Lang) -> Option<String> {
    if !(0..=3).contains(&protocol_type) {
        return Some(
            lang.tr("协议类型不合法", "invalid protocol type")
                .to_string(),
        );
    }
    if !(0..=1).contains(&billing_mode) {
        return Some(
            lang.tr("付费类型不合法", "invalid billing mode")
                .to_string(),
        );
    }
    None
}

/// 校验创建/更新的公共字段，返回第一个错误消息（None 表示通过）。
#[allow(clippy::too_many_arguments)]
fn validate_fields(
    name: &str,
    base_url: &str,
    custom_header: &str,
    extra: &str,
    api_key: Option<&str>,
    lang: Lang,
) -> Option<String> {
    if name.trim().is_empty() {
        return Some(lang.tr("名称不能为空", "name cannot be empty").to_string());
    }
    if base_url.trim().is_empty() {
        return Some(
            lang.tr("Base URL 不能为空", "Base URL cannot be empty")
                .to_string(),
        );
    }
    if let Some(key) = api_key
        && key.trim().is_empty()
    {
        return Some(
            lang.tr("API Key 不能为空", "API Key cannot be empty")
                .to_string(),
        );
    }
    if let Some(err) = validate_json_field(
        lang.tr("自定义请求头", "custom headers"),
        custom_header,
        lang,
    ) {
        return Some(err);
    }
    if let Some(err) = validate_json_field(lang.tr("额外字段", "extra fields"), extra, lang) {
        return Some(err);
    }
    None
}

/// 校验网络代理配置：开启时必须提供 http:// 代理地址（无认证）。
fn validate_proxy(proxy_enabled: bool, proxy_addr: &str, lang: Lang) -> Option<String> {
    let addr = proxy_addr.trim();
    if !proxy_enabled {
        // 未开启时不校验地址（允许留空）。
        return None;
    }
    if addr.is_empty() {
        return Some(
            lang.tr(
                "开启网络代理时必须填写代理地址",
                "proxy address is required when proxy is enabled",
            )
            .to_string(),
        );
    }
    if !addr.starts_with("http://") {
        return Some(
            lang.tr(
                "代理地址需以 http:// 开头",
                "proxy address must start with http://",
            )
            .to_string(),
        );
    }
    // 无认证：不允许 user:pass@ 形式。
    if addr.contains('@') {
        return Some(
            lang.tr(
                "暂不支持带认证的代理地址",
                "proxy address with authentication is not supported yet",
            )
            .to_string(),
        );
    }
    None
}

/// extra 校验：必须是合法 JSON 对象；当 usage 开启时，模板中值为空的
/// 推荐字段必须全部填写（允许 `usage`/`usage_type` 这类标记字段本身为空）。
fn validate_extra(extra: &str, lang: Lang) -> Option<String> {
    let parsed = match serde_json::from_str::<Value>(extra) {
        Ok(Value::Object(map)) => map,
        Ok(_) => {
            return Some(
                lang.tr(
                    "额外字段必须是 JSON 对象",
                    "extra fields must be a JSON object",
                )
                .to_string(),
            );
        }
        Err(e) => {
            let msg = if lang == Lang::En {
                format!("extra fields are not valid JSON: {e}")
            } else {
                format!("额外字段不是合法的 JSON：{e}")
            };
            return Some(msg);
        }
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
        let joined = empty_required.join("、");
        if lang == Lang::En {
            Some(format!(
                "usage query is enabled; fill in the following fields: {}",
                empty_required.join(", ")
            ))
        } else {
            Some(format!("用量查询已开启，请填写以下字段：{joined}"))
        }
    }
}

/// 校验字段值是否为合法 JSON（允许 `{}` 空对象）。
fn validate_json_field(label: &str, value: &str, lang: Lang) -> Option<String> {
    if serde_json::from_str::<Value>(value).is_err() {
        Some(if lang == Lang::En {
            format!("{label} is not valid JSON")
        } else {
            format!("{label}不是合法的 JSON")
        })
    } else {
        None
    }
}

/// 对 api_key 做掩码：保留前 3 位与后 4 位，中间用星号填充；
/// 解密失败（密钥变更等原因）时返回空字符串。
fn mask_api_key(stored: &str) -> String {
    match crypto::decrypt(stored) {
        Ok(plain) => crypto::mask(&plain),
        Err(_) => String::new(),
    }
}

/// 详情响应：与列表一致，api_key 始终脱敏（明文仅通过 /api-key 端点按需提供）。
async fn load_detail(db: &DatabaseConnection, id: i32) -> Result<Option<ProviderResponse>, DbErr> {
    Ok(Entity::find_by_id(id)
        .one(db)
        .await?
        .map(ProviderResponse::from_model))
}

async fn list_providers(State(state): State<AppState>) -> impl IntoResponse {
    match Entity::find()
        .order_by_asc(provider::Column::SortOrder)
        .order_by_asc(provider::Column::Id)
        .all(&state.db)
        .await
    {
        Ok(models) => {
            let response: Vec<ProviderResponse> = models
                .into_iter()
                .map(ProviderResponse::from_model)
                .collect();
            (StatusCode::OK, Json(Response::success(response)))
        }
        Err(e) => response::db_error(e.to_string()),
    }
}

async fn create_provider(
    State(state): State<AppState>,
    Json(req): Json<CreateProviderRequest>,
) -> impl IntoResponse {
    let lang = state.settings.lang().await;
    let api_key = req.api_key.trim();
    if let Some(msg) = validate_fields(
        &req.name,
        &req.base_url,
        &req.custom_header,
        &req.extra,
        Some(api_key),
        lang,
    ) {
        return response::bad_request(msg);
    }
    if let Some(msg) = validate_extra(&req.extra, lang) {
        return response::bad_request(msg);
    }
    if let Some(msg) = validate_protocol_billing(req.protocol_type, req.billing_mode, lang) {
        return response::bad_request(msg);
    }
    if let Some(msg) = validate_proxy(req.proxy_enabled, &req.proxy_addr, lang) {
        return response::bad_request(msg);
    }

    let now = chrono::Utc::now();
    let active = ActiveModel {
        name: Set(req.name.trim().to_string()),
        enable: Set(req.enable),
        base_url: Set(req.base_url.trim().to_string()),
        api_key: Set(crypto::encrypt(api_key)),
        custom_header: Set(req.custom_header),
        extra: Set(crypto::encrypt(&req.extra)),
        protocol_type: Set(req.protocol_type),
        billing_mode: Set(req.billing_mode),
        proxy_enabled: Set(req.proxy_enabled),
        proxy_addr: Set(req.proxy_addr.trim().to_string()),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };

    match crate::provider_repo::insert_provider(&state.db, active).await {
        Ok(model) => {
            let response = ProviderResponse::from_model(model);
            (StatusCode::CREATED, Json(Response::success(response)))
        }
        Err(e) if is_unique_violation(&e) => {
            let msg = lang.tr(
                "同名 Provider 已存在，名称需要唯一",
                "a provider with the same name already exists; names must be unique",
            );
            response::bad_request(msg)
        }
        Err(e) => response::db_error(e.to_string()),
    }
}

async fn update_provider(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Json(req): Json<UpdateProviderRequest>,
) -> impl IntoResponse {
    let lang = state.settings.lang().await;
    let model = match Entity::find_by_id(id).one(&state.db).await {
        Ok(Some(model)) => model,
        Ok(None) => {
            let msg = if lang == Lang::En {
                format!("provider {id} does not exist")
            } else {
                format!("Provider {id} 不存在")
            };
            return response::not_found(msg);
        }
        Err(e) => return response::db_error(e.to_string()),
    };

    let name = req.name.unwrap_or(model.name.clone());
    let base_url = req.base_url.unwrap_or_else(|| model.base_url.clone());
    let custom_header = req
        .custom_header
        .unwrap_or_else(|| model.custom_header.clone());
    let has_new_extra = req.extra.is_some();
    // 未提交新 extra 时跳过 extra 校验（存储值已通过历史校验，且可能为密文无法直接解析）。
    let extra_for_validate = req.extra.as_deref().unwrap_or("{}");
    let enable_new = req.enable.unwrap_or(model.enable);
    let enable_changed = enable_new != model.enable;
    // 空字符串表示"不修改"，其余值覆盖。
    let new_api_key = req
        .api_key
        .filter(|k| !k.trim().is_empty())
        .map(|k| k.trim().to_string());
    let _api_key = new_api_key.clone().unwrap_or_else(|| model.api_key.clone());

    if let Some(msg) = validate_fields(
        &name,
        &base_url,
        &custom_header,
        extra_for_validate,
        None,
        lang,
    ) {
        return response::bad_request(msg);
    }
    if has_new_extra && let Some(msg) = validate_extra(extra_for_validate, lang) {
        return response::bad_request(msg);
    }
    let protocol_type = req.protocol_type.unwrap_or(model.protocol_type);
    let billing_mode = req.billing_mode.unwrap_or(model.billing_mode);
    if let Some(msg) = validate_protocol_billing(protocol_type, billing_mode, lang) {
        return response::bad_request(msg);
    }
    // 代理字段：未传（None）保持原值；传了则校验（开启时地址必填 + http:// 格式）。
    let proxy_enabled = req.proxy_enabled.unwrap_or(model.proxy_enabled);
    let proxy_addr = req.proxy_addr.unwrap_or_else(|| model.proxy_addr.clone());
    if let Some(msg) = validate_proxy(proxy_enabled, &proxy_addr, lang) {
        return response::bad_request(msg);
    }

    let had_failure_disabled = model.failure_disabled;
    let mut active: ActiveModel = model.into();
    active.name = Set(name.trim().to_string());
    active.enable = Set(enable_new);
    // 手动启用即解除连续失败禁用（清除熔断标记并清零内存计数）。
    if enable_new && had_failure_disabled {
        active.failure_disabled = Set(false);
        state.failure_counter.reset(id);
        tracing::info!(provider_id = id, "手动启用供应商，清除连续失败禁用标记");
    }
    active.base_url = Set(base_url.trim().to_string());
    active.protocol_type = Set(protocol_type);
    active.billing_mode = Set(billing_mode);
    // 仅在提交了新密钥时重新加密。
    if let Some(plain) = new_api_key {
        active.api_key = Set(crypto::encrypt(&plain));
    }
    active.custom_header = Set(custom_header);
    // 仅在提交了新 extra 时加密写回；未提交时保持存储值（可能已是密文）。
    if let Some(plain) = &req.extra {
        active.extra = Set(crypto::encrypt(plain));
    }
    active.sort_order = Set(req.sort_order.unwrap_or(active.sort_order.unwrap()));
    active.proxy_enabled = Set(proxy_enabled);
    active.proxy_addr = Set(proxy_addr.trim().to_string());
    active.updated_at = Set(chrono::Utc::now());

    match crate::provider_repo::update_provider(&state.db, active).await {
        Ok(model) => {
            // 手动切换启用状态时级联同步该供应商名下全部虚拟模型子模型
            // （与用量额度门控共用 set_items_enabled，保证成员排序/编辑态一致）。
            if enable_changed
                && let Err(e) =
                    crate::provider_repo::set_items_enabled(&state.db, id, enable_new).await
            {
                tracing::warn!(provider_id = id, "级联更新虚拟模型子模型启用状态失败：{e}");
            }
            // 凭据/字段可能变化，失效（内存 + 数据库）用量缓存避免展示旧结果。
            state.usage_cache.invalidate(id).await;
            if let Err(e) = crate::usage::persist::invalidate_usage_cache(&state.db, id).await {
                tracing::warn!(provider_id = id, "用量缓存失效失败：{e}");
            }
            let response = ProviderResponse::from_model(model);
            (StatusCode::OK, Json(Response::success(response)))
        }
        Err(e) if is_unique_violation(&e) => {
            response::bad_request("同名 Provider 已存在，名称需要唯一")
        }
        Err(e) => response::db_error(e.to_string()),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReorderProvidersRequest {
    /// 目标顺序的 Provider id 列表（可只包含需要调整的部分）。
    ids: Vec<i32>,
}

/// 批量重排供应商列表顺序：按 ids 数组下标写入 sort_order（0 起）。
/// 允许部分重排（未传入的 id 保持原相对顺序）；id 缺失、重复或不存在则整体回滚。
async fn reorder_providers(
    State(state): State<AppState>,
    Json(req): Json<ReorderProvidersRequest>,
) -> impl IntoResponse {
    let lang = state.settings.lang().await;
    if req.ids.is_empty() {
        return response::bad_request(
            lang.tr("排序列表不能为空", "the reorder list cannot be empty"),
        );
    }
    let mut seen = HashSet::new();
    for id in &req.ids {
        if !seen.insert(*id) {
            let msg = if lang == Lang::En {
                format!("provider {id} appears more than once in the reorder list")
            } else {
                format!("排序列表中 Provider {id} 重复")
            };
            return response::bad_request(msg);
        }
    }

    let txn = match state.db.begin().await {
        Ok(txn) => txn,
        Err(e) => return response::db_error(e.to_string()),
    };

    // 先校验全部 id 存在，再统一写入，保证原子性。
    let found: Vec<provider::Model> = match Entity::find()
        .filter(provider::Column::Id.is_in(req.ids.clone()))
        .all(&txn)
        .await
    {
        Ok(models) => models,
        Err(e) => return response::db_error(e.to_string()),
    };
    let found_ids: HashSet<i32> = found.into_iter().map(|m| m.id).collect();
    if let Some(missing) = req.ids.iter().find(|id| !found_ids.contains(id)) {
        let msg = if lang == Lang::En {
            format!("provider {missing} does not exist")
        } else {
            format!("Provider {missing} 不存在")
        };
        return response::not_found(msg);
    }

    for (index, id) in req.ids.iter().enumerate() {
        if let Err(e) = Entity::update_many()
            .filter(provider::Column::Id.eq(*id))
            .col_expr(provider::Column::SortOrder, (index as i32).into())
            .exec(&txn)
            .await
        {
            return response::db_error(e.to_string());
        }
    }

    match txn.commit().await {
        Ok(()) => (StatusCode::OK, Json(Response::success(()))),
        Err(e) => response::db_error(e.to_string()),
    }
}

async fn delete_provider(State(state): State<AppState>, Path(id): Path<i32>) -> impl IntoResponse {
    let lang = state.settings.lang().await;
    // 先查原记录（日志需要 name），不存在直接 404。
    let provider = match provider::Entity::find_by_id(id).one(&state.db).await {
        Ok(Some(model)) => model,
        Ok(None) => {
            let msg = if lang == Lang::En {
                format!("provider {id} does not exist")
            } else {
                format!("Provider {id} 不存在")
            };
            return response::not_found(msg);
        }
        Err(e) => return response::db_error(e.to_string()),
    };
    // 级联硬删：同一事务内先删引用该供应商模型的虚拟模型成员（释放成员），
    // 再删该供应商名下全部模型，最后删供应商本身，避免 virtual_model_item 悬空。
    let txn = match state.db.begin().await {
        Ok(txn) => txn,
        Err(e) => return response::db_error(e.to_string()),
    };
    let mut deleted_item_count: u64 = 0;
    let model_ids: Vec<i32> = match provider_model::Entity::find()
        .filter(provider_model::Column::ProviderId.eq(id))
        .all(&txn)
        .await
    {
        Ok(models) => models.into_iter().map(|pm| pm.model_id).collect(),
        Err(e) => return response::db_error(e.to_string()),
    };
    if !model_ids.is_empty() {
        match virtual_model_item::Entity::delete_many()
            .filter(virtual_model_item::Column::ModelId.is_in(model_ids))
            .exec(&txn)
            .await
        {
            Ok(result) => deleted_item_count = result.rows_affected,
            Err(e) => return response::db_error(e.to_string()),
        }
    }
    let deleted_model_count = match provider_model::Entity::delete_many()
        .filter(provider_model::Column::ProviderId.eq(id))
        .exec(&txn)
        .await
    {
        Ok(result) => result.rows_affected,
        Err(e) => return response::db_error(e.to_string()),
    };
    match crate::provider_repo::delete_provider(&txn, provider).await {
        Ok(()) => match txn.commit().await {
            Ok(()) => {
                tracing::info!(
                    provider_id = id,
                    deleted_model_count,
                    deleted_item_count,
                    "删除供应商（级联删除名下模型与虚拟模型成员）"
                );
                state.usage_cache.invalidate(id).await;
                if let Err(e) = crate::usage::persist::invalidate_usage_cache(&state.db, id).await {
                    tracing::warn!(provider_id = id, "用量缓存失效失败：{e}");
                }
                (StatusCode::OK, Json(Response::success(())))
            }
            Err(e) => response::db_error(e.to_string()),
        },
        Err(e) => response::db_error(e.to_string()),
    }
}

async fn get_provider_detail(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> impl IntoResponse {
    let lang = state.settings.lang().await;
    match load_detail(&state.db, id).await {
        Ok(Some(detail)) => (StatusCode::OK, Json(Response::success(detail))),
        Ok(None) => {
            let msg = if lang == Lang::En {
                format!("provider {id} does not exist")
            } else {
                format!("Provider {id} 不存在")
            };
            response::not_found(msg)
        }
        Err(e) => response::db_error(e.to_string()),
    }
}

/// 按需返回某个 Provider 的明文 API Key（仅登录会话可访问）。
///
/// 详情接口不携带明文，前端在用户点击「显示/复制」时才请求本端点，
/// 且每次点击都重新请求，明文不进入前端缓存。
async fn get_provider_api_key(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> impl IntoResponse {
    let lang = state.settings.lang().await;
    match Entity::find_by_id(id).one(&state.db).await {
        Ok(Some(model)) => {
            let plain = crypto::decrypt(&model.api_key).unwrap_or_default();
            (
                StatusCode::OK,
                Json(Response::success(ProviderApiKeyResponse { api_key: plain })),
            )
        }
        Ok(None) => {
            let msg = if lang == Lang::En {
                format!("provider {id} does not exist")
            } else {
                format!("Provider {id} 不存在")
            };
            response::not_found(msg)
        }
        Err(e) => response::db_error(e.to_string()),
    }
}

#[derive(Deserialize)]
struct ProviderUsageQuery {
    /// `?refresh=1` 绕过 10 分钟数据库缓存，强制重新拉取上游。
    refresh: Option<String>,
}

/// 查询供应商用量（余额/订阅窗口额度）。
///
/// 优先直出数据库缓存（10 分钟内新鲜），过期/缺失才真实抓取并重新落库。
async fn get_provider_usage(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Query(query): Query<ProviderUsageQuery>,
) -> impl IntoResponse {
    let lang = state.settings.lang().await;
    let model = match Entity::find_by_id(id).one(&state.db).await {
        Ok(Some(m)) => m,
        Ok(None) => {
            let msg = if lang == Lang::En {
                format!("provider {id} does not exist")
            } else {
                format!("Provider {id} 不存在")
            };
            return response::not_found(msg);
        }
        Err(e) => return response::db_error(e.to_string()),
    };
    if !crate::usage::usage_enabled(&model.extra) {
        return response::bad_request(
            crate::usage::error::UsageError::NotEnabled.user_message(lang),
        );
    }

    let force_refresh = query
        .refresh
        .as_deref()
        .is_some_and(|v| v == "1" || v == "true");
    if !force_refresh
        && let Ok(Some(data)) = crate::usage::persist::read_usage_cache(&state.db, id).await
    {
        return (
            StatusCode::OK,
            Json(Response::success(
                data.with_normalized_remaining().with_localized_labels(lang),
            )),
        );
    }
    match crate::usage::persist::fetch_and_store(&state.db, id).await {
        Ok(data) => (
            StatusCode::OK,
            Json(Response::success(
                data.with_normalized_remaining().with_localized_labels(lang),
            )),
        ),
        Err(e) if e.is_client_error() => response::bad_request(e.user_message(lang)),
        Err(e) => response::bad_gateway(e.user_message(lang)),
    }
}

/// 订阅周期窗口长度（毫秒）：周 = 7 天，月 = 30 天。
const WEEK_MS: i64 = 7 * 24 * 3_600_000;
const MONTH_MS: i64 = 30 * 24 * 3_600_000;
/// 一天（毫秒）。
const DAY_MS: i64 = 24 * 3_600_000;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UsageEstimateResponse {
    provider_id: i32,
    /// 用于预估的窗口：weekly / monthly。
    window: String,
    /// 窗口起点（毫秒时间戳，由 resets_at 反推）。
    window_start: i64,
    /// 窗口终点（毫秒时间戳，即 resets_at）。
    window_end: i64,
    /// 窗口内实际有请求数据的日期数。
    covered_days: i64,
    /// 窗口总天数（周=7，月=30）。
    total_days: i64,
    /// 窗口内请求表统计的已用 token（成功行 total_tokens 合计）。
    used_tokens: i64,
    /// 用量卡该窗口已用配额（厂商单位，如 credits）。
    used: Option<f64>,
    /// 用量卡该窗口总配额（厂商单位）。
    limit: Option<f64>,
    /// 预估订阅周期内可用 token 总量（按已用配额比例折算）。
    estimated_total_tokens: Option<i64>,
    /// 是否可预估：请求数据覆盖完整且配额比例可折算时为 true。
    estimatable: bool,
}

/// 订阅制供应商的订阅周期 Token 总量预估。
///
/// 仅订阅制（billing_mode=1）且开启用量查询（extra.usage=true）的供应商可用。
/// 取用量卡 weekly/monthly 可用窗口（周优先），窗口起点由 resets_at 反推
/// （周 = resets_at - 7 天，月 = resets_at - 30 天），统计请求表在该窗口内
/// 该供应商的成功请求 token 总量。若窗口内请求数据天数覆盖不全（有日期
/// 缺口），或用量卡拿不到已用/总量/百分比，则无法准确预估（estimatable=false）。
async fn get_provider_usage_estimate(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> impl IntoResponse {
    use crate::usage::types::WindowKind;

    let lang = state.settings.lang().await;
    let model = match Entity::find_by_id(id).one(&state.db).await {
        Ok(Some(m)) => m,
        Ok(None) => {
            let msg = if lang == Lang::En {
                format!("provider {id} does not exist")
            } else {
                format!("Provider {id} 不存在")
            };
            return response::not_found(msg);
        }
        Err(e) => return response::db_error(e.to_string()),
    };
    // 仅订阅制可预估。
    if model.billing_mode != 1 {
        return response::bad_request(lang.tr(
            "仅订阅制供应商支持用量预估",
            "usage estimation is only supported for subscription providers",
        ));
    }
    if !crate::usage::usage_enabled(&model.extra) {
        return response::bad_request(
            crate::usage::error::UsageError::NotEnabled.user_message(lang),
        );
    }

    // 用量数据：数据库缓存新鲜直出，过期/缺失才真实抓取。
    let data = match crate::usage::persist::read_usage_cache(&state.db, id).await {
        Ok(Some(data)) => data,
        _ => match crate::usage::persist::fetch_and_store(&state.db, id).await {
            Ok(data) => data,
            Err(e) if e.is_client_error() => return response::bad_request(e.user_message(lang)),
            Err(e) => return response::bad_gateway(e.user_message(lang)),
        },
    };

    // 选取可用窗口：weekly 优先，其次 monthly。
    let window = data
        .windows
        .iter()
        .find(|w| w.available && w.window == WindowKind::Weekly)
        .or_else(|| {
            data.windows
                .iter()
                .find(|w| w.available && w.window == WindowKind::Monthly)
        });
    let Some(qw) = window else {
        return (
            StatusCode::OK,
            Json(Response::success(UsageEstimateResponse {
                provider_id: id,
                window: "none".to_string(),
                window_start: 0,
                window_end: 0,
                covered_days: 0,
                total_days: 0,
                used_tokens: 0,
                used: None,
                limit: None,
                estimated_total_tokens: None,
                estimatable: false,
            })),
        );
    };

    let window_name = match qw.window {
        WindowKind::Weekly => "weekly",
        WindowKind::Monthly => "monthly",
        _ => "other",
    };
    let window_len_ms = if qw.window == WindowKind::Weekly {
        WEEK_MS
    } else {
        MONTH_MS
    };

    // 窗口终点 = resets_at（取当前时刻兜底），起点 = 终点 - 窗口长度。
    let now_ms = chrono::Utc::now().timestamp_millis();
    let window_end = qw.resets_at.map(|t| t.timestamp_millis()).unwrap_or(now_ms);
    let window_start = window_end - window_len_ms;

    // 请求表统计：已过去时段内该供应商成功请求的 token 总量 + 覆盖天数（按天分桶）。
    // 统计上限取 min(窗口终点, now)：未来时段不应计入已用 token 与覆盖检查。
    let elapsed_end = window_end.min(now_ms);
    let sql = format!(
        "SELECT COALESCE(SUM(r.total_tokens), 0) AS used_tokens, \
                COUNT(DISTINCT r.start_time / {DAY_MS}) AS covered_days \
         FROM request r \
         WHERE r.provider_id = ? AND r.success = 1 \
           AND r.start_time >= ? AND r.start_time < ?"
    );
    let row = match state
        .db
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [id.into(), window_start.into(), elapsed_end.into()],
        ))
        .await
    {
        Ok(Some(row)) => row,
        Ok(None) => {
            let msg = lang.tr(
                "用量预估查询无结果",
                "usage estimation query returned no rows",
            );
            return response::db_error(msg);
        }
        Err(e) => return response::db_error(e.to_string()),
    };
    let used_tokens: i64 = row.try_get("", "used_tokens").unwrap_or(0);
    let covered_days: i64 = row.try_get("", "covered_days").unwrap_or(0);

    // 覆盖检查：只要求「已过去的时段」每天都有请求数据。
    // 窗口终点可能在未来（如本周还没结束），未来的天数不应计入应覆盖天数，
    // 否则会把「未来还没发生的请求」误判为数据缺口。
    // 应覆盖天数按「整数天分桶」对齐 covered_days 的统计口径：
    // 从 window_start 所在桶到 elapsed_end 所在桶的桶数（含两端）。
    let elapsed_days = elapsed_end / DAY_MS - window_start / DAY_MS + 1;
    let total_days = elapsed_days.max(1);
    let covered = covered_days >= total_days;

    // 折算基准：优先 used/limit 绝对值，其次 used_percent。
    let ratio: Option<f64> = match (qw.used, qw.limit) {
        (Some(used), Some(limit)) if limit > 0.0 => Some(used / limit),
        _ => qw.used_percent.map(|p| p / 100.0),
    };
    let ratio = ratio.filter(|r| *r > 0.0);

    let estimatable = covered && ratio.is_some();
    let estimated_total_tokens = ratio.map(|r| (used_tokens as f64 / r).round() as i64);

    (
        StatusCode::OK,
        Json(Response::success(UsageEstimateResponse {
            provider_id: id,
            window: window_name.to_string(),
            window_start,
            window_end,
            covered_days,
            total_days,
            used_tokens,
            used: qw.used,
            limit: qw.limit,
            estimated_total_tokens: if estimatable {
                estimated_total_tokens
            } else {
                None
            },
            estimatable,
        })),
    )
}

/// SQLite 唯一约束冲突（name UNIQUE）。
fn is_unique_violation(err: &DbErr) -> bool {
    err.to_string().contains("UNIQUE constraint failed")
}

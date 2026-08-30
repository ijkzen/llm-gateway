use std::collections::{HashMap, HashSet};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post, put},
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseTransaction, DbErr, EntityTrait,
    QueryFilter, QueryOrder, Set, TransactionTrait,
};
use serde::{Deserialize, Serialize};

use crate::entity::provider;
use crate::entity::provider_model;
use crate::entity::virtual_model::{self, ActiveModel, Entity};
use crate::entity::virtual_model_item;
use crate::response::{self, Response};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_virtual_models))
        .route("/", post(create_virtual_model))
        .route("/{id}", get(get_virtual_model))
        .route("/{id}", put(update_virtual_model))
        .route("/{id}", delete(delete_virtual_model))
}

/// 成员条目响应：附带供应商与供应商模型的展示信息。
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct VirtualModelItemResponse {
    virtual_model_item_id: i32,
    model_id: i32,
    enable: bool,
    provider_id: i32,
    provider_name: String,
    /// 供应商启用状态；false 时该成员实际不可用。
    provider_enable: bool,
    /// 供应商付费模式：0=按量付费，1=订阅制。
    billing_mode: i32,
    /// 远端模型 ID 字符串，如 `gpt-4o`。
    provider_model_id: String,
    context_length: i64,
    max_output_tokens: i64,
    reasoning: bool,
    tool_use: bool,
    image_understand: bool,
    video_understand: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VirtualModelResponse {
    virtual_model_id: i32,
    display_id: String,
    enable: bool,
    load_balancing_strategy: i32,
    fallback_strategy: i32,
    items: Vec<VirtualModelItemResponse>,
    created_at: String,
    updated_at: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VirtualModelItemRequest {
    model_id: i32,
    /// 缺省视为启用。
    #[serde(default = "default_true")]
    enable: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateVirtualModelRequest {
    display_id: String,
    /// 缺省视为启用。
    #[serde(default = "default_true")]
    enable: bool,
    load_balancing_strategy: i32,
    fallback_strategy: i32,
    items: Vec<VirtualModelItemRequest>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateVirtualModelRequest {
    display_id: Option<String>,
    enable: Option<bool>,
    load_balancing_strategy: Option<i32>,
    fallback_strategy: Option<i32>,
    /// 传入时以该集合为最终成员（diff 更新）；缺省表示不修改成员。
    items: Option<Vec<VirtualModelItemRequest>>,
}

/// 校验负载均衡与降级策略取值，返回第一个错误消息（None 表示通过）。
fn validate_strategies(load_balancing_strategy: i32, fallback_strategy: i32) -> Option<String> {
    if !(0..=3).contains(&load_balancing_strategy) {
        return Some("负载均衡策略不合法".to_string());
    }
    if !(0..=1).contains(&fallback_strategy) {
        return Some("降级策略不合法".to_string());
    }
    None
}

/// 按 model_id 去重（保留首次出现的 enable 设置）。
fn dedupe_items(items: &[VirtualModelItemRequest]) -> Vec<VirtualModelItemRequest> {
    let mut seen = HashSet::new();
    items
        .iter()
        .filter(|item| seen.insert(item.model_id))
        .cloned()
        .collect()
}

/// 校验成员 model_id：必须存在于 provider_model，且未被其他虚拟模型占用
/// （exclude_vm_id 用于编辑场景排除自身）。Some(msg) 表示校验失败。
async fn validate_item_model_ids<C: ConnectionTrait>(
    db: &C,
    model_ids: &[i32],
    exclude_vm_id: Option<i32>,
) -> Result<Option<String>, DbErr> {
    let found = provider_model::Entity::find()
        .filter(provider_model::Column::ModelId.is_in(model_ids.to_vec()))
        .all(db)
        .await?;
    if found.len() < model_ids.len() {
        let found_ids: HashSet<i32> = found.iter().map(|pm| pm.model_id).collect();
        if let Some(missing) = model_ids.iter().find(|id| !found_ids.contains(id)) {
            return Ok(Some(format!("模型 {missing} 不存在")));
        }
    }

    let mut query = virtual_model_item::Entity::find()
        .filter(virtual_model_item::Column::ModelId.is_in(model_ids.to_vec()));
    if let Some(vm_id) = exclude_vm_id {
        query = query.filter(virtual_model_item::Column::VirtualModelId.ne(vm_id));
    }
    let conflicts = query.all(db).await?;
    if let Some(conflict) = conflicts.first() {
        return Ok(Some(format!(
            "模型 {} 已被其他虚拟模型使用",
            conflict.model_id
        )));
    }
    Ok(None)
}

/// 加载成员明细并附带供应商与供应商模型展示信息，按虚拟模型 id 分组；
/// `virtual_model_id` 为 Some 时只加载该虚拟模型的成员。
///
/// 注意：组内顺序由 `virtual_model_response` 的 `sort_items` 统一排序
/// （启用优先 → LB 策略分组 → 字母序），此处保持数据库返回序。
async fn load_item_responses<C: ConnectionTrait>(
    db: &C,
    virtual_model_id: Option<i32>,
) -> Result<HashMap<i32, Vec<VirtualModelItemResponse>>, DbErr> {
    let mut query = virtual_model_item::Entity::find()
        .order_by_asc(virtual_model_item::Column::VirtualModelItemId);
    if let Some(vm_id) = virtual_model_id {
        query = query.filter(virtual_model_item::Column::VirtualModelId.eq(vm_id));
    }
    let items = query.all(db).await?;
    if items.is_empty() {
        return Ok(HashMap::new());
    }

    let model_ids: Vec<i32> = items.iter().map(|item| item.model_id).collect();
    let provider_models = provider_model::Entity::find()
        .filter(provider_model::Column::ModelId.is_in(model_ids))
        .all(db)
        .await?;
    if provider_models.is_empty() {
        return Ok(HashMap::new());
    }
    let provider_ids: Vec<i32> = provider_models
        .iter()
        .map(|pm| pm.provider_id)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let providers = provider::Entity::find()
        .filter(provider::Column::Id.is_in(provider_ids))
        .all(db)
        .await?;

    let model_map: HashMap<i32, &provider_model::Model> =
        provider_models.iter().map(|pm| (pm.model_id, pm)).collect();
    let provider_map: HashMap<i32, &provider::Model> =
        providers.iter().map(|p| (p.id, p)).collect();

    let mut grouped: HashMap<i32, Vec<VirtualModelItemResponse>> = HashMap::new();
    for item in items {
        // 正常不应出现悬空条目（供应商删除时级联清理成员）；防御性跳过。
        let Some(pm) = model_map.get(&item.model_id) else {
            continue;
        };
        let Some(p) = provider_map.get(&pm.provider_id) else {
            continue;
        };
        grouped
            .entry(item.virtual_model_id)
            .or_default()
            .push(VirtualModelItemResponse {
                virtual_model_item_id: item.virtual_model_item_id,
                model_id: item.model_id,
                enable: item.enable,
                provider_id: p.id,
                provider_name: p.name.clone(),
                provider_enable: p.enable,
                billing_mode: p.billing_mode,
                provider_model_id: pm.provider_model_id.clone(),
                context_length: pm.context_length,
                max_output_tokens: pm.max_output_tokens,
                reasoning: pm.reasoning,
                tool_use: pm.tool_use,
                image_understand: pm.image_understand,
                video_understand: pm.video_understand,
            });
    }
    Ok(grouped)
}

/// 成员排序：启用优先 → 按虚拟模型负载均衡策略分组 → 组内远端模型 ID 字母升序。
///
/// 策略 0（订阅制优先）：订阅制成员在前、按量付费在后；策略 1（按量付费优先）
/// 反之；策略 2/3（轮转/随机）不按付费模式分组。启用/停用两个大组内部都
/// 先做策略分组，再按 provider_model_id 字母升序。
fn sort_items(items: &mut [VirtualModelItemResponse], load_balancing_strategy: i32) {
    let subscription_first = load_balancing_strategy == 0;
    let payg_first = load_balancing_strategy == 1;
    items.sort_by(|a, b| {
        // 第一层：启用在前。
        b.enable
            .cmp(&a.enable)
            // 第二层：LB 策略分组（仅策略 0/1 生效）。
            .then_with(|| {
                if !(subscription_first || payg_first) {
                    return std::cmp::Ordering::Equal;
                }
                let a_sub = a.billing_mode == 1;
                let b_sub = b.billing_mode == 1;
                if subscription_first {
                    b_sub.cmp(&a_sub)
                } else {
                    a_sub.cmp(&b_sub)
                }
            })
            // 第三层：字母升序。
            .then_with(|| a.provider_model_id.cmp(&b.provider_model_id))
    });
}

fn virtual_model_response(
    model: virtual_model::Model,
    mut items: Vec<VirtualModelItemResponse>,
) -> VirtualModelResponse {
    sort_items(&mut items, model.load_balancing_strategy);
    VirtualModelResponse {
        virtual_model_id: model.virtual_model_id,
        display_id: model.display_id,
        enable: model.enable,
        load_balancing_strategy: model.load_balancing_strategy,
        fallback_strategy: model.fallback_strategy,
        items,
        created_at: model.created_at.to_rfc3339(),
        updated_at: model.updated_at.to_rfc3339(),
    }
}

async fn load_virtual_model_response(
    db: &impl ConnectionTrait,
    model: virtual_model::Model,
) -> Result<VirtualModelResponse, DbErr> {
    let mut grouped = load_item_responses(db, Some(model.virtual_model_id)).await?;
    let items = grouped.remove(&model.virtual_model_id).unwrap_or_default();
    Ok(virtual_model_response(model, items))
}

async fn load_virtual_model_list(
    db: &impl ConnectionTrait,
) -> Result<Vec<VirtualModelResponse>, DbErr> {
    let models = Entity::find()
        .order_by_asc(virtual_model::Column::VirtualModelId)
        .all(db)
        .await?;
    if models.is_empty() {
        return Ok(Vec::new());
    }
    let grouped = load_item_responses(db, None).await?;
    Ok(models
        .into_iter()
        .map(|model| {
            let items = grouped
                .get(&model.virtual_model_id)
                .cloned()
                .unwrap_or_default();
            virtual_model_response(model, items)
        })
        .collect())
}

async fn insert_items(
    txn: &DatabaseTransaction,
    virtual_model_id: i32,
    items: &[VirtualModelItemRequest],
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), DbErr> {
    for item in items {
        let active = virtual_model_item::ActiveModel {
            virtual_model_id: Set(virtual_model_id),
            model_id: Set(item.model_id),
            enable: Set(item.enable),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        active.insert(txn).await?;
    }
    Ok(())
}

async fn list_virtual_models(State(state): State<AppState>) -> impl IntoResponse {
    match load_virtual_model_list(&state.db).await {
        Ok(list) => (StatusCode::OK, Json(Response::success(list))),
        Err(e) => response::db_error(e.to_string()),
    }
}

async fn get_virtual_model(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> impl IntoResponse {
    match Entity::find_by_id(id).one(&state.db).await {
        Ok(Some(model)) => match load_virtual_model_response(&state.db, model).await {
            Ok(resp) => (StatusCode::OK, Json(Response::success(resp))),
            Err(e) => response::db_error(e.to_string()),
        },
        Ok(None) => response::not_found(format!("虚拟模型 {id} 不存在")),
        Err(e) => response::db_error(e.to_string()),
    }
}

async fn create_virtual_model(
    State(state): State<AppState>,
    Json(req): Json<CreateVirtualModelRequest>,
) -> impl IntoResponse {
    let display_id = req.display_id.trim();
    if display_id.is_empty() {
        return response::bad_request("模型 ID 不能为空");
    }
    if let Some(msg) = validate_strategies(req.load_balancing_strategy, req.fallback_strategy) {
        return response::bad_request(msg);
    }
    if req.items.is_empty() {
        return response::bad_request("至少选择一个成员模型");
    }
    let items = dedupe_items(&req.items);
    let model_ids: Vec<i32> = items.iter().map(|item| item.model_id).collect();
    match validate_item_model_ids(&state.db, &model_ids, None).await {
        Ok(Some(msg)) => return response::bad_request(msg),
        Ok(None) => {}
        Err(e) => return response::db_error(e.to_string()),
    }

    let txn = match state.db.begin().await {
        Ok(txn) => txn,
        Err(e) => return response::db_error(e.to_string()),
    };
    let now = chrono::Utc::now();
    let active = ActiveModel {
        display_id: Set(display_id.to_string()),
        enable: Set(req.enable),
        load_balancing_strategy: Set(req.load_balancing_strategy),
        fallback_strategy: Set(req.fallback_strategy),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };
    let model = match active.insert(&txn).await {
        Ok(model) => model,
        Err(e) if is_unique_violation(&e) => {
            return response::bad_request(unique_conflict_message(&e));
        }
        Err(e) => return response::db_error(e.to_string()),
    };
    if let Err(e) = insert_items(&txn, model.virtual_model_id, &items, now).await {
        if is_unique_violation(&e) {
            return response::bad_request(unique_conflict_message(&e));
        }
        return response::db_error(e.to_string());
    }
    if let Err(e) = txn.commit().await {
        return response::db_error(e.to_string());
    }

    match load_virtual_model_response(&state.db, model).await {
        Ok(resp) => (StatusCode::CREATED, Json(Response::success(resp))),
        Err(e) => response::db_error(e.to_string()),
    }
}

async fn update_virtual_model(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Json(req): Json<UpdateVirtualModelRequest>,
) -> impl IntoResponse {
    let existing = match Entity::find_by_id(id).one(&state.db).await {
        Ok(Some(model)) => model,
        Ok(None) => return response::not_found(format!("虚拟模型 {id} 不存在")),
        Err(e) => return response::db_error(e.to_string()),
    };

    let display_id = req
        .display_id
        .unwrap_or_else(|| existing.display_id.clone());
    let display_id = display_id.trim();
    if display_id.is_empty() {
        return response::bad_request("模型 ID 不能为空");
    }
    let load_balancing_strategy = req
        .load_balancing_strategy
        .unwrap_or(existing.load_balancing_strategy);
    let fallback_strategy = req.fallback_strategy.unwrap_or(existing.fallback_strategy);
    if let Some(msg) = validate_strategies(load_balancing_strategy, fallback_strategy) {
        return response::bad_request(msg);
    }

    let txn = match state.db.begin().await {
        Ok(txn) => txn,
        Err(e) => return response::db_error(e.to_string()),
    };

    // 成员 diff 更新：传入 items 时以其为最终成员集合。
    if let Some(req_items) = &req.items {
        if req_items.is_empty() {
            return response::bad_request("至少选择一个成员模型");
        }
        let items = dedupe_items(req_items);
        let model_ids: Vec<i32> = items.iter().map(|item| item.model_id).collect();
        match validate_item_model_ids(&txn, &model_ids, Some(id)).await {
            Ok(Some(msg)) => return response::bad_request(msg),
            Ok(None) => {}
            Err(e) => return response::db_error(e.to_string()),
        }

        let current = match virtual_model_item::Entity::find()
            .filter(virtual_model_item::Column::VirtualModelId.eq(id))
            .all(&txn)
            .await
        {
            Ok(current) => current,
            Err(e) => return response::db_error(e.to_string()),
        };
        let current_map: HashMap<i32, virtual_model_item::Model> = current
            .into_iter()
            .map(|item| (item.model_id, item))
            .collect();
        let keep_ids: HashSet<i32> = model_ids.iter().copied().collect();
        let removed: Vec<i32> = current_map
            .keys()
            .copied()
            .filter(|model_id| !keep_ids.contains(model_id))
            .collect();
        if !removed.is_empty()
            && let Err(e) = virtual_model_item::Entity::delete_many()
                .filter(virtual_model_item::Column::VirtualModelId.eq(id))
                .filter(virtual_model_item::Column::ModelId.is_in(removed))
                .exec(&txn)
                .await
        {
            return response::db_error(e.to_string());
        }

        let now = chrono::Utc::now();
        for item in &items {
            match current_map.get(&item.model_id) {
                // 保留的成员：enable 有变化才更新。
                Some(existing_item) if existing_item.enable != item.enable => {
                    let mut active: virtual_model_item::ActiveModel = existing_item.clone().into();
                    active.enable = Set(item.enable);
                    active.updated_at = Set(now);
                    if let Err(e) = active.update(&txn).await {
                        return response::db_error(e.to_string());
                    }
                }
                Some(_) => {}
                None => {
                    if let Err(e) = insert_items(&txn, id, std::slice::from_ref(item), now).await {
                        if is_unique_violation(&e) {
                            return response::bad_request(unique_conflict_message(&e));
                        }
                        return response::db_error(e.to_string());
                    }
                }
            }
        }
    }

    let enable = req.enable.unwrap_or(existing.enable);
    let mut active: ActiveModel = existing.into();
    active.display_id = Set(display_id.to_string());
    active.enable = Set(enable);
    active.load_balancing_strategy = Set(load_balancing_strategy);
    active.fallback_strategy = Set(fallback_strategy);
    active.updated_at = Set(chrono::Utc::now());
    match active.update(&txn).await {
        Ok(model) => {
            if let Err(e) = txn.commit().await {
                return response::db_error(e.to_string());
            }
            match load_virtual_model_response(&state.db, model).await {
                Ok(resp) => (StatusCode::OK, Json(Response::success(resp))),
                Err(e) => response::db_error(e.to_string()),
            }
        }
        Err(e) if is_unique_violation(&e) => response::bad_request(unique_conflict_message(&e)),
        Err(e) => response::db_error(e.to_string()),
    }
}

async fn delete_virtual_model(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> impl IntoResponse {
    // 级联硬删：同一事务内先删全部成员条目（释放成员模型），再删虚拟模型本身。
    let txn = match state.db.begin().await {
        Ok(txn) => txn,
        Err(e) => return response::db_error(e.to_string()),
    };
    if let Err(e) = virtual_model_item::Entity::delete_many()
        .filter(virtual_model_item::Column::VirtualModelId.eq(id))
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
        Ok(_) => response::not_found(format!("虚拟模型 {id} 不存在")),
        Err(e) => response::db_error(e.to_string()),
    }
}

/// SQLite 唯一约束冲突（display_id 列约束或成员 model_id 唯一索引）。
fn is_unique_violation(err: &DbErr) -> bool {
    err.to_string().contains("UNIQUE constraint failed")
}

/// 区分 display_id 与成员 model_id 的唯一冲突，返回对应的中文提示。
fn unique_conflict_message(err: &DbErr) -> String {
    let text = err.to_string();
    if text.contains("virtual_model_item.model_id")
        || text.contains("uq_virtual_model_items_model_id")
    {
        "部分模型已被其他虚拟模型使用".to_string()
    } else {
        "虚拟模型 ID 已存在".to_string()
    }
}

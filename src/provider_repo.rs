//! Provider 域统一写操作（DAO 层）。
//!
//! 把「供应商及名下虚拟模型子模型」的状态变更收编到这里，接口路由与用量额度门控
//! （`src/usage/persist.rs` 定时任务）共用同一入口，保证任何路径的变更都有日志。
//! 日志统一为结构化 tracing，api_key 一律经 `crypto::mask` 脱敏，绝不落明文。

use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, DbErr, EntityTrait,
    QueryFilter, Set,
};

use crate::crypto;
use crate::entity::provider::{self, ActiveModel, Entity};
use crate::entity::provider_model;
use crate::entity::virtual_model_item;

/// 对存储态 api_key 做脱敏：先解密再 mask，失败返回空串（与路由层 `mask_api_key` 一致）。
fn mask_stored_key(stored: &str) -> String {
    match crypto::decrypt(stored) {
        Ok(plain) => crypto::mask(&plain),
        Err(_) => String::new(),
    }
}

/// 插入一条供应商记录，成功落库后输出全字段结构化日志（api_key 脱敏）。
pub async fn insert_provider(
    db: &impl ConnectionTrait,
    active: ActiveModel,
) -> Result<provider::Model, DbErr> {
    let model = active.insert(db).await?;
    tracing::info!(
        provider_id = model.id,
        name = %model.name,
        enable = model.enable,
        base_url = %model.base_url,
        api_key_masked = %mask_stored_key(&model.api_key),
        custom_header = %model.custom_header,
        extra = %model.extra,
        status = model.status,
        protocol_type = model.protocol_type,
        billing_mode = model.billing_mode,
        sort_order = model.sort_order,
        "创建供应商",
    );
    Ok(model)
}

/// 更新一条供应商记录，成功落库后输出全字段结构化日志（api_key 脱敏）。
pub async fn update_provider(
    db: &impl ConnectionTrait,
    active: ActiveModel,
) -> Result<provider::Model, DbErr> {
    let model = active.update(db).await?;
    tracing::info!(
        provider_id = model.id,
        name = %model.name,
        enable = model.enable,
        base_url = %model.base_url,
        api_key_masked = %mask_stored_key(&model.api_key),
        custom_header = %model.custom_header,
        extra = %model.extra,
        status = model.status,
        protocol_type = model.protocol_type,
        billing_mode = model.billing_mode,
        sort_order = model.sort_order,
        "更新供应商",
    );
    Ok(model)
}

/// 删除一条供应商记录，成功后输出日志。级联删除（provider_model / virtual_model_item）
/// 由调用方在事务内完成，本方法只删 provider 行。
pub async fn delete_provider(
    db: &impl ConnectionTrait,
    provider: provider::Model,
) -> Result<(), DbErr> {
    Entity::delete_by_id(provider.id).exec(db).await?;
    tracing::info!(
        provider_id = provider.id,
        name = %provider.name,
        base_url = %provider.base_url,
        "删除供应商",
    );
    Ok(())
}

/// 开关单个供应商的启用状态（接口更新与额度门控共用入口）。
/// 幂等：enable 未变化时直接返回 false 且不打日志。
pub async fn set_provider_enabled(
    db: &DatabaseConnection,
    provider_id: i32,
    enabled: bool,
) -> Result<bool, DbErr> {
    let Some(row) = provider::Entity::find_by_id(provider_id).one(db).await? else {
        return Ok(false);
    };
    if row.enable == enabled {
        return Ok(false);
    }
    let (id, name, enable_old) = (row.id, row.name.clone(), row.enable);
    let mut active: provider::ActiveModel = row.into();
    active.enable = Set(enabled);
    active.updated_at = Set(chrono::Utc::now());
    active.update(db).await?;
    tracing::info!(
        provider_id = id,
        name = %name,
        enable_old,
        enable_new = enabled,
        "Provider 启用状态变更",
    );
    Ok(true)
}

/// 级联开关该供应商名下全部虚拟模型子模型，返回实际变更的条目数。
/// 幂等：已处于目标状态的条目跳过。逐行更新，变更后输出日志。
pub async fn set_items_enabled(
    db: &DatabaseConnection,
    provider_id: i32,
    enabled: bool,
) -> Result<usize, DbErr> {
    let model_ids: Vec<i32> = provider_model::Entity::find()
        .filter(provider_model::Column::ProviderId.eq(provider_id))
        .all(db)
        .await?
        .into_iter()
        .map(|m| m.model_id)
        .collect();
    if model_ids.is_empty() {
        return Ok(0);
    }
    let items = virtual_model_item::Entity::find()
        .filter(virtual_model_item::Column::ModelId.is_in(model_ids))
        .all(db)
        .await?;
    let now = chrono::Utc::now();
    let mut count = 0;
    for item in items {
        if item.enable == enabled {
            continue;
        }
        let mut active: virtual_model_item::ActiveModel = item.into();
        active.enable = Set(enabled);
        active.updated_at = Set(now);
        active.update(db).await?;
        count += 1;
    }
    if count > 0 {
        tracing::info!(
            provider_id,
            changed_count = count,
            enable_new = enabled,
            "级联更新虚拟模型子模型启用状态",
        );
    }
    Ok(count)
}

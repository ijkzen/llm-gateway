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

/// 启动时一次性迁移：把 provider 表中未加密的明文 extra 加密写回。
///
/// 幂等：已带 `enc:v1:` 前缀的行跳过。未配置密钥时跳过并记录日志
/// （与 api_key 的明文降级行为一致），配置密钥后下次启动自动完成。
/// 单行迁移失败仅记录 warn 并继续，不阻塞其余行与启动。
pub async fn backfill_extra_encryption(db: &DatabaseConnection) -> Result<usize, DbErr> {
    if !crypto::encryption_enabled() {
        tracing::info!(
            "{} 未配置，provider extra 保持明文存储，跳过加密迁移",
            crypto::ENCRYPTION_KEY_ENV
        );
        return Ok(0);
    }
    let rows = provider::Entity::find().all(db).await?;
    let mut migrated = 0usize;
    for row in rows {
        if crypto::is_encrypted(&row.extra) || row.extra.is_empty() {
            continue;
        }
        let encrypted = crypto::encrypt(&row.extra);
        let mut active: provider::ActiveModel = row.clone().into();
        active.extra = Set(encrypted);
        match active.update(db).await {
            Ok(_) => migrated += 1,
            Err(e) => {
                tracing::warn!(provider_id = row.id, "迁移 provider extra 加密失败：{e}");
            }
        }
    }
    if migrated > 0 {
        tracing::info!(migrated, "Provider extra 加密迁移完成");
    }
    Ok(migrated)
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

/// 连续失败达到阈值时的熔断停用：供应商停用 + 名下虚拟模型条目级联停用 +
/// 打 `failure_disabled` 标记。与额度门控禁用区分：用量定时刷新不会自动恢复，
/// 仅管理员手动启用（清标记）解除。
/// 原子性：条件更新 `failure_disabled=0 → 1`，并发仅一个胜出，返回 true。
pub async fn disable_provider_on_failures(
    db: &DatabaseConnection,
    provider_id: i32,
    consecutive: u32,
    request_id: &str,
) -> Result<bool, DbErr> {
    use sea_orm::sea_query::Expr;
    let now = chrono::Utc::now();
    let affected = provider::Entity::update_many()
        .col_expr(provider::Column::Enable, Expr::value(false))
        .col_expr(provider::Column::FailureDisabled, Expr::value(true))
        .col_expr(provider::Column::UpdatedAt, Expr::value(now))
        .filter(provider::Column::Id.eq(provider_id))
        .filter(provider::Column::FailureDisabled.eq(false))
        .exec(db)
        .await?;
    if affected.rows_affected == 0 {
        return Ok(false);
    }
    let items = set_items_enabled(db, provider_id, false).await?;
    tracing::warn!(
        request_id,
        provider_id,
        consecutive,
        items,
        "连续失败达到阈值，熔断停用供应商及其全部虚拟模型子模型（仅手动启用可恢复）"
    );
    Ok(true)
}

/// 级联开关该供应商名下全部虚拟模型子模型，返回实际变更的条目数。
/// 幂等：已处于目标状态的条目跳过。逐行更新，变更后输出日志。
///
/// 分层语义：级联停用（enabled=false）只动当前启用条目，并打上 `cascade_disabled`
/// 标记；级联恢复（enabled=true）只恢复带该标记的条目并清除标记，用户手动关闭的
/// 成员（无标记）保持不变。
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
    let mut query = virtual_model_item::Entity::find()
        .filter(virtual_model_item::Column::ModelId.is_in(model_ids));
    // 恢复时只取被级联停用的条目，避免覆盖用户手动关闭的成员。
    if enabled {
        query = query.filter(virtual_model_item::Column::CascadeDisabled.eq(true));
    }
    let items = query.all(db).await?;
    let now = chrono::Utc::now();
    let mut count = 0;
    for item in items {
        let (new_enable, new_flag) = if enabled {
            (true, false)
        } else {
            (false, true)
        };
        // 已处于目标状态则跳过（幂等）。
        if item.enable == new_enable && item.cascade_disabled == new_flag {
            continue;
        }
        // 停用时只操作当前启用条目（已禁用的条目可能是手动关闭的，不碰标记）。
        if !enabled && !item.enable {
            continue;
        }
        let mut active: virtual_model_item::ActiveModel = item.into();
        active.enable = Set(new_enable);
        active.cascade_disabled = Set(new_flag);
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

#[cfg(test)]
mod tests {
    use super::*;

    async fn insert_provider(db: &DatabaseConnection, name: &str, extra: &str) -> i32 {
        let now = chrono::Utc::now();
        let row = provider::ActiveModel {
            name: Set(name.to_string()),
            enable: Set(true),
            base_url: Set(format!("https://{name}.example.com/v1")),
            api_key: Set(crate::crypto::encrypt("sk-x")),
            custom_header: Set("{}".to_string()),
            status: Set(0),
            protocol_type: Set(0),
            billing_mode: Set(0),
            extra: Set(extra.to_string()),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        row.id
    }

    async fn extra_of(db: &DatabaseConnection, id: i32) -> String {
        provider::Entity::find_by_id(id)
            .one(db)
            .await
            .unwrap()
            .unwrap()
            .extra
    }

    #[tokio::test]
    async fn migration_encrypts_plaintext_extra() {
        temp_env::async_with_vars(
            [(crate::crypto::ENCRYPTION_KEY_ENV, Some("test-key"))],
            async {
                let db = crate::db::connect("sqlite::memory:").await.unwrap();
                let plain = r#"{"ak":"sk-secret","usage":true}"#;
                let id = insert_provider(&db, "plain", plain).await;

                let n = backfill_extra_encryption(&db).await.unwrap();
                assert_eq!(n, 1);

                let stored = extra_of(&db, id).await;
                assert!(crate::crypto::is_encrypted(&stored), "迁移后应为密文");
                assert_eq!(crate::crypto::decrypt(&stored).unwrap(), plain);
            },
        )
        .await;
    }

    #[tokio::test]
    async fn migration_skips_encrypted_rows_idempotent() {
        temp_env::async_with_vars(
            [(crate::crypto::ENCRYPTION_KEY_ENV, Some("test-key"))],
            async {
                let db = crate::db::connect("sqlite::memory:").await.unwrap();
                let encrypted = crate::crypto::encrypt(r#"{"ak":"sk-enc","usage":true}"#);
                let id = insert_provider(&db, "already", &encrypted).await;

                // 已加密行不重复迁移（幂等）。
                let n = backfill_extra_encryption(&db).await.unwrap();
                assert_eq!(n, 0);
                assert_eq!(extra_of(&db, id).await, encrypted);
            },
        )
        .await;
    }

    #[tokio::test]
    async fn migration_skips_when_key_missing() {
        temp_env::async_with_vars([(crate::crypto::ENCRYPTION_KEY_ENV, None::<&str>)], async {
            let db = crate::db::connect("sqlite::memory:").await.unwrap();
            let plain = r#"{"ak":"sk-plain","usage":true}"#;
            let id = insert_provider(&db, "nokey", plain).await;

            let n = backfill_extra_encryption(&db).await.unwrap();
            assert_eq!(n, 0);
            assert_eq!(extra_of(&db, id).await, plain);
        })
        .await;
    }
}

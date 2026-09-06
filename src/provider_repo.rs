//! Provider 域统一写操作（DAO 层）。
//!
//! 把「供应商及名下虚拟模型子模型」的状态变更收编到这里，接口路由与用量额度门控
//! （`src/usage/persist.rs` 定时任务）共用同一入口，保证任何路径的变更都有日志。
//! 日志统一为结构化 tracing，api_key 一律经 `crypto::mask` 脱敏，绝不落明文。

use sea_orm::{ActiveModelTrait, ConnectionTrait, DatabaseConnection, DbErr, EntityTrait, Set};

use crate::crypto;
use crate::entity::provider::{self, ActiveModel, Entity};

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
    if std::env::var(crypto::ENCRYPTION_KEY_ENV).map_or(true, |k| k.trim().is_empty()) {
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

use sea_orm::{ColumnTrait, DbErr, EntityTrait, QueryFilter, QueryOrder};

use crate::crypto;
use crate::entity::{provider, provider_model};
use crate::state::AppState;

/// 执行一轮连续失败禁用供应商的自动恢复探测，返回成功恢复数量。
pub async fn recover_failure_disabled(state: &AppState) -> Result<usize, DbErr> {
    let providers = provider::Entity::find()
        .filter(provider::Column::FailureDisabled.eq(true))
        .all(&state.db)
        .await?;
    let mut recovered = 0;

    for provider in providers {
        if !usage_allows_probe(&state.db, &provider).await {
            continue;
        }
        let provider = match provider::Entity::find_by_id(provider.id)
            .one(&state.db)
            .await
        {
            Ok(Some(provider)) if provider.failure_disabled => provider,
            Ok(_) => continue,
            Err(error) => {
                tracing::warn!(
                    provider_id = provider.id,
                    "自动恢复重新读取供应商失败：{error}"
                );
                continue;
            }
        };
        let model = match provider_model::Entity::find()
            .filter(provider_model::Column::ProviderId.eq(provider.id))
            .order_by_asc(provider_model::Column::ModelId)
            .one(&state.db)
            .await
        {
            Ok(Some(model)) => model,
            Ok(None) => {
                tracing::warn!(provider_id = provider.id, "自动恢复跳过：供应商没有模型");
                continue;
            }
            Err(error) => {
                tracing::warn!(
                    provider_id = provider.id,
                    "自动恢复查询供应商模型失败：{error}"
                );
                continue;
            }
        };
        let api_key = match crypto::decrypt(&provider.api_key) {
            Ok(api_key) if !api_key.is_empty() => api_key,
            Ok(_) => {
                tracing::warn!(
                    provider_id = provider.id,
                    "自动恢复跳过：供应商未配置 API Key"
                );
                continue;
            }
            Err(error) => {
                tracing::warn!(
                    provider_id = provider.id,
                    "自动恢复跳过：API Key 解密失败：{error}"
                );
                continue;
            }
        };

        if let Err(error) = super::test_model(state, &provider, &model, &api_key).await {
            tracing::warn!(provider_id = provider.id, "自动恢复探测失败：{error}");
            continue;
        }
        match crate::provider_repo::recover_provider_from_failures(
            &state.db,
            provider.id,
            provider.updated_at,
        )
        .await
        {
            Ok(true) => {
                state.failure_counter.reset(provider.id);
                recovered += 1;
            }
            Ok(false) => {}
            Err(error) => {
                tracing::warn!(provider_id = provider.id, "自动恢复状态更新失败：{error}")
            }
        }
    }

    Ok(recovered)
}

async fn usage_allows_probe(db: &sea_orm::DatabaseConnection, provider: &provider::Model) -> bool {
    if !crate::usage::usage_enabled(&provider.extra) {
        return true;
    }
    let data = match crate::usage::persist::fetch_and_store(db, provider.id).await {
        Ok(data) => data,
        Err(error) => {
            tracing::warn!(provider_id = provider.id, "自动恢复用量查询失败：{error}");
            return false;
        }
    };
    data.usable_for_billing_mode(provider.billing_mode) == Some(true)
}

pub mod seed;

#[cfg(test)]
mod tests;

use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, Set,
};

use crate::crypto;
use crate::entity::provider_template::{self, ActiveModel, Entity};

/// 将种子模板数据批量 upsert 到 provider_template 表。
///
/// 每次应用启动时调用：已存在的记录按 name 更新（base_url/protocol_type/
/// billing_mode/extra），不存在的插入。不会删除用户手动添加或修改的记录。
pub async fn upsert_templates(db: &DatabaseConnection) -> Result<usize, DbErr> {
    let now = Utc::now();
    let mut inserted = 0usize;
    let mut updated = 0usize;

    for tmpl in seed::TEMPLATES {
        let existing = Entity::find()
            .filter(provider_template::Column::Name.eq(tmpl.name))
            .one(db)
            .await?;

        if let Some(row) = existing {
            let mut am: ActiveModel = row.into();
            am.base_url = Set(tmpl.base_url.to_string());
            am.protocol_type = Set(tmpl.protocol_type);
            am.billing_mode = Set(tmpl.billing_mode);
            am.extra = Set(tmpl.extra.to_string());
            am.updated_at = Set(now);
            am.update(db).await?;
            updated += 1;
        } else {
            ActiveModel {
                name: Set(tmpl.name.to_string()),
                base_url: Set(tmpl.base_url.to_string()),
                protocol_type: Set(tmpl.protocol_type),
                billing_mode: Set(tmpl.billing_mode),
                extra: Set(tmpl.extra.to_string()),
                created_at: Set(now),
                updated_at: Set(now),
                ..Default::default()
            }
            .insert(db)
            .await?;
            backfill_provider_extra(db, tmpl).await?;
            inserted += 1;
        }
    }

    backfill_krill_provider_extra(db).await?;
    backfill_sensenova_provider_extra(db).await?;
    tracing::info!("Provider templates seeded: {inserted} inserted, {updated} updated");
    Ok(inserted + updated)
}

pub(crate) fn is_krill_host(host: &str) -> bool {
    matches!(
        host,
        // krill-code.net 与 krill-code.com 控制台同后端（生产在用）；krill-ai.net 为早期接入域。
        "api-slb.krill-ai.net"
            | "api.krill-ai.net"
            | "api.cdn-krill-ai.com"
            | "api-slb.krill-code.net"
    )
}

pub(crate) fn is_sensenova_host(host: &str) -> bool {
    matches!(host, "token.sensenova.cn" | "platform.sensenova.cn")
}

/// 每次启动幂等对齐历史 Krill Provider 的凭据结构与用量类型。
async fn backfill_krill_provider_extra(db: &DatabaseConnection) -> Result<(), DbErr> {
    let providers = crate::entity::provider::Entity::find().all(db).await?;
    for provider in providers {
        let Some(host) = host_of(&provider.base_url) else {
            continue;
        };
        if !is_krill_host(&host) {
            continue;
        }

        let plain = match crypto::decrypt(&provider.extra) {
            Ok(plain) => plain,
            Err(error) => {
                tracing::warn!(
                    provider_id = provider.id,
                    "回填 Krill provider extra 失败：存储值无法解密：{error}"
                );
                continue;
            }
        };
        let mut extra = match serde_json::from_str::<serde_json::Value>(&plain) {
            Ok(serde_json::Value::Object(extra)) => extra,
            Ok(_) => {
                tracing::warn!(
                    provider_id = provider.id,
                    "回填 Krill provider extra 失败：不是 JSON 对象"
                );
                continue;
            }
            Err(error) => {
                tracing::warn!(
                    provider_id = provider.id,
                    "回填 Krill provider extra 失败：不是合法 JSON 对象：{error}"
                );
                continue;
            }
        };
        let before = extra.clone();
        for key in ["email", "password", "jwt"] {
            extra.entry(key.to_string()).or_insert_with(|| "".into());
        }
        extra.entry("usage".to_string()).or_insert(true.into());
        extra.insert("usage_type".to_string(), provider.billing_mode.into());
        if extra == before {
            continue;
        }

        let mut active: crate::entity::provider::ActiveModel = provider.into();
        active.extra = Set(crypto::encrypt(
            &serde_json::Value::Object(extra).to_string(),
        ));
        active.update(db).await?;
    }
    Ok(())
}

/// 每次启动幂等对齐历史 SenseNova Provider 的凭据结构。
///
/// SenseNova 模板早已随早期版本 upsert 进库，新增 username/password 键后
/// 走的是 update 分支（不触发 `backfill_provider_extra`），历史 provider 的
/// extra 不会补入缺失键；这里仿 Krill 每次启动无条件对齐。
/// 只补缺、不覆盖：已有 refresh_token/username/password/usage 一律保留。
async fn backfill_sensenova_provider_extra(db: &DatabaseConnection) -> Result<(), DbErr> {
    let providers = crate::entity::provider::Entity::find().all(db).await?;
    for provider in providers {
        let provider_id = provider.id;
        let Some(host) = host_of(&provider.base_url) else {
            continue;
        };
        if !is_sensenova_host(&host) {
            continue;
        }

        let plain = match crypto::decrypt(&provider.extra) {
            Ok(plain) => plain,
            Err(error) => {
                tracing::warn!(
                    provider_id = provider.id,
                    "回填 SenseNova provider extra 失败：存储值无法解密：{error}"
                );
                continue;
            }
        };
        let mut extra = match serde_json::from_str::<serde_json::Value>(&plain) {
            Ok(serde_json::Value::Object(extra)) => extra,
            Ok(_) => {
                tracing::warn!(
                    provider_id = provider.id,
                    "回填 SenseNova provider extra 失败：不是 JSON 对象"
                );
                continue;
            }
            Err(error) => {
                tracing::warn!(
                    provider_id = provider.id,
                    "回填 SenseNova provider extra 失败：不是合法 JSON 对象：{error}"
                );
                continue;
            }
        };
        let before = extra.clone();
        for key in ["refresh_token", "username", "password"] {
            extra.entry(key.to_string()).or_insert_with(|| "".into());
        }
        extra.entry("usage".to_string()).or_insert(true.into());
        if extra == before {
            continue;
        }

        let mut active: crate::entity::provider::ActiveModel = provider.into();
        active.extra = Set(crypto::encrypt(
            &serde_json::Value::Object(extra).to_string(),
        ));
        active.update(db).await?;
        tracing::info!(
            provider_id = provider_id,
            "回填 SenseNova provider extra 缺失键（username/password/refresh_token）"
        );
    }
    Ok(())
}

/// 模板首次插入时，向 base_url host 匹配的既有 provider 的 extra 补齐
/// 模板中存在而 provider 缺失的键（只补缺、不覆盖用户已设值）。
/// 仅在首次插入分支调用；模板更新不触发，尊重用户后续修改。
async fn backfill_provider_extra(
    db: &DatabaseConnection,
    tmpl: &seed::Template,
) -> Result<(), DbErr> {
    let Some(host) = host_of(tmpl.base_url) else {
        return Ok(());
    };
    let template_extra: serde_json::Map<String, serde_json::Value> =
        match serde_json::from_str(tmpl.extra) {
            Ok(map) => map,
            Err(_) => return Ok(()),
        };
    if template_extra.is_empty() {
        return Ok(());
    }

    let providers = crate::entity::provider::Entity::find().all(db).await?;
    for p in providers {
        if host_of(&p.base_url).as_deref() != Some(host.as_str()) {
            continue;
        }
        let Ok(plain) = crypto::decrypt(&p.extra) else {
            tracing::warn!(
                provider_id = p.id,
                "回填 provider extra 失败：存储值无法解密"
            );
            continue;
        };
        let Ok(serde_json::Value::Object(mut map)) =
            serde_json::from_str::<serde_json::Value>(&plain)
        else {
            continue;
        };
        let before = map.len();
        for (key, value) in &template_extra {
            map.entry(key.clone()).or_insert(value.clone());
        }
        if map.len() == before {
            continue;
        }
        let mut am: crate::entity::provider::ActiveModel = p.into();
        am.extra = Set(crypto::encrypt(&serde_json::Value::Object(map).to_string()));
        am.update(db).await?;
        tracing::info!(provider_template = tmpl.name, "回填 provider extra 缺失键");
    }
    Ok(())
}

/// 从 base_url 中提取 host（去协议、路径、端口、`${VAR}` 占位符）。
///
/// 示例：`https://api.deepseek.com` → `api.deepseek.com`；
/// `https://api.302.ai/v1` → `api.302.ai`；
/// `https://${CLOUDFLARE_ACCOUNT_ID}/ai/v1` → 含占位符，返回 None。
pub(crate) fn host_of(base_url: &str) -> Option<String> {
    let rest = base_url.split("://").nth(1).unwrap_or(base_url);
    // 去掉路径部分（保留 host 段）
    let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    // 去掉端口
    let host = host.split(':').next().unwrap_or(host);
    if host.contains('$') || host.contains('{') || host.is_empty() {
        return None;
    }
    Some(host.to_ascii_lowercase())
}

/// 根据域名匹配 provider 模板（返回全部命中，保留种子顺序）。
///
/// 按 base_url 的 host（忽略协议/路径/端口/大小写）匹配；同一 host 下可能有
/// 多个模板（如 Alibaba 的按量/订阅、MiniMax 的国内外、Coding Plan 变体等），
/// 前端展示全部候选让用户选择。含 `${VAR}` 占位符的 base_url 无法匹配，跳过。
pub async fn find_by_domain_all(
    db: &DatabaseConnection,
    domain: &str,
) -> Result<Vec<provider_template::Model>, DbErr> {
    let domain = domain.trim().to_ascii_lowercase();
    if domain.is_empty() {
        return Ok(Vec::new());
    }
    let domain_host = host_of(&domain).unwrap_or(domain);

    let templates = Entity::find().all(db).await?;
    Ok(templates
        .into_iter()
        .filter(|t| host_of(&t.base_url).as_deref() == Some(domain_host.as_str()))
        .collect())
}

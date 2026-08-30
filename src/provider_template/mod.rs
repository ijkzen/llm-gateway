pub mod seed;

#[cfg(test)]
mod tests;

use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, Set,
};

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
            inserted += 1;
        }
    }

    tracing::info!("Provider templates seeded: {inserted} inserted, {updated} updated");
    Ok(inserted + updated)
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

/// 按域名匹配 provider 模板（返回第一条命中，兼容旧调用方）。
///
/// 按 base_url 的 host（忽略协议/路径/端口/大小写）匹配；无匹配返回 None。
/// 含 `${VAR}` 占位符的 base_url 无法匹配，跳过。
pub async fn find_by_domain(
    db: &DatabaseConnection,
    domain: &str,
) -> Result<Option<provider_template::Model>, DbErr> {
    Ok(find_by_domain_all(db, domain).await?.into_iter().next())
}

//! 统计主体键助手（ADR-0021 读路径）：主体键解析、单主体降级守卫与
//! api_key id↔name 归并的单一所有者。registry.rs 保持纯常量表，
//! 凡触 DB 的主体助手收敛于此。
//!
//! 核心不变量：「单主体快照读取必须持有已解析的主体键」——过滤形态要求的
//! 主体（pm/Key）已删导致键解析失败时，快照贡献必须为空（按全量主体行取数
//! 会把其它主体加进来、高估数字），一律整窗实时兑底。

use std::collections::{BTreeMap, HashMap};

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use super::reader::Coverage;
use super::registry::ENTITY_WHOLE;

/// 供应商模型主体键解析：(provider_id, provider_model_id 文本) → provider_model
/// 自增主键（快照 model 行的 entity 文本）。行已删（映射不到）返回 None。
pub(crate) async fn resolve_pm_key(
    db: &DatabaseConnection,
    provider_id: i32,
    provider_model_id: &str,
) -> Option<String> {
    db.query_one_raw(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT CAST(model_id AS TEXT) AS v FROM provider_model \
         WHERE provider_id = ? AND provider_model_id = ?"
            .to_string(),
        [provider_id.into(), provider_model_id.into()],
    ))
    .await
    .ok()
    .flatten()
    .and_then(|row| row.try_get("", "v").ok())
}

/// API Key 主体键解析：名称（request.api_key_name）→ api_key 自增主键
/// （快照 api_key 行的 entity 文本）。Key 已删返回 None。
pub(crate) async fn resolve_api_key_id(db: &DatabaseConnection, name: &str) -> Option<String> {
    db.query_one_raw(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT CAST(id AS TEXT) AS v FROM api_key WHERE name = ?".to_string(),
        [name.into()],
    ))
    .await
    .ok()
    .flatten()
    .and_then(|row| row.try_get("", "v").ok())
}

/// 单主体降级守卫：非 whole 主体而键缺失（解析失败/未解析）时整窗兑底。
/// 返回 true = 已降级。赛马类「列举全部主体」的读取（exact 本就为 None）
/// 不经过此守卫——它只服务「过滤形态指定了单一主体」的读取。
pub(crate) fn demote_if_unresolved(
    cov: &mut Coverage,
    start: i64,
    end: i64,
    entity_type: &str,
    exact: Option<&str>,
) -> bool {
    if entity_type == ENTITY_WHOLE || exact.is_some() {
        return false;
    }
    cov.snapshots.clear();
    cov.live = vec![(start, end)];
    true
}

/// api_key id→名称解析（批量；缺失即已删，不进结果表）。
async fn api_key_name_map(
    db: &DatabaseConnection,
    ids: &[String],
) -> Result<HashMap<String, String>, String> {
    let mut map = HashMap::new();
    if ids.is_empty() {
        return Ok(map);
    }
    let in_list = ids
        .iter()
        .map(|k| format!("'{k}'"))
        .collect::<Vec<_>>()
        .join(", ");
    let rows = db
        .query_all_raw(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "SELECT CAST(id AS TEXT) AS k, name AS n FROM api_key \
                 WHERE CAST(id AS TEXT) IN ({in_list})"
            ),
        ))
        .await
        .map_err(|e| e.to_string())?;
    for row in rows {
        let k: String = row.try_get("", "k").unwrap_or_default();
        let n: String = row.try_get("", "n").unwrap_or_default();
        map.insert(k, n);
    }
    Ok(map)
}

/// api_key 归并到名称域：快照侧按数字 id 键归并、兑底侧按名称键归并的混合
/// map，统一折算为 (名称 → 值)。数字 id 键解析名称，已删 Key（解析不到）的
/// 快照闭桶贡献按生成语义丢弃（孤儿名称行由兑底侧保留）。
pub(crate) async fn api_key_reconcile_names<T>(
    db: &DatabaseConnection,
    by_id: HashMap<String, T>,
) -> Result<BTreeMap<String, T>, String>
where
    T: Default + std::ops::AddAssign,
{
    let id_keys: Vec<String> = by_id
        .keys()
        .filter(|k| k.parse::<i64>().is_ok())
        .cloned()
        .collect();
    let name_map = api_key_name_map(db, &id_keys).await?;
    let mut by_name: BTreeMap<String, T> = BTreeMap::new();
    for (key, value) in by_id {
        let name = if key.parse::<i64>().is_ok() {
            match name_map.get(&key) {
                Some(name) => name.clone(),
                None => continue, // 已删 Key：快照贡献丢弃
            }
        } else {
            key // 兑底侧名称键原样保留
        };
        *by_name.entry(name).or_default() += value;
    }
    Ok(by_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats_snapshot::{Frame, Level};

    async fn test_db() -> DatabaseConnection {
        let db = crate::db::connect("sqlite::memory:").await.unwrap();
        let ts = "2024-01-01T00:00:00Z";
        db.execute_unprepared(&format!(
            "INSERT INTO provider (id, name, enable, base_url, api_key, custom_header, \
             protocol_type, billing_mode, extra, sort_order, proxy_enabled, proxy_addr, \
             created_at, updated_at) \
             VALUES (7, 'p7', 1, 'https://a.example', 'k', '{{}}', 0, 1, '{{}}', 0, 0, '', \
             '{ts}', '{ts}')"
        ))
        .await
        .unwrap();
        db.execute_unprepared(&format!(
            "INSERT INTO provider_model (model_id, provider_id, provider_model_id, \
             context_length, max_output_tokens, reasoning, tool_use, image_understand, \
             video_understand, proxy_enabled, proxy_addr, created_at, updated_at) \
             VALUES (42, 7, 'gpt-4o', 8000, 2000, 0, 0, 0, 0, 0, '', '{ts}', '{ts}')"
        ))
        .await
        .unwrap();
        db.execute_unprepared(&format!(
            "INSERT INTO api_key (id, name, key, key_hash, enable, created_at, updated_at) \
             VALUES (5, 'alice', 'lg-aaa', NULL, 1, '{ts}', '{ts}')"
        ))
        .await
        .unwrap();
        db
    }

    #[tokio::test]
    async fn resolve_pm_key_hits_and_misses() {
        let db = test_db().await;
        assert_eq!(
            resolve_pm_key(&db, 7, "gpt-4o").await.as_deref(),
            Some("42")
        );
        assert_eq!(resolve_pm_key(&db, 7, "deleted-model").await, None);
        assert_eq!(resolve_pm_key(&db, 999, "gpt-4o").await, None);
    }

    #[tokio::test]
    async fn resolve_api_key_id_hits_and_misses() {
        let db = test_db().await;
        assert_eq!(resolve_api_key_id(&db, "alice").await.as_deref(), Some("5"));
        assert_eq!(resolve_api_key_id(&db, "deleted-key").await, None);
    }

    #[test]
    fn demote_only_fires_for_non_whole_with_missing_key() {
        let mut cov = Coverage {
            snapshots: vec![Frame {
                level: Level::Day,
                start: 0,
                end: 86_400_000,
            }],
            live: vec![(86_400_000, 100_000_000)],
        };
        // whole 主体：exact None 合法，不降级。
        assert!(!demote_if_unresolved(&mut cov, 0, 100, ENTITY_WHOLE, None));
        assert_eq!(cov.snapshots.len(), 1);
        // 非 whole 但键已解析：不降级。
        assert!(!demote_if_unresolved(&mut cov, 0, 100, "model", Some("42")));
        assert_eq!(cov.snapshots.len(), 1);
        // 非 whole 且键缺失：整窗兑底。
        assert!(demote_if_unresolved(&mut cov, 0, 100, "model", None));
        assert!(cov.snapshots.is_empty());
        assert_eq!(cov.live, vec![(0, 100)]);
    }

    #[tokio::test]
    async fn reconcile_names_drops_deleted_and_keeps_orphans() {
        let db = test_db().await;
        let mut by_id: HashMap<String, f64> = HashMap::new();
        by_id.insert("5".into(), 3.0); // 现存 Key alice
        by_id.insert("99".into(), 7.0); // 已删 Key：丢弃
        by_id.insert("orphan-name".into(), 2.0); // 兑底孤儿名称：保留
        let by_name = api_key_reconcile_names(&db, by_id).await.unwrap();
        assert_eq!(by_name.get("alice"), Some(&3.0));
        assert_eq!(by_name.get("orphan-name"), Some(&2.0));
        assert_eq!(by_name.len(), 2);
    }
}

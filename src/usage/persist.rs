//! 用量数据库缓存：10 分钟新鲜度直出 + 定时刷新 + 订阅额度耗尽自动停用/恢复。
//!
//! - `read_usage_cache` / `write_usage_cache`：数据库缓存读写（fresh ≤ 10 分钟）。
//! - `fetch_and_store`：真实抓取一次用量并落库（供接口缓存过期与 LB 选路兜底）。
//! - `refresh_all_usage`：定时任务主体，刷新全部「已开启用量展示」的供应商并执行额度门控。
//! - `apply_usage_gate`：订阅制额度耗尽 → 停用 Provider 及其全部虚拟模型子模型；
//!   恢复可用 → 反向启用（「不可用」= 任一厂商已提供的窗口剩余为 0）。

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, Set,
};
use serde_json::Value;

use crate::entity::{provider, provider_model, usage_cache, virtual_model_item};
use crate::usage::types::{UsageData, UsageKind};
use crate::usage::{UsageCache, UsageError};

/// 数据库缓存新鲜时长：10 分钟内直出缓存，过期则重新抓取。
pub const DB_USAGE_CACHE_TTL: Duration = Duration::from_secs(600);

fn usage_json_encode(data: &UsageData) -> Result<String, DbErr> {
    serde_json::to_string(data).map_err(|e| DbErr::Custom(format!("用量缓存序列化失败：{e}")))
}

/// 读取供应商用量的数据库缓存；`fetched_at` 距今超过 10 分钟视为过期（返回 None）。
pub async fn read_usage_cache(
    db: &DatabaseConnection,
    provider_id: i32,
) -> Result<Option<UsageData>, DbErr> {
    let row = usage_cache::Entity::find()
        .filter(usage_cache::Column::ProviderId.eq(provider_id))
        .one(db)
        .await?;
    let Some(row) = row else { return Ok(None) };
    let age = Utc::now().signed_duration_since(row.fetched_at);
    if age > chrono::Duration::from_std(DB_USAGE_CACHE_TTL).unwrap_or_default() {
        return Ok(None);
    }
    // 反序列化失败同样按缓存缺失处理（下次抓取会覆盖写入）。
    Ok(serde_json::from_str(&row.usage_json).ok())
}

/// 写入/更新某供应商的用量缓存行（按 provider_id upsert）。
pub async fn write_usage_cache(db: &DatabaseConnection, data: &UsageData) -> Result<(), DbErr> {
    let usage_json = usage_json_encode(data)?;
    let now = Utc::now();
    let existing = usage_cache::Entity::find()
        .filter(usage_cache::Column::ProviderId.eq(data.provider_id))
        .one(db)
        .await?;
    match existing {
        Some(row) => {
            let mut active: usage_cache::ActiveModel = row.into();
            active.usage_json = Set(usage_json);
            active.fetched_at = Set(data.fetched_at);
            active.updated_at = Set(now);
            active.update(db).await?;
        }
        None => {
            usage_cache::ActiveModel {
                provider_id: Set(data.provider_id),
                usage_json: Set(usage_json),
                fetched_at: Set(data.fetched_at),
                created_at: Set(now),
                updated_at: Set(now),
                ..Default::default()
            }
            .insert(db)
            .await?;
        }
    }
    Ok(())
}

/// 删除某供应商的用量缓存行（Provider 更新/删除后调用，避免旧凭据的缓存残留）。
pub async fn invalidate_usage_cache(
    db: &DatabaseConnection,
    provider_id: i32,
) -> Result<(), DbErr> {
    usage_cache::Entity::delete_many()
        .filter(usage_cache::Column::ProviderId.eq(provider_id))
        .exec(db)
        .await?;
    Ok(())
}

/// 真实抓取一次供应商用量并写入数据库缓存，返回新数据。
pub async fn fetch_and_store(
    db: &DatabaseConnection,
    provider_id: i32,
) -> Result<UsageData, UsageError> {
    // force_refresh=true 绕过 60s 内存缓存，确保是真请求；落库后读接口与
    // LB 排序可直接命中 10 分钟数据库缓存。
    let data =
        crate::usage::query_provider_usage(db, &UsageCache::default(), provider_id, true).await?;
    if let Err(e) = write_usage_cache(db, &data).await {
        tracing::warn!(provider_id, "用量缓存落库失败：{e}");
    }
    Ok(data)
}

/// 刷新全部「已开启用量展示」（extra.usage=true，不看 enable）的供应商用量并落库，
/// 订阅制供应商成功抓取后执行额度自动停用/恢复。返回成功落库的供应商数；
/// 单家失败仅记录日志，不中断整体。
pub async fn refresh_all_usage(db: &DatabaseConnection) -> Result<usize, DbErr> {
    let providers = provider::Entity::find().all(db).await?;
    let mut targets = Vec::new();
    for p in providers {
        let extra = match serde_json::from_str::<Value>(&p.extra) {
            Ok(Value::Object(map)) => map,
            _ => Default::default(),
        };
        if extra.get("usage").and_then(Value::as_bool).unwrap_or(false) {
            targets.push(p);
        }
    }
    if targets.is_empty() {
        return Ok(0);
    }

    // 每家用独立 reqwest 客户端；限并发避免同时打开过多连接。
    let semaphore = Arc::new(tokio::sync::Semaphore::new(4));
    let mut set = tokio::task::JoinSet::new();
    for p in targets {
        let provider_id = p.id;
        let db = db.clone();
        let semaphore = semaphore.clone();
        set.spawn(async move {
            let _permit = semaphore.acquire().await.expect("用量刷新信号量未关闭");
            (p, fetch_and_store(&db, provider_id).await)
        });
    }

    let mut ok = 0;
    while let Some(outcome) = set.join_next().await {
        match outcome {
            Ok((p, Ok(data))) => {
                ok += 1;
                if let Err(e) = apply_usage_gate(db, &p, &data).await {
                    tracing::warn!(provider_id = p.id, "用量额度门控执行失败：{e}");
                }
            }
            Ok((p, Err(e))) => {
                tracing::warn!(provider_id = p.id, "用量刷新失败：{e}");
            }
            Err(e) => tracing::warn!("用量刷新任务异常：{e}"),
        }
    }
    Ok(ok)
}

/// 订阅制「当前是否可用」判定：全部厂商已提供的窗口剩余 > 0 → true；
/// 任一已提供窗口剩余为 0 → false；无任何可用窗口数据（无法判定）→ None。
/// 调用方在 None 时必须保持原状，避免上游抖动误伤。
fn subscription_usable(data: &UsageData) -> Option<bool> {
    if data.kind != UsageKind::Quota {
        return None;
    }
    let mut saw_available = false;
    for window in &data.windows {
        match window.remaining_percent_value() {
            Some(p) => {
                saw_available = true;
                if p <= 0.0 {
                    return Some(false);
                }
            }
            None => {}
        }
    }
    saw_available.then_some(true)
}

async fn set_provider_enabled(
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
    let mut active: provider::ActiveModel = row.into();
    active.enable = Set(enabled);
    active.updated_at = Set(Utc::now());
    active.update(db).await?;
    Ok(true)
}

/// 级联开关该供应商名下全部虚拟模型子模型，返回实际变更的条目数。
async fn set_items_enabled(
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
    let now = Utc::now();
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
    Ok(count)
}

/// 订阅额度自动停用/恢复。仅对订阅制（billing_mode=1）且能判定的数据生效；
/// 抓取失败/无窗口数据的场景由调用方保证不传入或不做动作。
pub async fn apply_usage_gate(
    db: &DatabaseConnection,
    p: &provider::Model,
    data: &UsageData,
) -> Result<(), DbErr> {
    if p.billing_mode != 1 {
        return Ok(());
    }
    let usable = match subscription_usable(data) {
        Some(v) => v,
        None => return Ok(()),
    };
    if usable && !p.enable {
        set_provider_enabled(db, p.id, true).await?;
        let items = set_items_enabled(db, p.id, true).await?;
        tracing::info!(
            provider_id = p.id,
            items,
            "订阅额度已恢复，自动启用供应商及其全部虚拟模型子模型"
        );
    } else if !usable && p.enable {
        set_provider_enabled(db, p.id, false).await?;
        let items = set_items_enabled(db, p.id, false).await?;
        tracing::info!(
            provider_id = p.id,
            items,
            "订阅额度已耗尽，自动停用供应商及其全部虚拟模型子模型"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::types::{
        QuotaWindow, UsageData, UsageKind, WindowKind, empty_windows, set_window,
    };

    fn quota_data(provider_id: i32, windows: Vec<crate::usage::types::QuotaWindow>) -> UsageData {
        UsageData {
            provider_id,
            fetched_at: Utc::now(),
            kind: UsageKind::Quota,
            plan: Some("pro".to_string()),
            windows,
            balances: vec![],
        }
    }

    fn window(kind: WindowKind, remaining: f64) -> QuotaWindow {
        QuotaWindow::from_remaining_percent(kind, remaining, None)
    }

    #[test]
    fn subscription_usable_all_windows_remaining() {
        let mut windows = empty_windows();
        set_window(&mut windows, window(WindowKind::FiveHour, 42.0));
        set_window(&mut windows, window(WindowKind::Weekly, 80.0));
        assert_eq!(subscription_usable(&quota_data(1, windows)), Some(true));
    }

    #[test]
    fn subscription_usable_any_exhausted_is_unusable() {
        // 周剩余为 0 → 不可用，即使 5h 还有剩余。
        let mut windows = empty_windows();
        set_window(&mut windows, window(WindowKind::FiveHour, 5.0));
        set_window(&mut windows, window(WindowKind::Weekly, 0.0));
        set_window(&mut windows, window(WindowKind::Monthly, 90.0));
        assert_eq!(subscription_usable(&quota_data(1, windows)), Some(false));
    }

    #[test]
    fn subscription_usable_no_provided_window_is_none() {
        // 厂商未提供任何窗口数据 → 无法判定。
        assert_eq!(subscription_usable(&quota_data(1, empty_windows())), None);
        // 余额形态的订阅供应商 → 无法判定。
        let balance = UsageData {
            provider_id: 1,
            fetched_at: Utc::now(),
            kind: UsageKind::Balance,
            plan: None,
            windows: vec![],
            balances: vec![],
        };
        assert_eq!(subscription_usable(&balance), None);
    }

    #[tokio::test]
    async fn cache_write_read_roundtrip_and_stale() {
        let db = crate::db::connect("sqlite::memory:").await.unwrap();
        assert!(read_usage_cache(&db, 1).await.unwrap().is_none());
        write_usage_cache(&db, &quota_data(1, empty_windows()))
            .await
            .unwrap();
        let read = read_usage_cache(&db, 1).await.unwrap().unwrap();
        assert_eq!(read.provider_id, 1);
        assert_eq!(read.kind, UsageKind::Quota);

        // 回拨 fetched_at 到 11 分钟前 → 视为过期。
        let row = usage_cache::Entity::find()
            .filter(usage_cache::Column::ProviderId.eq(1))
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        let mut active: usage_cache::ActiveModel = row.into();
        active.fetched_at = Set(Utc::now() - chrono::Duration::minutes(11));
        active.update(&db).await.unwrap();
        assert!(read_usage_cache(&db, 1).await.unwrap().is_none());

        // 再次写入刷新后恢复可读。
        write_usage_cache(&db, &quota_data(1, empty_windows()))
            .await
            .unwrap();
        assert!(read_usage_cache(&db, 1).await.unwrap().is_some());

        invalidate_usage_cache(&db, 1).await.unwrap();
        assert!(read_usage_cache(&db, 1).await.unwrap().is_none());
    }
}

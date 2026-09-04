//! 用量数据库缓存：10 分钟新鲜度直出 + 定时刷新 + 订阅额度耗尽自动停用/恢复。
//!
//! - `read_usage_cache` / `write_usage_cache`：数据库缓存读写（fresh ≤ 10 分钟）。
//! - `fetch_and_store`：真实抓取一次用量并落库（供接口缓存过期与 LB 选路兜底）。
//! - `refresh_all_usage`：定时任务主体，刷新全部「已开启用量展示」的供应商并执行额度门控。
//! - `apply_usage_gate`：订阅制额度耗尽或按量余额耗尽 → 停用 Provider 及其全部虚拟模型子模型；
//!   恢复可用 → 反向启用（「不可用」= 订阅任一已提供窗口剩余为 0，或按量查得到余额且合计为 0）。

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, Set,
};

use crate::entity::{provider, usage_cache};
use crate::usage::UsageError;
use crate::usage::types::UsageData;

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
    // 真实抓取（无内存缓存）；落库后读接口与 LB 排序命中 10 分钟数据库缓存。
    let data = crate::usage::query_provider_usage(db, provider_id).await?;
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
        if super::usage_enabled(&p.extra) {
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

// Provider 及其虚拟模型子模型的启用状态开关已收编到 `crate::provider_repo`
// （`set_provider_enabled` / `set_items_enabled`），接口与定时任务共用同一入口并输出日志。
// 「订阅制是否可用」判定已收敛到 `UsageData::subscription_usable`（src/usage/types.rs），
// 用量门控与 LB 选路共用同一口径。

/// 用量额度自动停用/恢复。
///
/// - 订阅制（billing_mode=1）：按 `subscription_usable` 判定（任一已提供窗口剩余为 0 即不可用）。
/// - 按量付费（billing_mode=0）：按 `balance_usable` 判定（查得到余额且合计为 0 即不可用）。
///
/// 无法判定（None）或未开启用量查询的供应商不做任何动作；抓取失败/无数据的场景由调用方保证不传入。
pub async fn apply_usage_gate(
    db: &DatabaseConnection,
    p: &provider::Model,
    data: &UsageData,
) -> Result<(), DbErr> {
    let Some(usable) = data.usable_for_billing_mode(p.billing_mode) else {
        return Ok(());
    };
    let (recovered_msg, exhausted_msg) = if p.billing_mode == 1 {
        (
            "订阅额度已恢复，自动启用供应商及其全部虚拟模型子模型",
            "订阅额度已耗尽，自动停用供应商及其全部虚拟模型子模型",
        )
    } else {
        (
            "余额已恢复，自动启用供应商及其全部虚拟模型子模型",
            "余额已耗尽，自动停用供应商及其全部虚拟模型子模型",
        )
    };
    // 连续失败禁用（failure_disabled）不能由普通用量刷新解除；由手动启用或自动恢复探测处理。
    if usable && !p.enable && !p.failure_disabled {
        crate::provider_repo::set_provider_enabled(db, p.id, true).await?;
        let items = crate::provider_repo::set_items_enabled(db, p.id, true).await?;
        tracing::info!(provider_id = p.id, items, "{recovered_msg}");
    } else if !usable && p.enable {
        crate::provider_repo::set_provider_enabled(db, p.id, false).await?;
        let items = crate::provider_repo::set_items_enabled(db, p.id, false).await?;
        tracing::info!(provider_id = p.id, items, "{exhausted_msg}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::provider_model;
    use crate::entity::virtual_model;
    use crate::entity::virtual_model_item;
    use crate::usage::types::{
        BalanceItem, QuotaWindow, UsageData, UsageKind, WindowKind, empty_windows, set_window,
    };

    fn balance_data(provider_id: i32, amounts: &[f64]) -> UsageData {
        UsageData {
            provider_id,
            fetched_at: Utc::now(),
            kind: UsageKind::Balance,
            plan: None,
            windows: vec![],
            balances: amounts
                .iter()
                .enumerate()
                .map(|(i, a)| BalanceItem {
                    label: "余额".to_string(),
                    amount: *a,
                    currency: None,
                    primary: i == 0,
                })
                .collect(),
        }
    }

    async fn seed_balance_provider(db: &DatabaseConnection) -> (i32, i32) {
        let now = Utc::now();
        let p = provider::ActiveModel {
            name: Set("按量供应商".to_string()),
            enable: Set(true),
            base_url: Set("https://api.deepseek.com/v1".to_string()),
            api_key: Set(crate::crypto::encrypt("sk-x")),
            custom_header: Set("{}".to_string()),
            protocol_type: Set(0),
            billing_mode: Set(0),
            extra: Set(r#"{"usage": true, "usage_type": 0}"#.to_string()),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();

        let m = provider_model::ActiveModel {
            provider_id: Set(p.id),
            provider_model_id: Set("deepseek-chat".to_string()),
            context_length: Set(64000),
            max_output_tokens: Set(8192),
            reasoning: Set(false),
            tool_use: Set(true),
            image_understand: Set(false),
            video_understand: Set(false),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();

        let vm = virtual_model::ActiveModel {
            display_id: Set("vm-balance".to_string()),
            enable: Set(true),
            load_balancing_strategy: Set(0),
            fallback_strategy: Set(1),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();

        virtual_model_item::ActiveModel {
            virtual_model_id: Set(vm.virtual_model_id),
            model_id: Set(m.model_id),
            enable: Set(true),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();

        (p.id, m.model_id)
    }

    async fn provider_enabled(db: &DatabaseConnection, id: i32) -> bool {
        provider::Entity::find_by_id(id)
            .one(db)
            .await
            .unwrap()
            .unwrap()
            .enable
    }

    async fn item_enabled(db: &DatabaseConnection, model_id: i32) -> bool {
        virtual_model_item::Entity::find()
            .filter(virtual_model_item::Column::ModelId.eq(model_id))
            .one(db)
            .await
            .unwrap()
            .unwrap()
            .enable
    }

    #[tokio::test]
    async fn balance_exhaustion_disables_and_restore_reenables() {
        let db = crate::db::connect("sqlite::memory:").await.unwrap();
        let (pid, model_id) = seed_balance_provider(&db).await;

        // 余额耗尽（合计 0）→ 停用 provider 及其虚拟模型子模型。
        let p = provider::Entity::find_by_id(pid)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        apply_usage_gate(&db, &p, &balance_data(pid, &[0.0]))
            .await
            .unwrap();
        assert!(!provider_enabled(&db, pid).await);
        assert!(!item_enabled(&db, model_id).await);

        // 余额恢复（>0）→ 恢复启用 provider 及其子模型。
        let p = provider::Entity::find_by_id(pid)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        apply_usage_gate(&db, &p, &balance_data(pid, &[50.0]))
            .await
            .unwrap();
        assert!(provider_enabled(&db, pid).await);
        assert!(item_enabled(&db, model_id).await);
    }

    #[tokio::test]
    async fn balance_unjudgeable_keeps_state() {
        let db = crate::db::connect("sqlite::memory:").await.unwrap();
        let (pid, model_id) = seed_balance_provider(&db).await;

        // 查不到余额（空 balances）→ 无法判定，保持原状。
        let p = provider::Entity::find_by_id(pid)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        apply_usage_gate(&db, &p, &balance_data(pid, &[]))
            .await
            .unwrap();
        assert!(provider_enabled(&db, pid).await);
        assert!(item_enabled(&db, model_id).await);
    }

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
        assert_eq!(quota_data(1, windows).subscription_usable(), Some(true));
    }

    #[test]
    fn subscription_usable_any_exhausted_is_unusable() {
        // 周剩余为 0 → 不可用，即使 5h 还有剩余。
        let mut windows = empty_windows();
        set_window(&mut windows, window(WindowKind::FiveHour, 5.0));
        set_window(&mut windows, window(WindowKind::Weekly, 0.0));
        set_window(&mut windows, window(WindowKind::Monthly, 90.0));
        assert_eq!(quota_data(1, windows).subscription_usable(), Some(false));
    }

    #[test]
    fn subscription_usable_daily_exhausted_is_unusable() {
        let windows = vec![window(WindowKind::Daily, 0.0)];
        assert_eq!(quota_data(1, windows).subscription_usable(), Some(false));
    }

    #[test]
    fn subscription_usable_no_provided_window_is_none() {
        // 厂商未提供任何窗口数据 → 无法判定。
        assert_eq!(quota_data(1, empty_windows()).subscription_usable(), None);
        // 余额形态的订阅供应商 → 无法判定。
        let balance = UsageData {
            provider_id: 1,
            fetched_at: Utc::now(),
            kind: UsageKind::Balance,
            plan: None,
            windows: vec![],
            balances: vec![],
        };
        assert_eq!(balance.subscription_usable(), None);
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

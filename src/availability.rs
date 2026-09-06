//! 供应商可用性状态机（ADR-0003）。
//!
//! 「供应商能不能用」的唯一 owner：停用原因四值（NULL=正常启用 / failure=连续
//! 失败禁用 / quota=额度耗尽 / manual=手动停用）与 enable 列的镜像不变式
//! （启用 ⇔ NULL）由本模块统一写入保证；失败连击计数器的复位规则
//! （「禁用不碰、恢复与手动启用必须清零」）也收拢在此。
//!
//! 动作式入口：
//! - [`disable_for_quota`] / [`recover_quota`]：额度门控（usage_refresh 与失败复查共用）
//! - [`disable_for_failures`] / [`recover_probe`]：连续失败熔断与自动恢复探测
//! - [`enable_manual`] / [`disable_manual`]：管理员手动启停
//! - [`on_forward_failure`]：转发失败入口（计数 + 达阈值熔断），复查触发留在转发侧
//!
//! 各动作自带幂等、级联（见 [`set_items_enabled`] 的 `cascade_disabled` 标记语义）
//! 与结构化日志。额度刷新只解除 `quota` 态；`manual` 态不被任何自动流程触碰。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, DbErr, EntityTrait,
    QueryFilter, TransactionTrait,
};

use crate::entity::{provider, virtual_model_item};

/// 停用原因（provider.disabled_reason 的非空取值，NULL 表示正常启用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisabledReason {
    /// 连续失败达到阈值熔断，仅手动启用或恢复探测解除。
    Failure,
    /// 额度/余额耗尽自动停用，恢复后由用量刷新自动解除。
    Quota,
    /// 管理员手动停用，不被任何自动流程解除。
    Manual,
}

impl DisabledReason {
    pub fn as_str(self) -> &'static str {
        match self {
            DisabledReason::Failure => "failure",
            DisabledReason::Quota => "quota",
            DisabledReason::Manual => "manual",
        }
    }
}

/// provider 粒度的内存连续失败计数器（Clone 共享同一份状态）。
/// 任一转发请求失败 +1（不论失败能否重试），成功/恢复/手动启用清零，进程重启清零。
#[derive(Clone, Default)]
pub struct FailureCounter {
    counters: Arc<Mutex<HashMap<i32, u32>>>,
}

impl FailureCounter {
    /// 记一次失败，返回累计连续失败次数。
    pub fn record_failure(&self, provider_id: i32) -> u32 {
        let mut counters = self.counters.lock().expect("failure counters lock");
        let entry = counters.entry(provider_id).or_insert(0);
        *entry += 1;
        *entry
    }

    /// 清零（成功请求 / 恢复探测 / 手动启用）。
    pub fn reset(&self, provider_id: i32) {
        self.counters
            .lock()
            .expect("failure counters lock")
            .remove(&provider_id);
    }
}

/// 级联开关该供应商名下全部虚拟模型条目，返回实际变更的条目数。
/// 幂等：已处于目标状态的条目跳过。逐行更新，变更后输出日志。
///
/// 分层语义：级联停用（enabled=false）只动当前启用条目，并打上 `cascade_disabled`
/// 标记；级联恢复（enabled=true）只恢复带该标记的条目并清除标记，用户手动关闭的
/// 成员（无标记）保持不变。
pub async fn set_items_enabled(
    db: &impl ConnectionTrait,
    provider_id: i32,
    enabled: bool,
) -> Result<usize, DbErr> {
    let model_ids: Vec<i32> = crate::entity::provider_model::Entity::find()
        .filter(crate::entity::provider_model::Column::ProviderId.eq(provider_id))
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
        active.enable = sea_orm::Set(new_enable);
        active.cascade_disabled = sea_orm::Set(new_flag);
        active.updated_at = sea_orm::Set(now);
        active.update(db).await?;
        count += 1;
    }
    if count > 0 {
        tracing::info!(
            provider_id,
            changed_count = count,
            enable_new = enabled,
            "级联更新虚拟模型子模型启用状态"
        );
    }
    Ok(count)
}

/// 额度/余额耗尽自动停用：仅从可用态触发。`manual`/`failure` 态不动
/// （已停用且来源不同，额度刷新无权覆盖）；已是 quota 态幂等返回 false。
/// `label` 为日志用语（"订阅额度"/"余额"）。
pub async fn disable_for_quota(
    db: &DatabaseConnection,
    provider_id: i32,
    label: &str,
) -> Result<bool, DbErr> {
    let now = chrono::Utc::now();
    let affected = provider::Entity::update_many()
        .col_expr(provider::Column::Enable, Expr::value(false))
        .col_expr(
            provider::Column::DisabledReason,
            Expr::value(DisabledReason::Quota.as_str()),
        )
        .col_expr(provider::Column::UpdatedAt, Expr::value(now))
        .filter(provider::Column::Id.eq(provider_id))
        .filter(provider::Column::Enable.eq(true))
        .exec(db)
        .await?;
    if affected.rows_affected == 0 {
        return Ok(false);
    }
    let items = set_items_enabled(db, provider_id, false).await?;
    tracing::info!(
        provider_id,
        items,
        "{label}已耗尽，自动停用供应商及其全部虚拟模型子模型"
    );
    Ok(true)
}

/// 额度/余额恢复自动启用：仅解除 `quota` 态。`manual` 态不被额度刷新触碰
/// （修复：手动停用的供应商曾被额度恢复自动重新启用），`failure` 态只允许
/// 手动启用或恢复探测解除。`label` 为日志用语（"订阅额度"/"余额"）。
pub async fn recover_quota(
    db: &DatabaseConnection,
    provider_id: i32,
    label: &str,
) -> Result<bool, DbErr> {
    let now = chrono::Utc::now();
    let affected = provider::Entity::update_many()
        .col_expr(provider::Column::Enable, Expr::value(true))
        .col_expr(
            provider::Column::DisabledReason,
            Expr::value(Option::<String>::None),
        )
        .col_expr(provider::Column::UpdatedAt, Expr::value(now))
        .filter(provider::Column::Id.eq(provider_id))
        .filter(provider::Column::DisabledReason.eq(DisabledReason::Quota.as_str()))
        .exec(db)
        .await?;
    if affected.rows_affected == 0 {
        return Ok(false);
    }
    let items = set_items_enabled(db, provider_id, true).await?;
    tracing::info!(
        provider_id,
        items,
        "{label}已恢复，自动启用供应商及其全部虚拟模型子模型"
    );
    Ok(true)
}

/// 连续失败达到阈值时的熔断停用：原子条件更新（可用态 → failure），
/// 并发下仅一个胜出，返回 true。已停用的供应商（manual/quota）不重复打标。
pub async fn disable_for_failures(
    db: &DatabaseConnection,
    provider_id: i32,
    consecutive: u32,
    request_id: &str,
) -> Result<bool, DbErr> {
    let now = chrono::Utc::now();
    let affected = provider::Entity::update_many()
        .col_expr(provider::Column::Enable, Expr::value(false))
        .col_expr(
            provider::Column::DisabledReason,
            Expr::value(DisabledReason::Failure.as_str()),
        )
        .col_expr(provider::Column::UpdatedAt, Expr::value(now))
        .filter(provider::Column::Id.eq(provider_id))
        .filter(provider::Column::DisabledReason.is_null())
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
        "连续失败达到阈值，熔断停用供应商及其全部虚拟模型子模型"
    );
    Ok(true)
}

/// 管理员手动启用：解除任意停用（含连续失败禁用），清零失败计数并级联恢复。
/// 幂等：已处于可用态返回 false。
pub async fn enable_manual(
    db: &DatabaseConnection,
    counters: &FailureCounter,
    provider_id: i32,
) -> Result<bool, DbErr> {
    let Some(row) = provider::Entity::find_by_id(provider_id).one(db).await? else {
        return Ok(false);
    };
    if row.enable && row.disabled_reason.is_none() {
        return Ok(false);
    }
    let was_failure_disabled =
        row.disabled_reason.as_deref() == Some(DisabledReason::Failure.as_str());
    let mut active: provider::ActiveModel = row.into();
    active.enable = sea_orm::Set(true);
    active.disabled_reason = sea_orm::Set(None);
    active.updated_at = sea_orm::Set(chrono::Utc::now());
    active.update(db).await?;
    counters.reset(provider_id);
    let items = set_items_enabled(db, provider_id, true).await?;
    if was_failure_disabled {
        tracing::info!(
            provider_id,
            items,
            "手动启用供应商，解除连续失败禁用并清零失败计数"
        );
    } else {
        tracing::info!(provider_id, items, "手动启用供应商");
    }
    Ok(true)
}

/// 管理员手动停用：标记 `manual` 并级联停用。幂等：已停用（任何来源）返回 false，
/// 不覆盖原停用原因。
pub async fn disable_manual(db: &DatabaseConnection, provider_id: i32) -> Result<bool, DbErr> {
    let Some(row) = provider::Entity::find_by_id(provider_id).one(db).await? else {
        return Ok(false);
    };
    if !row.enable {
        return Ok(false);
    }
    let mut active: provider::ActiveModel = row.into();
    active.enable = sea_orm::Set(false);
    active.disabled_reason = sea_orm::Set(Some(DisabledReason::Manual.as_str().to_string()));
    active.updated_at = sea_orm::Set(chrono::Utc::now());
    active.update(db).await?;
    let items = set_items_enabled(db, provider_id, false).await?;
    tracing::info!(provider_id, items, "手动停用供应商及其全部虚拟模型子模型");
    Ok(true)
}

/// 恢复探测成功后的条件恢复：仅解除 `failure` 态，且要求探测期间 updated_at
/// 未变化（乐观锁，避免旧探测覆盖新状态）。成功即清零失败计数并级联恢复。
pub async fn recover_probe(
    db: &DatabaseConnection,
    counters: &FailureCounter,
    provider_id: i32,
    expected_updated_at: chrono::DateTime<chrono::Utc>,
) -> Result<bool, DbErr> {
    let txn = db.begin().await?;
    let affected = provider::Entity::update_many()
        .col_expr(provider::Column::Enable, Expr::value(true))
        .col_expr(
            provider::Column::DisabledReason,
            Expr::value(Option::<String>::None),
        )
        .col_expr(provider::Column::UpdatedAt, Expr::value(chrono::Utc::now()))
        .filter(provider::Column::Id.eq(provider_id))
        .filter(provider::Column::DisabledReason.eq(DisabledReason::Failure.as_str()))
        .filter(provider::Column::UpdatedAt.eq(expected_updated_at))
        .exec(&txn)
        .await?;
    if affected.rows_affected == 0 {
        txn.commit().await?;
        return Ok(false);
    }
    let items = set_items_enabled(&txn, provider_id, true).await?;
    txn.commit().await?;
    counters.reset(provider_id);
    tracing::info!(provider_id, items, "自动恢复连续失败禁用供应商");
    Ok(true)
}

/// 转发失败入口：失败计数 +1，达到设置阈值即熔断停用。失败复查的异步触发
/// 留在转发侧（依赖 AppState 的节流门与请求上下文）。
pub async fn on_forward_failure(
    db: &DatabaseConnection,
    counters: &FailureCounter,
    provider_id: i32,
    threshold: u32,
    request_id: &str,
) -> Result<(), DbErr> {
    let consecutive = counters.record_failure(provider_id);
    if consecutive >= threshold {
        disable_for_failures(db, provider_id, consecutive, request_id).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{provider_model, virtual_model};
    use sea_orm::{ActiveModelTrait, Set};

    async fn setup() -> DatabaseConnection {
        let db = crate::db::connect("sqlite::memory:").await.unwrap();
        crate::db::migrate(&db).await.unwrap();
        db
    }

    /// 种子：供应商（指定 enable / disabled_reason）+ 名下模型 + 虚拟模型条目，
    /// 返回 (provider_id, model_id)。
    async fn seed(
        db: &DatabaseConnection,
        name: &str,
        enable: bool,
        reason: Option<&str>,
    ) -> (i32, i32) {
        let now = chrono::Utc::now();
        let p = provider::ActiveModel {
            name: Set(name.to_string()),
            enable: Set(enable),
            disabled_reason: Set(reason.map(str::to_string)),
            base_url: Set("https://api.example.com".to_string()),
            api_key: Set(crate::crypto::encrypt("sk-x")),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        let m = provider_model::ActiveModel {
            provider_id: Set(p.id),
            provider_model_id: Set(format!("{name}-model")),
            context_length: Set(128000),
            max_output_tokens: Set(8192),
            reasoning: Set(true),
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
            display_id: Set(format!("vm-{name}")),
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

    async fn row(db: &DatabaseConnection, id: i32) -> provider::Model {
        provider::Entity::find_by_id(id)
            .one(db)
            .await
            .unwrap()
            .unwrap()
    }

    async fn item(db: &DatabaseConnection, model_id: i32) -> virtual_model_item::Model {
        virtual_model_item::Entity::find()
            .filter(virtual_model_item::Column::ModelId.eq(model_id))
            .one(db)
            .await
            .unwrap()
            .unwrap()
    }

    #[tokio::test]
    async fn quota_disable_from_available_then_idempotent() {
        let db = setup().await;
        let counters = FailureCounter::default();
        let (pid, mid) = seed(&db, "p1", true, None).await;

        // 计数器先记两次失败：停用动作不得触碰计数（「禁用不碰计数器」规则）。
        counters.record_failure(pid);
        counters.record_failure(pid);

        assert!(disable_for_quota(&db, pid, "订阅额度").await.unwrap());
        let r = row(&db, pid).await;
        assert!(!r.enable);
        assert_eq!(r.disabled_reason.as_deref(), Some("quota"));
        let it = item(&db, mid).await;
        assert!(!it.enable);
        assert!(it.cascade_disabled, "级联停用应打标记");
        assert_eq!(counters.record_failure(pid), 3, "停用不应清零失败计数");

        // 已是 quota 态：幂等返回 false。
        assert!(!disable_for_quota(&db, pid, "订阅额度").await.unwrap());
    }

    #[tokio::test]
    async fn quota_disable_never_touches_manual_or_failure() {
        let db = setup().await;
        let (manual_id, _) = seed(&db, "p-manual", false, Some("manual")).await;
        let (failure_id, _) = seed(&db, "p-failure", false, Some("failure")).await;

        assert!(!disable_for_quota(&db, manual_id, "订阅额度").await.unwrap());
        assert!(
            !disable_for_quota(&db, failure_id, "订阅额度")
                .await
                .unwrap()
        );
        assert_eq!(
            row(&db, manual_id).await.disabled_reason.as_deref(),
            Some("manual")
        );
        assert_eq!(
            row(&db, failure_id).await.disabled_reason.as_deref(),
            Some("failure")
        );
    }

    #[tokio::test]
    async fn quota_recover_only_quota_state() {
        let db = setup().await;
        let (quota_id, mid) = seed(&db, "p-quota", false, Some("quota")).await;
        let (manual_id, _) = seed(&db, "p-manual", false, Some("manual")).await;
        let (failure_id, _) = seed(&db, "p-failure", false, Some("failure")).await;
        let (active_id, _) = seed(&db, "p-active", true, None).await;

        // 核心回归：manual 态不被额度刷新触碰（修复手动停用被覆盖缺陷）。
        assert!(!recover_quota(&db, manual_id, "订阅额度").await.unwrap());
        assert!(!recover_quota(&db, failure_id, "订阅额度").await.unwrap());
        assert!(!recover_quota(&db, active_id, "订阅额度").await.unwrap());
        assert_eq!(
            row(&db, manual_id).await.disabled_reason.as_deref(),
            Some("manual")
        );
        assert_eq!(
            row(&db, failure_id).await.disabled_reason.as_deref(),
            Some("failure")
        );

        assert!(recover_quota(&db, quota_id, "订阅额度").await.unwrap());
        let r = row(&db, quota_id).await;
        assert!(r.enable);
        assert_eq!(r.disabled_reason, None, "恢复后镜像不变式：启用 ⇔ NULL");
        let it = item(&db, mid).await;
        assert!(it.enable);
        assert!(!it.cascade_disabled, "级联恢复应清除标记");
    }

    #[tokio::test]
    async fn failure_disable_only_from_available_then_idempotent() {
        let db = setup().await;
        let (pid, mid) = seed(&db, "p1", true, None).await;

        assert!(disable_for_failures(&db, pid, 5, "req-1").await.unwrap());
        let r = row(&db, pid).await;
        assert!(!r.enable);
        assert_eq!(r.disabled_reason.as_deref(), Some("failure"));
        assert!(!item(&db, mid).await.enable);

        // 已是 failure 态：条件更新不命中（幂等，并发仅一个胜出）。
        assert!(!disable_for_failures(&db, pid, 6, "req-1").await.unwrap());

        // quota/manual 态不打 failure 标记（保持原停用来源）。
        let (quota_id, _) = seed(&db, "p-quota", false, Some("quota")).await;
        assert!(
            !disable_for_failures(&db, quota_id, 9, "req-1")
                .await
                .unwrap()
        );
        assert_eq!(
            row(&db, quota_id).await.disabled_reason.as_deref(),
            Some("quota")
        );
    }

    #[tokio::test]
    async fn manual_enable_clears_any_reason_and_resets_counter() {
        let db = setup().await;
        let counters = FailureCounter::default();
        let (failure_id, mid) = seed(&db, "p-failure", false, Some("failure")).await;
        let (quota_id, _) = seed(&db, "p-quota", false, Some("quota")).await;

        counters.record_failure(failure_id);
        counters.record_failure(failure_id);

        assert!(enable_manual(&db, &counters, failure_id).await.unwrap());
        let r = row(&db, failure_id).await;
        assert!(r.enable);
        assert_eq!(r.disabled_reason, None);
        assert!(item(&db, mid).await.enable);
        assert_eq!(
            counters.record_failure(failure_id),
            1,
            "手动启用应清零失败计数"
        );

        assert!(enable_manual(&db, &counters, quota_id).await.unwrap());
        assert!(row(&db, quota_id).await.enable);

        // 已可用：幂等返回 false。
        let (active_id, _) = seed(&db, "p-active", true, None).await;
        assert!(!enable_manual(&db, &counters, active_id).await.unwrap());
    }

    #[tokio::test]
    async fn manual_disable_marks_manual_and_is_idempotent() {
        let db = setup().await;
        let (pid, mid) = seed(&db, "p1", true, None).await;

        assert!(disable_manual(&db, pid).await.unwrap());
        let r = row(&db, pid).await;
        assert!(!r.enable);
        assert_eq!(r.disabled_reason.as_deref(), Some("manual"));
        assert!(!item(&db, mid).await.enable);

        // 已停用（任何来源）：不覆盖原停用原因。
        assert!(!disable_manual(&db, pid).await.unwrap());
        assert_eq!(
            row(&db, pid).await.disabled_reason.as_deref(),
            Some("manual")
        );
    }

    #[tokio::test]
    async fn recover_probe_respects_cas_and_reason() {
        let db = setup().await;
        let counters = FailureCounter::default();
        let (pid, mid) = seed(&db, "p1", false, Some("failure")).await;
        let stale = row(&db, pid).await.updated_at;
        counters.record_failure(pid);

        // 乐观锁命中：恢复 + 清零计数 + 级联恢复。
        assert!(recover_probe(&db, &counters, pid, stale).await.unwrap());
        let r = row(&db, pid).await;
        assert!(r.enable);
        assert_eq!(r.disabled_reason, None);
        assert!(item(&db, mid).await.enable);
        assert_eq!(
            counters.record_failure(pid),
            1,
            "恢复探测成功应清零失败计数"
        );

        // 乐观锁不命中（状态已被更新）。
        let (pid2, _) = seed(&db, "p2", false, Some("failure")).await;
        let wrong = chrono::Utc::now();
        assert!(!recover_probe(&db, &counters, pid2, wrong).await.unwrap());
        assert_eq!(
            row(&db, pid2).await.disabled_reason.as_deref(),
            Some("failure")
        );

        // 非 failure 态不恢复。
        let (quota_id, _) = seed(&db, "p3", false, Some("quota")).await;
        let updated = row(&db, quota_id).await.updated_at;
        assert!(
            !recover_probe(&db, &counters, quota_id, updated)
                .await
                .unwrap()
        );
        assert_eq!(
            row(&db, quota_id).await.disabled_reason.as_deref(),
            Some("quota")
        );
    }

    #[tokio::test]
    async fn forward_failure_disables_at_threshold() {
        let db = setup().await;
        let counters = FailureCounter::default();
        let (pid, _) = seed(&db, "p1", true, None).await;
        let threshold = 3;

        on_forward_failure(&db, &counters, pid, threshold, "req-1")
            .await
            .unwrap();
        on_forward_failure(&db, &counters, pid, threshold, "req-1")
            .await
            .unwrap();
        assert!(row(&db, pid).await.enable, "未达阈值不停用");

        on_forward_failure(&db, &counters, pid, threshold, "req-1")
            .await
            .unwrap();
        let r = row(&db, pid).await;
        assert!(!r.enable);
        assert_eq!(r.disabled_reason.as_deref(), Some("failure"));
    }

    #[test]
    fn counts_incrementally_per_provider() {
        let counter = FailureCounter::default();
        assert_eq!(counter.record_failure(1), 1);
        assert_eq!(counter.record_failure(1), 2);
        assert_eq!(counter.record_failure(2), 1, "供应商之间相互独立");
    }

    #[test]
    fn reset_clears_counter() {
        let counter = FailureCounter::default();
        counter.record_failure(1);
        counter.record_failure(1);
        counter.reset(1);
        assert_eq!(counter.record_failure(1), 1, "清零后从 1 重新计");
    }

    #[test]
    fn reset_unknown_provider_is_noop() {
        let counter = FailureCounter::default();
        counter.reset(42);
        assert_eq!(counter.record_failure(42), 1);
    }
}

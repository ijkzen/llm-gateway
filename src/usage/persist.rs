//! 用量数据库缓存：10 分钟新鲜度直出 + 定时刷新 + 订阅额度耗尽自动停用/恢复。
//!
//! - `read_usage_cache` / `write_usage_cache`：数据库缓存读写（fresh ≤ 10 分钟）。
//! - `fetch_and_store`：真实抓取一次用量并落库（供接口缓存过期与 LB 选路兜底）。
//! - `refresh_all_usage`：定时任务主体，刷新全部「已开启用量展示」的供应商并执行额度门控。
//! - `apply_usage_gate`：订阅制额度耗尽或按量余额耗尽 → 停用 Provider 及其全部虚拟模型子模型；
//!   恢复可用 → 反向启用（「不可用」= 订阅任一已提供窗口剩余为 0，或按量查得到余额且合计为 0）。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use sea_orm::{ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, Set};

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
    if !cache_age_fresh(row.fetched_at) {
        return Ok(None);
    }
    // 反序列化失败同样按缓存缺失处理（下次抓取会覆盖写入）。
    Ok(serde_json::from_str(&row.usage_json).ok())
}

/// 一次查询批量读取多家供应商的数据库缓存（LB 选路热路径免逐家往返）。
pub async fn read_usage_cache_many(
    db: &DatabaseConnection,
    provider_ids: &[i32],
) -> Result<HashMap<i32, UsageData>, DbErr> {
    let mut map = HashMap::new();
    if provider_ids.is_empty() {
        return Ok(map);
    }
    let rows = usage_cache::Entity::find()
        .filter(usage_cache::Column::ProviderId.is_in(provider_ids.iter().copied()))
        .all(db)
        .await?;
    for row in rows {
        if cache_age_fresh(row.fetched_at)
            && let Ok(data) = serde_json::from_str::<UsageData>(&row.usage_json)
        {
            map.insert(row.provider_id, data);
        }
    }
    Ok(map)
}

/// 数据库缓存新鲜度判定（fetched_at 距今 ≤ 10 分钟）。
pub(crate) fn cache_age_fresh(fetched_at: chrono::DateTime<Utc>) -> bool {
    cache_age_fresh_at(fetched_at, Utc::now())
}

/// 指定参照时刻的新鲜度判定（批量读共享同一 now，保证同批次口径一致）。
pub(crate) fn cache_age_fresh_at(
    fetched_at: chrono::DateTime<Utc>,
    now: chrono::DateTime<Utc>,
) -> bool {
    let age = now.signed_duration_since(fetched_at);
    age <= chrono::Duration::from_std(DB_USAGE_CACHE_TTL).unwrap_or_default()
}

/// 写入/更新某供应商的用量缓存行（单语句 `ON CONFLICT(provider_id) DO UPDATE`
/// upsert：并发刷新同一供应商不再有 find→insert 两段竞态撞唯一键）。
pub async fn write_usage_cache(db: &DatabaseConnection, data: &UsageData) -> Result<(), DbErr> {
    use sea_orm::sea_query::OnConflict;

    let usage_json = usage_json_encode(data)?;
    let now = Utc::now();
    usage_cache::Entity::insert(usage_cache::ActiveModel {
        provider_id: Set(data.provider_id),
        usage_json: Set(usage_json),
        fetched_at: Set(data.fetched_at),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    })
    .on_conflict(
        OnConflict::column(usage_cache::Column::ProviderId)
            .update_columns([
                usage_cache::Column::UsageJson,
                usage_cache::Column::FetchedAt,
                usage_cache::Column::UpdatedAt,
            ])
            .to_owned(),
    )
    .exec(db)
    .await?;
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
///
/// 落库失败按抓取失败返回（不静默降级成「抓到了但缓存不新鲜」）：调用方
/// （定时刷新/手动刷新/LB 选路兜底）会如实告警或回退，避免 DB 持续故障时
/// 每轮无退避地反复抓取且缓存永远不新鲜的盲区。
pub async fn fetch_and_store(
    db: &DatabaseConnection,
    provider_id: i32,
) -> Result<UsageData, UsageError> {
    // 真实抓取（无内存缓存）；落库后读接口与 LB 排序命中 10 分钟数据库缓存。
    let data = crate::usage::query_provider_usage(db, provider_id).await?;
    write_usage_cache(db, &data)
        .await
        .map_err(|e| UsageError::Database(e.to_string()))?;
    Ok(data)
}

/// 刷新全部「已开启用量展示」（extra.usage=true，不看 enable）的供应商用量并落库，
/// 订阅制供应商成功抓取后执行额度自动停用/恢复。返回成功落库的供应商数；
/// 单家失败仅记录日志，不中断整体。
///
/// 抓取经 `UsageMemCache` 单飞入口（07-01 收敛）：与 LB 选路、手动刷新、失败复查
/// 同刻命中同一家时只发一次厂商调用；成功后顺带回填 mem 缓存（07-02：避免刷新
/// 完 DB 却有内存旧值被优先命中）。失败进入 60s 负缓存窗口。
pub async fn refresh_all_usage(
    db: &DatabaseConnection,
    mem: &crate::usage::mem_cache::UsageMemCache,
) -> Result<usize, DbErr> {
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

    // 客户端按代理维度进程级复用（usage::http，P5）；限并发避免同时打开过多连接。
    let semaphore = Arc::new(tokio::sync::Semaphore::new(4));
    let mut set = tokio::task::JoinSet::new();
    for p in targets {
        let provider_id = p.id;
        let db = db.clone();
        let semaphore = semaphore.clone();
        let mem = mem.clone();
        set.spawn(async move {
            let _permit = semaphore.acquire().await.expect("用量刷新信号量未关闭");
            // force=true：定时刷新是真实重取（不复用新鲜缓存），否则「刷新」
            // 会退化成读旧值；单飞与代次护栏仍在。
            let result = mem
                .fetch_shared_stored(&db, provider_id, true)
                .await
                .map_err(|e| e.to_string());
            (p, result)
        });
    }

    let mut ok = 0;
    while let Some(outcome) = set.join_next().await {
        match outcome {
            Ok((p, Ok(data))) => {
                ok += 1;
                if let Err(e) = apply_usage_gate(db, &p, &data).await {
                    tracing::warn!(
                        provider_id = p.id,
                        provider_name = &p.name,
                        "供应商「{}」用量额度门控执行失败：{e}",
                        p.name
                    );
                }
            }
            Ok((p, Err(e))) => {
                tracing::warn!(
                    provider_id = p.id,
                    provider_name = &p.name,
                    "供应商「{}」用量刷新失败：{e}",
                    p.name
                );
            }
            Err(e) => tracing::warn!("用量刷新任务异常：{e}"),
        }
    }
    Ok(ok)
}

// Provider 及其虚拟模型子模型的启用状态开关已收编到 `crate::availability`
// （可用性状态机，ADR-0003），接口路由、转发链路与定时任务共用同一组动作入口。
// 「订阅制是否可用」判定已收敛到 `UsageData::subscription_usable`（src/usage/types.rs），
// 用量门控与 LB 选路共用同一口径。

/// 用量额度自动停用/恢复。
///
/// - 订阅制（billing_mode=1）：按 `subscription_usable` 判定（任一已提供窗口剩余为 0 即不可用）。
/// - 按量付费（billing_mode=0）：按 `balance_usable` 判定（查得到余额且合计为 0 即不可用）。
///
/// 无法判定（None）或未开启用量查询的供应商不做任何动作；抓取失败/无数据的场景由调用方保证不传入。
/// 状态迁移的守卫（额度刷新只解除 quota 态，manual/failure 不受触碰）由 availability 模块保证。
pub async fn apply_usage_gate(
    db: &DatabaseConnection,
    p: &provider::Model,
    data: &UsageData,
) -> Result<(), DbErr> {
    let Some(usable) = data.usable_for_billing_mode(p.billing_mode) else {
        return Ok(());
    };
    let label = if p.billing_mode == 1 {
        "订阅额度"
    } else {
        "余额"
    };
    if usable {
        crate::availability::recover_quota(db, p.id, &p.name, label).await?;
    } else {
        crate::availability::disable_for_quota(db, p.id, &p.name, label).await?;
    }
    Ok(())
}

/// 边界探活的候选过滤（纯判定，无序读 DB）：订阅制 + 可探活态（可用或 quota
/// 停用；manual/failure 停用无权解除）+ 已开启用量展示。
pub(crate) fn boundary_probe_eligible(p: &provider::Model) -> bool {
    p.billing_mode == 1
        && matches!(
            p.disabled_reason
                .as_deref()
                .and_then(crate::availability::DisabledReason::parse),
            None | Some(crate::availability::DisabledReason::Quota)
        )
        && crate::usage::usage_enabled(&p.extra)
}

/// 边界探活是否适用（纯判定）：订阅额度可用（非耗尽）且任一已提供窗口剩余
/// 落在 (0,1) 边界区。
pub(crate) fn boundary_probe_applicable(data: &UsageData) -> bool {
    data.subscription_usable() == Some(true) && data.has_low_remaining_window()
}

/// 订阅制边界探活：读取新鲜的用量数据库缓存，对处于边界区（任一已提供窗口
/// 剩余百分比落在 (0, 1)）的订阅制供应商发最小测试请求（`proxy::probe_provider`，
/// 同模型弹窗/失败恢复探测入口）——
/// - 成功 → 解除 quota 停用（幂等；可用态无操作），探活同时充当恢复探测；
/// - 失败 → 按订阅额度耗尽停用（quota 标记 + 级联停用虚拟模型子模型）。
///
/// manual/failure 停用态与未开启用量查询的供应商不探活（本机制无权解除，
/// 探活无意义）。返回探活供应商数，单家失败仅记录日志不中断整体。
/// 由 usage_refresh 在全量刷新落库后调用（缓存必新鲜）；即使本轮刷新全失败，
/// 10 分钟内的旧缓存仍可判定，与 LB 排序/门控同新鲜度口径。
pub async fn probe_boundary_providers(state: &crate::state::AppState) -> Result<usize, DbErr> {
    let providers = provider::Entity::find().all(&state.db).await?;
    // 候选过滤（纯判定）：订阅制 + 可探活态（可用或 quota 停用）+ 用量开启
    // + 缓存新鲜且处边界区（任一已提供窗口剩余落在 (0,1)）。
    let mut candidates = Vec::new();
    for p in providers {
        if !boundary_probe_eligible(&p) {
            continue;
        }
        let Some(data) = read_usage_cache(&state.db, p.id).await? else {
            continue; // 无缓存（抓取从未成功落库）→ 无从判定，本轮跳过
        };
        if !boundary_probe_applicable(&data) {
            continue;
        }
        candidates.push(p);
    }
    // 并发探活（信号量上限，同 refresh_all_usage 形态）：逐家顺序执行时多家
    // 同时处边界会把整轮拖过 5 分钟周期（单家最坏 ~260s，04 票超时形态）。
    let semaphore = Arc::new(tokio::sync::Semaphore::new(4));
    let mut set = tokio::task::JoinSet::new();
    for p in candidates {
        let state = state.clone();
        let semaphore = semaphore.clone();
        set.spawn(async move {
            let _permit = semaphore.acquire().await.expect("边界探活信号量未关闭");
            probe_one_boundary(&state, &p).await
        });
    }
    let mut probed = 0;
    while let Some(outcome) = set.join_next().await {
        match outcome {
            Ok(counted) => probed += counted,
            Err(e) => tracing::warn!("边界探活任务异常：{e}"),
        }
    }
    Ok(probed)
}

/// 单家边界探活：成功恢复 / 失败按额度耗尽停用 / 跳过仅记日志。
/// 返回 1 表示真实发起了探活（成功或失败），0 表示跳过。
async fn probe_one_boundary(state: &crate::state::AppState, p: &provider::Model) -> usize {
    match crate::proxy::probe_provider(state, p).await {
        Ok(duration_ms) => {
            tracing::info!(
                provider_id = p.id,
                provider_name = &p.name,
                "供应商「{}」边界探活成功（{}ms），额度可用",
                p.name,
                duration_ms
            );
            if let Err(e) =
                crate::availability::recover_quota(&state.db, p.id, &p.name, "订阅额度").await
            {
                tracing::warn!(
                    provider_id = p.id,
                    provider_name = &p.name,
                    "供应商「{}」边界探活恢复执行失败：{e}",
                    p.name
                );
            }
        }
        Err(crate::proxy::ProbeFailure::Failed(reason)) => {
            tracing::warn!(
                provider_id = p.id,
                provider_name = &p.name,
                "供应商「{}」边界探活失败（{reason}），按订阅额度耗尽自动停用",
                p.name
            );
            if let Err(e) =
                crate::availability::disable_for_quota(&state.db, p.id, &p.name, "订阅额度").await
            {
                tracing::warn!(
                    provider_id = p.id,
                    provider_name = &p.name,
                    "供应商「{}」边界探活停用执行失败：{e}",
                    p.name
                );
            }
        }
        Err(crate::proxy::ProbeFailure::Skipped(reason)) => {
            tracing::debug!(
                provider_id = p.id,
                provider_name = &p.name,
                "供应商「{}」边界探活跳过：{reason}",
                p.name
            );
            return 0;
        }
    }
    1
}

// 用量内存缓存（LB 选路热路径，P3）已拆至 `usage::mem_cache`（UsageMemCache：
// 10 分钟新鲜 + 单飞抓取去重），与本模块共享 cache_age_fresh* 新鲜度判定。

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::provider_model;
    use crate::entity::virtual_model;
    use crate::entity::virtual_model_item;
    use crate::usage::types::{
        BalanceItem, QuotaWindow, UsageData, UsageKind, WindowKind, empty_windows, set_window,
    };
    use sea_orm::ActiveModelTrait;

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

    /// 07-05：边界探活候选过滤矩阵（纯判定，不看 DB）。
    #[test]
    fn boundary_probe_eligibility_matrix() {
        let mk = |billing_mode: i32, reason: Option<&str>, extra: &str| provider::Model {
            id: 1,
            name: "p".to_string(),
            enable: true,
            base_url: "https://a.example".to_string(),
            api_key: "enc".to_string(),
            custom_header: "{}".to_string(),
            protocol_type: 0,
            billing_mode,
            extra: extra.to_string(),
            sort_order: 0,
            proxy_enabled: false,
            proxy_addr: String::new(),
            disabled_reason: reason.map(str::to_string),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let usage_on = r#"{"usage": true}"#;
        let usage_off = r#"{"usage": false}"#;

        // 订阅制 + 可用/停用 quota + 用量开启 → 可探活。
        assert!(boundary_probe_eligible(&mk(1, None, usage_on)));
        assert!(boundary_probe_eligible(&mk(1, Some("quota"), usage_on)));
        // 其余组合全部排除。
        assert!(
            !boundary_probe_eligible(&mk(0, None, usage_on)),
            "按量不探活"
        );
        assert!(
            !boundary_probe_eligible(&mk(1, Some("manual"), usage_on)),
            "manual 停用不探活"
        );
        assert!(
            !boundary_probe_eligible(&mk(1, Some("failure"), usage_on)),
            "failure 停用不探活"
        );
        assert!(
            !boundary_probe_eligible(&mk(1, None, usage_off)),
            "用量未开启"
        );
        assert!(
            !boundary_probe_eligible(&mk(1, None, "{}")),
            "usage 键缺失按未开启"
        );
    }

    /// 07-05：边界探活适用性——额度可用且处边界区才探活。
    #[test]
    fn boundary_probe_applicability_matrix() {
        let with_pct = |pct: f64| {
            let mut data = balance_data(1, &[]);
            data.kind = UsageKind::Quota;
            data.windows = vec![QuotaWindow::from_remaining_percent(
                WindowKind::FiveHour,
                pct,
                None,
            )];
            data
        };
        assert!(boundary_probe_applicable(&with_pct(0.5)), "边界区应探活");
        assert!(!boundary_probe_applicable(&with_pct(0.0)), "耗尽不可用");
        assert!(
            !boundary_probe_applicable(&with_pct(50.0)),
            "远离边界不探活"
        );
        // 无窗口数据（无法判定）不探活。
        let no_windows = balance_data(1, &[]);
        assert!(!boundary_probe_applicable(&no_windows));
    }

    /// 07-04：批量读数据库缓存——新鲜命中、过期与其他家缺失都不计入。
    #[tokio::test]
    async fn read_usage_cache_many_filters_stale_and_missing() {
        let db = sea_orm::Database::connect("sqlite::memory:").await.unwrap();
        crate::db::migrate(&db).await.unwrap();

        // 新鲜、过期、未写三种形态。
        write_usage_cache(&db, &balance_data(1, &[10.0]))
            .await
            .unwrap();
        let mut stale = balance_data(2, &[20.0]);
        stale.fetched_at = Utc::now() - chrono::TimeDelta::minutes(11);
        write_usage_cache(&db, &stale).await.unwrap();

        let map = read_usage_cache_many(&db, &[1, 2, 3]).await.unwrap();
        assert!(map.contains_key(&1), "新鲜缓存应返回");
        assert!(!map.contains_key(&2), "过期缓存不计入");
        assert!(!map.contains_key(&3), "未写缓存不计入");
        assert_eq!(map.len(), 1);

        let empty = read_usage_cache_many(&db, &[]).await.unwrap();
        assert!(empty.is_empty(), "空入参短路");
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
    async fn concurrent_usage_cache_writes_upsert_without_unique_violation() {
        let db = crate::db::connect("sqlite::memory:").await.unwrap();
        // 同一 provider 并发写缓存（模拟 usage_refresh 与 ?refresh=1 同时命中）：
        // 单语句 ON CONFLICT upsert 无两段竞态，全部成功且只留一行。
        let mut handles = Vec::new();
        for i in 0..4 {
            let db = db.clone();
            handles.push(tokio::spawn(async move {
                let mut data = balance_data(99, &[10.0 + i as f64]);
                data.fetched_at = Utc::now();
                write_usage_cache(&db, &data).await.unwrap();
            }));
        }
        for handle in handles {
            handle.await.unwrap();
        }

        let rows = usage_cache::Entity::find().all(&db).await.unwrap();
        assert_eq!(rows.len(), 1, "并发 upsert 后应只有一行");
        assert_eq!(rows[0].provider_id, 99);
        assert!(read_usage_cache(&db, 99).await.unwrap().is_some());
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

    /// 刷新失败日志点名供应商（消息文本带「供应商「{name}」」，任务日志 UI
    /// 只渲染 message，结构化字段不可见）。用不认识的 host 使抓取确定性失败，
    /// 不触发真实网络请求。
    #[tokio::test(flavor = "current_thread")]
    #[allow(clippy::await_holding_lock)]
    async fn refresh_failure_logs_name_the_provider() {
        use crate::cron::log_capture::{JobLogEvent, JobLogLayer, SUBSCRIBER_LOCK};
        use tokio::sync::broadcast;
        use tracing::Instrument;
        use tracing_subscriber::Registry;
        use tracing_subscriber::layer::SubscriberExt;

        let _lock = SUBSCRIBER_LOCK.lock().unwrap();
        let (log_tx, mut log_rx) = broadcast::channel::<Arc<JobLogEvent>>(8192);
        let keep_alive = log_tx.clone();
        let subscriber = Registry::default().with(JobLogLayer::new(log_tx));
        let _guard = tracing::subscriber::set_default(subscriber);

        // 单连接内存库：多连接池的内存库每连接独立，插入与刷新查询会互相不可见。
        let db = sea_orm::Database::connect("sqlite::memory:").await.unwrap();
        crate::db::migrate(&db).await.unwrap();
        let now = Utc::now();
        let _p = provider::ActiveModel {
            name: Set("失败供应商".to_string()),
            enable: Set(true),
            base_url: Set("https://no-such-host.invalid/v1".to_string()),
            api_key: Set(crate::crypto::encrypt("sk-x")),
            custom_header: Set("{}".to_string()),
            protocol_type: Set(0),
            billing_mode: Set(1),
            extra: Set(r#"{"usage": true, "usage_type": 1}"#.to_string()),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let span = tracing::info_span!(
            target: "cron_job_log",
            "cron_job_run",
            job_name = "usage_refresh",
            run_id = "run-1",
        );
        let refreshed = refresh_all_usage(&db, &Default::default())
            .instrument(span)
            .await
            .unwrap();
        assert_eq!(refreshed, 0, "唯一目标供应商抓取失败，成功数应为 0");

        let mut messages = Vec::new();
        while let Ok(event) = log_rx.try_recv() {
            if let Some(m) = event.message.clone() {
                messages.push(m);
            }
        }
        assert!(
            messages
                .iter()
                .any(|m| m.contains("供应商「失败供应商」用量刷新失败")),
            "刷新失败日志未点名供应商: {messages:?}"
        );
        drop(keep_alive);
    }
}

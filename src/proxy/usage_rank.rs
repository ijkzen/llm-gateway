//! 用量感知排序的纯比较器（供 `order_members` 使用，独立成模块便于单元测试）。
//!
//! 订阅制（quota）：截止时间优先（FEFO，先过期先出）——从最短窗口层（5 小时）起
//! 逐层检查，双方该层都有额度（窗口可用且剩余 > 0）时跳过剩余百分比，改比更上层
//! （期限更长的窗口）的截止时间链：早重置者优先、一方缺截止时间则另一方优先、都缺
//! 则继续往更上层；截止链全平回退剩余百分比逐层比较（现有口径）。该层某方窗口
//! 不可用/无额度则判平进入下一层。全部平局返回 Equal（调用方 shuffle 后稳定排序
//! 实现“同等条件随机选一个”）。按量付费（balance）：按各供应商主余额字段（fetcher
//! 标记的 primary 条目）降序。

use std::cmp::Ordering;

use crate::usage::types::{UsageData, WindowKind};

/// 订阅制窗口层序：从最短滚动窗口到最长。
const QUOTA_LAYERS: [WindowKind; 4] = [
    WindowKind::FiveHour,
    WindowKind::Daily,
    WindowKind::Weekly,
    WindowKind::Monthly,
];

/// 比较两个供应商的订阅制剩余用量（返回 Greater = a 排在 b 前面）。
/// `None` 表示无用量数据，排在任何有数据的后面。
pub fn cmp_quota_deadline_priority(a: Option<&UsageData>, b: Option<&UsageData>) -> Ordering {
    match (a, b) {
        (Some(x), Some(y)) => {
            for (i, kind) in QUOTA_LAYERS.iter().enumerate() {
                if !(has_quota(x, *kind) && has_quota(y, *kind)) {
                    // 某方该层无额度（窗口不可用或剩余为 0）→ 判平，进入下一层。
                    continue;
                }
                // 双方该层都有额度：不比较剩余百分比，改比更上层的截止时间，
                // 截止更近者优先（其额度先到期，先消耗避免被重置浪费）。
                for upper in &QUOTA_LAYERS[i + 1..] {
                    match cmp_deadline(x, y, *upper) {
                        Ordering::Equal => continue,
                        ord => return ord,
                    }
                }
                // 截止链全平（上层无窗口数据或截止全同/全缺）：回退剩余百分比。
                return cmp_remaining_percent(x, y);
            }
            Ordering::Equal
        }
        (Some(_), None) => Ordering::Greater,
        (None, Some(_)) => Ordering::Less,
        (None, None) => Ordering::Equal,
    }
}

/// 该层是否有可用额度：该层窗口存在、剩余可推导且 > 0。多窗口取最差剩余口径
/// （与额度门控一致：任一容器耗尽即视为该层无额度）；窗口扫描实现收敛在
/// `UsageData::worst_window`（见 usage/types.rs），排序与判定共用同一访问器。
fn has_quota(data: &UsageData, kind: WindowKind) -> bool {
    data.worst_window(kind)
        .and_then(|w| w.remaining_percent_value())
        .is_some_and(|p| p > 0.0)
}

/// 某层截止时间比较：早重置者优先；一方有截止时间另一方缺失（窗口不可用或
/// 无 resets_at）→ 有者可判定者优先；都缺失判平（由调用方继续往更上层比较）。
fn cmp_deadline(x: &UsageData, y: &UsageData, kind: WindowKind) -> Ordering {
    match (
        x.worst_window(kind).and_then(|w| w.resets_at),
        y.worst_window(kind).and_then(|w| w.resets_at),
    ) {
        (Some(xr), Some(yr)) => yr.cmp(&xr),
        (Some(_), None) => Ordering::Greater,
        (None, Some(_)) => Ordering::Less,
        (None, None) => Ordering::Equal,
    }
}

/// 剩余百分比兜底（截止链全平时）：5h→日→周→月 逐层比较，即旧版订阅制口径。
fn cmp_remaining_percent(x: &UsageData, y: &UsageData) -> Ordering {
    for kind in QUOTA_LAYERS {
        match cmp_window(x, y, kind) {
            Ordering::Equal => continue,
            ord => return ord,
        }
    }
    Ordering::Equal
}

fn cmp_window(x: &UsageData, y: &UsageData, kind: WindowKind) -> Ordering {
    // 缺失/不可用的窗口视为平局（进入下一层比较），而不是判负：
    // 厂商不提供某窗口不代表该供应商更差（如 Kimi 无月窗）。
    let xw = x.worst_window(kind);
    let yw = y.worst_window(kind);
    let (Some(xw), Some(yw)) = (xw, yw) else {
        return Ordering::Equal;
    };
    let ord = xw
        .remaining_percent_value()
        .partial_cmp(&yw.remaining_percent_value())
        .unwrap_or(Ordering::Equal);
    if ord != Ordering::Equal {
        return ord;
    }
    // 同层剩余打平：重置时间早的优先（先消耗即将重置的余量，避免被重置覆盖浪费）。
    // 任一侧缺失重置时间则视为平局，交给下一层。
    match (xw.resets_at, yw.resets_at) {
        (Some(xr), Some(yr)) => yr.cmp(&xr),
        _ => Ordering::Equal,
    }
}

/// 按量付费比较用金额（fetcher 标记的主余额字段）；非 balance 形态或无数据返回 0.0。
pub fn balance_amount(data: Option<&UsageData>) -> f64 {
    data.and_then(UsageData::primary_balance).unwrap_or(0.0)
}

/// 比较两个按量付费供应商的主余额金额（a 比 b 的金额多少），金额多的为 Greater。
pub fn cmp_balance(a: Option<&UsageData>, b: Option<&UsageData>) -> Ordering {
    balance_amount(a)
        .partial_cmp(&balance_amount(b))
        .unwrap_or(Ordering::Equal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::types::{UsageData, UsageKind};

    fn balance(provider_id: i32, amounts: &[f64]) -> Option<UsageData> {
        Some(UsageData {
            provider_id,
            fetched_at: chrono::Utc::now(),
            kind: UsageKind::Balance,
            plan: None,
            windows: vec![],
            balances: amounts
                .iter()
                .enumerate()
                .map(|(i, a)| crate::usage::types::BalanceItem {
                    label: if i == 0 { "余额" } else { "其他" }.to_string(),
                    amount: *a,
                    currency: None,
                    primary: i == 0,
                })
                .collect(),
        })
    }

    fn window(kind: WindowKind, remaining: f64) -> crate::usage::types::QuotaWindow {
        crate::usage::types::QuotaWindow::from_remaining_percent(kind, remaining, None)
    }

    fn window_reset_at(
        kind: WindowKind,
        remaining: f64,
        resets_at: chrono::DateTime<chrono::Utc>,
    ) -> crate::usage::types::QuotaWindow {
        crate::usage::types::QuotaWindow::from_remaining_percent(kind, remaining, Some(resets_at))
    }

    fn quota(
        provider_id: i32,
        five_hour: Option<f64>,
        weekly: Option<f64>,
        monthly: Option<f64>,
    ) -> Option<UsageData> {
        let mut windows = Vec::new();
        for (kind, val) in [
            (WindowKind::FiveHour, five_hour),
            (WindowKind::Weekly, weekly),
            (WindowKind::Monthly, monthly),
        ] {
            windows.push(match val {
                Some(p) => window(kind, p),
                None => crate::usage::types::QuotaWindow::unavailable(kind),
            });
        }
        quota_from_windows(provider_id, windows)
    }

    fn quota_from_windows(
        provider_id: i32,
        windows: Vec<crate::usage::types::QuotaWindow>,
    ) -> Option<UsageData> {
        Some(UsageData {
            provider_id,
            fetched_at: chrono::Utc::now(),
            kind: UsageKind::Quota,
            plan: None,
            windows,
            balances: vec![],
        })
    }

    // ── 截止时间优先（新语义） ──

    #[test]
    fn deadline_decides_over_remaining_percent() {
        use chrono::Duration;
        let soon = chrono::Utc::now() + Duration::hours(2);
        let later = chrono::Utc::now() + Duration::hours(2 * 24);
        // 双方 5h 层都有额度：B 5h 剩余更低（40% < 60%）但周截止更近 → B 优先。
        let a = quota_from_windows(
            1,
            vec![
                window(WindowKind::FiveHour, 60.0),
                window_reset_at(WindowKind::Weekly, 50.0, later),
                crate::usage::types::QuotaWindow::unavailable(WindowKind::Monthly),
            ],
        );
        let b = quota_from_windows(
            2,
            vec![
                window(WindowKind::FiveHour, 40.0),
                window_reset_at(WindowKind::Weekly, 50.0, soon),
                crate::usage::types::QuotaWindow::unavailable(WindowKind::Monthly),
            ],
        );
        assert_eq!(
            cmp_quota_deadline_priority(b.as_ref(), a.as_ref()),
            Ordering::Greater
        );
        assert_eq!(
            cmp_quota_deadline_priority(a.as_ref(), b.as_ref()),
            Ordering::Less
        );
    }

    #[test]
    fn weekly_deadline_tie_falls_to_monthly() {
        use chrono::Duration;
        let now = chrono::Utc::now();
        // 周截止相同（3 天后）；月截止近者（A 10 天 vs B 20 天）优先。
        let a = quota_from_windows(
            1,
            vec![
                window(WindowKind::FiveHour, 50.0),
                window_reset_at(WindowKind::Weekly, 50.0, now + Duration::days(3)),
                window_reset_at(WindowKind::Monthly, 50.0, now + Duration::days(10)),
            ],
        );
        let b = quota_from_windows(
            2,
            vec![
                window(WindowKind::FiveHour, 50.0),
                window_reset_at(WindowKind::Weekly, 50.0, now + Duration::days(3)),
                window_reset_at(WindowKind::Monthly, 50.0, now + Duration::days(20)),
            ],
        );
        assert_eq!(
            cmp_quota_deadline_priority(a.as_ref(), b.as_ref()),
            Ordering::Greater
        );
        assert_eq!(
            cmp_quota_deadline_priority(b.as_ref(), a.as_ref()),
            Ordering::Less
        );
    }

    #[test]
    fn missing_deadline_ranks_after_known() {
        use chrono::Duration;
        // 5h 都有额度；A 周窗无截止时间、B 周窗有（且更晚）→ B 仍优先（可判定者优先）。
        let b_weekly_reset = chrono::Utc::now() + Duration::hours(2 * 24);
        let a = quota_from_windows(
            1,
            vec![
                window(WindowKind::FiveHour, 50.0),
                window(WindowKind::Weekly, 50.0),
                crate::usage::types::QuotaWindow::unavailable(WindowKind::Monthly),
            ],
        );
        let b = quota_from_windows(
            2,
            vec![
                window(WindowKind::FiveHour, 50.0),
                window_reset_at(WindowKind::Weekly, 50.0, b_weekly_reset),
                crate::usage::types::QuotaWindow::unavailable(WindowKind::Monthly),
            ],
        );
        assert_eq!(
            cmp_quota_deadline_priority(b.as_ref(), a.as_ref()),
            Ordering::Greater
        );
    }

    #[test]
    fn deadline_chain_tie_falls_back_to_remaining() {
        // 5h 都有额度但上层无任何截止数据 → 兜底按 5h 剩余决胜。
        let high = quota(1, Some(80.0), None, None);
        let low = quota(2, Some(20.0), None, None);
        assert_eq!(
            cmp_quota_deadline_priority(high.as_ref(), low.as_ref()),
            Ordering::Greater
        );
        assert_eq!(
            cmp_quota_deadline_priority(low.as_ref(), high.as_ref()),
            Ordering::Less
        );
    }

    #[test]
    fn layer_without_quota_defers_to_next() {
        use chrono::Duration;
        // A 无 5h 窗口（只有周窗）、B 有 5h 窗口 → 5h 判平；周层双方都有额度、
        // 周剩余相同 → 兜底比周截止：A 截止更近 → A 优先。
        let a_weekly_reset = chrono::Utc::now() + Duration::hours(2);
        let b_weekly_reset = chrono::Utc::now() + Duration::days(5);
        let a = quota_from_windows(
            1,
            vec![
                crate::usage::types::QuotaWindow::unavailable(WindowKind::FiveHour),
                window_reset_at(WindowKind::Weekly, 50.0, a_weekly_reset),
                crate::usage::types::QuotaWindow::unavailable(WindowKind::Monthly),
            ],
        );
        let b = quota_from_windows(
            2,
            vec![
                window(WindowKind::FiveHour, 50.0),
                window_reset_at(WindowKind::Weekly, 50.0, b_weekly_reset),
                crate::usage::types::QuotaWindow::unavailable(WindowKind::Monthly),
            ],
        );
        assert_eq!(
            cmp_quota_deadline_priority(a.as_ref(), b.as_ref()),
            Ordering::Greater
        );
    }

    #[test]
    fn exhausted_five_hour_defers_to_weekly_remaining() {
        // A 5h 剩余 0（耗尽，展示端才可能出现）→ 5h 层无额度判平；
        // 周层都有额度、截止全缺 → 兜底 5h：A 0 < B 50 → B 优先。
        let a = quota(1, Some(0.0), Some(50.0), None);
        let b = quota(2, Some(50.0), Some(50.0), None);
        assert_eq!(
            cmp_quota_deadline_priority(b.as_ref(), a.as_ref()),
            Ordering::Greater
        );
    }

    #[test]
    fn duplicate_windows_worst_zero_defers_to_next_layer() {
        // 多池（商汤）最差剩余为 0 → 该层视为无额度判平进下一层（与额度门控
        // subscription_usable 口径一致：任一容器耗尽即不可用）。
        let mut a = quota(1, Some(90.0), Some(50.0), None).unwrap();
        a.windows.push(window(WindowKind::FiveHour, 0.0));
        let b = quota(2, Some(50.0), Some(50.0), None);
        assert_eq!(
            cmp_quota_deadline_priority(b.as_ref(), Some(&a)),
            Ordering::Greater
        );
    }

    // ── 回归（无截止数据时兜底口径与旧行为一致） ──

    #[test]
    fn five_hour_window_decides() {
        let high = quota(1, Some(80.0), None, None);
        let low = quota(2, Some(20.0), None, None);
        assert_eq!(
            cmp_quota_deadline_priority(high.as_ref(), low.as_ref()),
            Ordering::Greater
        );
        assert_eq!(
            cmp_quota_deadline_priority(low.as_ref(), high.as_ref()),
            Ordering::Less
        );
    }

    #[test]
    fn daily_window_decides_before_weekly() {
        let daily_high = quota_from_windows(
            1,
            vec![
                window(WindowKind::Daily, 80.0),
                window(WindowKind::Weekly, 10.0),
            ],
        );
        let daily_low = quota_from_windows(
            2,
            vec![
                window(WindowKind::Daily, 20.0),
                window(WindowKind::Weekly, 90.0),
            ],
        );
        assert_eq!(
            cmp_quota_deadline_priority(daily_high.as_ref(), daily_low.as_ref()),
            Ordering::Greater
        );
    }

    #[test]
    fn tie_on_five_hour_falls_to_weekly() {
        let a = quota(1, Some(50.0), Some(70.0), None);
        let b = quota(2, Some(50.0), Some(30.0), None);
        assert_eq!(
            cmp_quota_deadline_priority(a.as_ref(), b.as_ref()),
            Ordering::Greater
        );
    }

    #[test]
    fn tie_on_all_windows_is_equal() {
        let a = quota(1, Some(50.0), Some(50.0), Some(50.0));
        let b = quota(2, Some(50.0), Some(50.0), Some(50.0));
        assert_eq!(
            cmp_quota_deadline_priority(a.as_ref(), b.as_ref()),
            Ordering::Equal
        );
    }

    #[test]
    fn duplicate_windows_take_worst_remaining() {
        // 多池（商汤）同类窗口多条：取最差剩余参与比较。
        let mut multi = quota(1, Some(90.0), None, None).unwrap();
        multi.windows.push(window(WindowKind::FiveHour, 5.0));
        let plain = quota(2, Some(50.0), None, None);
        assert_eq!(
            cmp_quota_deadline_priority(Some(&multi), plain.as_ref()),
            Ordering::Less
        );
    }

    #[test]
    fn missing_window_defers_to_next() {
        // a 无 5h 窗口（提供 weekly），b 有 5h 窗口 → 5h 判平（缺数据持平）→ 周决胜。
        let a = quota(1, None, Some(80.0), None);
        let b = quota(2, Some(50.0), Some(10.0), None);
        assert_eq!(
            cmp_quota_deadline_priority(a.as_ref(), b.as_ref()),
            Ordering::Greater
        );
    }

    #[test]
    fn earlier_reset_wins_on_remaining_tie() {
        use chrono::Duration;
        let soon = chrono::Utc::now() + Duration::hours(1);
        let later = chrono::Utc::now() + Duration::hours(4);
        let a = quota_from_windows(1, vec![window_reset_at(WindowKind::FiveHour, 50.0, soon)]);
        let b = quota_from_windows(2, vec![window_reset_at(WindowKind::FiveHour, 50.0, later)]);
        assert_eq!(
            cmp_quota_deadline_priority(a.as_ref(), b.as_ref()),
            Ordering::Greater
        );
        assert_eq!(
            cmp_quota_deadline_priority(b.as_ref(), a.as_ref()),
            Ordering::Less
        );
    }

    #[test]
    fn missing_reset_defers_to_next_layer() {
        use chrono::Duration;
        let later = chrono::Utc::now() + Duration::hours(4);
        // a 的 5h 窗口无重置时间 → 5h 层打平 → 周层决胜（a 70% > b 10%）。
        let a = quota(1, Some(50.0), Some(70.0), None);
        let b = quota_from_windows(
            2,
            vec![
                window_reset_at(WindowKind::FiveHour, 50.0, later),
                window(WindowKind::Weekly, 10.0),
                crate::usage::types::QuotaWindow::unavailable(WindowKind::Monthly),
            ],
        );
        assert_eq!(
            cmp_quota_deadline_priority(a.as_ref(), b.as_ref()),
            Ordering::Greater
        );
    }

    #[test]
    fn provider_without_data_ranks_last() {
        let d = quota(1, Some(50.0), None, None);
        assert_eq!(
            cmp_quota_deadline_priority(d.as_ref(), None),
            Ordering::Greater
        );
        assert_eq!(
            cmp_quota_deadline_priority(None, d.as_ref()),
            Ordering::Less
        );
        assert_eq!(cmp_quota_deadline_priority(None, None), Ordering::Equal);
    }

    #[test]
    fn balance_ranks_by_total_amount_desc() {
        let rich = balance(1, &[110.0, 5.5]);
        let poor = balance(2, &[10.0]);
        assert_eq!(cmp_balance(rich.as_ref(), poor.as_ref()), Ordering::Greater);
        assert_eq!(cmp_balance(poor.as_ref(), rich.as_ref()), Ordering::Less);
        assert_eq!(cmp_balance(rich.as_ref(), None), Ordering::Greater);
        assert_eq!(cmp_balance(None, None), Ordering::Equal);
    }

    #[test]
    fn balance_compares_primary_item_only() {
        // 只比 primary 条目，不做合计：a 的主字段 10 反而比 b 的主字段 50 小，
        // 即使 a 另有一条 1000 的非主条目。
        let a = balance(1, &[10.0, 1000.0]);
        let b = balance(2, &[50.0]);
        assert_eq!(cmp_balance(a.as_ref(), b.as_ref()), Ordering::Less);
        // 旧缓存数据无 primary 标记：回退取第一条。
        let mut legacy = a.unwrap();
        for item in &mut legacy.balances {
            item.primary = false;
        }
        assert_eq!(balance_amount(Some(&legacy)), 10.0);
    }

    // ── 与额度门控口径一致性（防漂移回归） ──

    /// 排序的逐层判定与 `subscription_usable` 对同一窗口矩阵给出一致结论：
    /// 「全局可用」⇔「每一可推导窗口层都有额度」。available 但剩余无法推导的
    /// 窗口（如 used/limit 均缺）不计入判定，两种口径对它一致地保持中立。
    #[test]
    fn layer_quota_verdict_consistent_with_subscription_usable() {
        // weekly 为 available 但无法推导（无 used/limit/percent 字段）。
        let underivable = crate::usage::types::QuotaWindow {
            window: WindowKind::Weekly,
            available: true,
            used_percent: None,
            remaining_percent: None,
            resets_at: None,
            used: None,
            limit: None,
            unit: None,
            label: None,
        };
        let mut with_underivable = quota(5, Some(80.0), None, None).unwrap();
        with_underivable.windows.push(underivable);

        let fixtures: Vec<Option<UsageData>> = vec![
            quota(1, Some(50.0), Some(50.0), Some(50.0)),
            quota(2, Some(0.0), Some(50.0), Some(50.0)),
            quota(3, Some(50.0), Some(0.0), None),
            quota(4, Some(0.0), Some(0.0), Some(0.0)),
            quota(5, Some(80.0), None, None),
            quota(6, None, Some(20.0), None),
            quota(7, Some(0.0), None, None),
            Some(with_underivable),
        ];
        for data in fixtures {
            let data = data.unwrap();
            // 只把「能推导出剩余」的窗口层纳入对照：全局判定对无法推导的窗口中立。
            let derivable_layers_all_have_quota = QUOTA_LAYERS
                .iter()
                .filter(|kind| data.worst_window(**kind).is_some())
                .all(|kind| has_quota(&data, *kind));
            assert_eq!(
                data.subscription_usable() == Some(true),
                derivable_layers_all_have_quota,
                "provider {} 逐层判定与全局可用不一致",
                data.provider_id
            );
        }
    }

    /// 商汤式多池（同类窗口多条、其一耗尽）在排序层与全局判定都视为无额度/不可用。
    #[test]
    fn duplicate_pool_worst_consistent_with_usable() {
        let mut multi = quota(1, Some(90.0), Some(50.0), None).unwrap();
        multi.windows.push(window(WindowKind::FiveHour, 0.0));
        assert_eq!(multi.subscription_usable(), Some(false));
        assert!(!has_quota(&multi, WindowKind::FiveHour));
        assert!(has_quota(&multi, WindowKind::Weekly));
    }
}

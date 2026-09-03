//! 用量感知排序的纯比较器（供 `order_members` 使用，独立成模块便于单元测试）。
//!
//! 订阅制（quota）：按 5 小时 → 周 → 月 的剩余百分比逐层比较，缺失/不可用的
//! 窗口视为平局交给下一层；同层剩余打平再比该层重置时间，早的优先（先消耗
//! 即将重置的余量，避免被重置覆盖浪费）。全部平局返回 Equal（调用方 shuffle
//! 后稳定排序实现“同等条件随机选一个”）。按量付费（balance）：按各供应商
//! 主余额字段（fetcher 标记的 primary 条目）降序。

use std::cmp::Ordering;

use crate::usage::types::{QuotaWindow, UsageData, WindowKind};

/// 比较两个供应商的订阅制剩余用量（降序：剩余多的排前面）。
/// `None` 表示无用量数据，排在任何有数据的后面。
pub fn cmp_quota_remaining(a: Option<&UsageData>, b: Option<&UsageData>) -> Ordering {
    match (a, b) {
        (Some(x), Some(y)) => {
            for kind in [
                WindowKind::FiveHour,
                WindowKind::Daily,
                WindowKind::Weekly,
                WindowKind::Monthly,
            ] {
                match cmp_window(x, y, kind) {
                    Ordering::Equal => continue,
                    ord => return ord,
                }
            }
            Ordering::Equal
        }
        (Some(_), None) => Ordering::Greater,
        (None, Some(_)) => Ordering::Less,
        (None, None) => Ordering::Equal,
    }
}

fn cmp_window(x: &UsageData, y: &UsageData, kind: WindowKind) -> Ordering {
    // 缺失/不可用的窗口视为平局（进入下一层比较），而不是判负：
    // 厂商不提供某窗口不代表该供应商更差（如 Kimi 无月窗）。
    let xw = worst_window(x, kind);
    let yw = worst_window(y, kind);
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

/// 同类窗口可能有多条（如商汤各积分池独立产出），取最差剩余的那条参与比较：
/// 任一容器耗尽即接近不可用，与额度门控的逐窗口判定口径一致。
fn worst_window(data: &UsageData, kind: WindowKind) -> Option<&QuotaWindow> {
    data.windows
        .iter()
        .filter(|w| w.window == kind)
        .filter_map(|w| w.remaining_percent_value().map(|p| (p, w)))
        .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(Ordering::Equal))
        .map(|(_, w)| w)
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
    use crate::usage::types::{QuotaWindow, UsageData, UsageKind};

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
        QuotaWindow::from_remaining_percent(kind, remaining, None)
    }

    fn window_reset_at(
        kind: WindowKind,
        remaining: f64,
        resets_at: chrono::DateTime<chrono::Utc>,
    ) -> crate::usage::types::QuotaWindow {
        QuotaWindow::from_remaining_percent(kind, remaining, Some(resets_at))
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

    #[test]
    fn five_hour_window_decides() {
        let high = quota(1, Some(80.0), None, None);
        let low = quota(2, Some(20.0), None, None);
        assert_eq!(
            cmp_quota_remaining(high.as_ref(), low.as_ref()),
            Ordering::Greater
        );
        assert_eq!(
            cmp_quota_remaining(low.as_ref(), high.as_ref()),
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
            cmp_quota_remaining(daily_high.as_ref(), daily_low.as_ref()),
            Ordering::Greater
        );
    }

    #[test]
    fn tie_on_five_hour_falls_to_weekly() {
        let a = quota(1, Some(50.0), Some(70.0), None);
        let b = quota(2, Some(50.0), Some(30.0), None);
        assert_eq!(
            cmp_quota_remaining(a.as_ref(), b.as_ref()),
            Ordering::Greater
        );
    }

    #[test]
    fn tie_on_all_windows_is_equal() {
        let a = quota(1, Some(50.0), Some(50.0), Some(50.0));
        let b = quota(2, Some(50.0), Some(50.0), Some(50.0));
        assert_eq!(cmp_quota_remaining(a.as_ref(), b.as_ref()), Ordering::Equal);
    }

    #[test]
    fn duplicate_windows_take_worst_remaining() {
        // 多池（商汤）同类窗口多条：取最差剩余参与比较。
        let mut multi = quota(1, Some(90.0), None, None).unwrap();
        multi.windows.push(window(WindowKind::FiveHour, 5.0));
        let plain = quota(2, Some(50.0), None, None);
        assert_eq!(
            cmp_quota_remaining(Some(&multi), plain.as_ref()),
            Ordering::Less
        );
    }

    #[test]
    fn missing_window_defers_to_next() {
        // a 无 5h 窗口（提供 weekly），b 有 5h 窗口 → 5h 平局（缺数据持平）→ 周决胜。
        let a = quota(1, None, Some(80.0), None);
        let b = quota(2, Some(50.0), Some(10.0), None);
        assert_eq!(
            cmp_quota_remaining(a.as_ref(), b.as_ref()),
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
            cmp_quota_remaining(a.as_ref(), b.as_ref()),
            Ordering::Greater
        );
        assert_eq!(cmp_quota_remaining(b.as_ref(), a.as_ref()), Ordering::Less);
    }

    #[test]
    fn missing_reset_defers_to_next_layer() {
        use chrono::Duration;
        let later = chrono::Utc::now() + Duration::hours(4);
        // a 的 5h 窗口无重置时间 → 5h 层平局 → 周层决胜（a 70% > b 10%）。
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
            cmp_quota_remaining(a.as_ref(), b.as_ref()),
            Ordering::Greater
        );
    }

    #[test]
    fn provider_without_data_ranks_last() {
        let d = quota(1, Some(50.0), None, None);
        assert_eq!(cmp_quota_remaining(d.as_ref(), None), Ordering::Greater);
        assert_eq!(cmp_quota_remaining(None, d.as_ref()), Ordering::Less);
        assert_eq!(cmp_quota_remaining(None, None), Ordering::Equal);
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
}

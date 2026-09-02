//! 用量感知排序的纯比较器（供 `order_members` 使用，独立成模块便于单元测试）。
//!
//! 订阅制（quota）：按 5 小时 → 周 → 月 的剩余百分比逐层比较，缺失/不可用的
//! 窗口视为平局交给下一层，三层全平返回 Equal（调用方 shuffle 后稳定排序实现
//! “同等条件随机选一个”）。按量付费（balance）：按剩余金额合计降序。

use std::cmp::Ordering;

use crate::usage::types::{UsageData, UsageKind, WindowKind};

/// 比较两个供应商的订阅制剩余用量（降序：剩余多的排前面）。
/// `None` 表示无用量数据，排在任何有数据的后面。
pub fn cmp_quota_remaining(a: Option<&UsageData>, b: Option<&UsageData>) -> Ordering {
    match (a, b) {
        (Some(x), Some(y)) => {
            for kind in [
                WindowKind::FiveHour,
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
    let xp = worst_remaining(x, kind);
    let yp = worst_remaining(y, kind);
    match (xp, yp) {
        (Some(a), Some(b)) => a.partial_cmp(&b).unwrap_or(Ordering::Equal),
        _ => Ordering::Equal,
    }
}

/// 同类窗口可能有多条（如商汤各积分池独立产出），取最差剩余参与比较：
/// 任一容器耗尽即接近不可用，与额度门控的逐窗口判定口径一致。
fn worst_remaining(data: &UsageData, kind: WindowKind) -> Option<f64> {
    data.windows
        .iter()
        .filter(|w| w.window == kind)
        .filter_map(|w| w.remaining_percent_value())
        .reduce(|a, b| if a < b { a } else { b })
}

/// 按量付费剩余金额（balances 合计）；非 balance 形态或无数据返回 0.0。
pub fn balance_amount(data: Option<&UsageData>) -> f64 {
    match data {
        Some(d) if d.kind == UsageKind::Balance => d.balances.iter().map(|b| b.amount).sum(),
        _ => 0.0,
    }
}

/// 比较两个按量付费供应商的剩余金额（a 比 b 的金额多少），金额多的为 Greater。
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
                .map(|a| crate::usage::types::BalanceItem {
                    label: "余额".to_string(),
                    amount: *a,
                    currency: None,
                })
                .collect(),
        })
    }

    fn window(kind: WindowKind, remaining: f64) -> crate::usage::types::QuotaWindow {
        QuotaWindow::from_remaining_percent(kind, remaining, None)
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
}

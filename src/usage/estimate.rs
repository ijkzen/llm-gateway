//! 订阅周期 Token 总量预估的纯核心（无 SQL / 无 DB 依赖）。
//!
//! 算术：网关已用 token ÷ 用量卡已用比例 = 整个订阅周期的 token 总量。
//! 「数据完整吗」与「怎么折算」在这里是显式的两个问题：折算比例来自
//! `quota_ratio`，可否预估的信任边界是 `is_estimatable`（固化现状语义：
//! 网关记录 > 0 且比例可折算；按天覆盖检查不参与算术——流量未全走网关时
//! 0 ÷ 比例折算出 0 无意义，故 used=0 一律不可预估）。

use crate::usage::types::{QuotaWindow, WindowKind};

/// 订阅周期窗口长度（毫秒）：周 = 7 天，月 = 30 天（估算口径，与厂商
/// resets_at 反推一致）；其余窗口无法预估。
pub fn period_len_ms(window: WindowKind) -> Option<i64> {
    match window {
        WindowKind::Weekly => Some(7 * 24 * 3_600_000),
        WindowKind::Monthly => Some(30 * 24 * 3_600_000),
        _ => None,
    }
}

/// 折算比例（0~1）：优先 used/limit 绝对值，其次 used_percent 兜底；
/// 无法折算或比例非正（0/负——折算出 0/负没有意义）返回 None。
pub fn quota_ratio(window: &QuotaWindow) -> Option<f64> {
    let ratio = match (window.used, window.limit) {
        (Some(used), Some(limit)) if limit > 0.0 => Some(used / limit),
        _ => window.used_percent.map(|p| p / 100.0),
    };
    ratio.filter(|r| *r > 0.0)
}

/// 信任边界（显式化，语义与 645bca1 后现状一致）：网关记录为 0 却已消耗
/// 配额说明前提不成立（流量未全走网关），视为不可预估。
pub fn is_estimatable(used_tokens: i64, ratio: Option<f64>) -> bool {
    used_tokens > 0 && ratio.is_some()
}

/// 按比例折算整个订阅周期总量（四舍五入取整）；比例缺失返回 None。
pub fn estimated_total(used_tokens: i64, ratio: Option<f64>) -> Option<i64> {
    ratio.map(|r| (used_tokens as f64 / r).round() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::types::QuotaWindow;

    fn used_limit(kind: WindowKind, used: f64, limit: f64) -> QuotaWindow {
        QuotaWindow::from_used_limit(kind, used, limit, None, None)
    }

    fn percent(kind: WindowKind, used_percent: f64) -> QuotaWindow {
        QuotaWindow::from_used_percent(kind, used_percent, None)
    }

    #[test]
    fn period_len_weekly_monthly_only() {
        assert_eq!(period_len_ms(WindowKind::Weekly), Some(7 * 24 * 3_600_000));
        assert_eq!(
            period_len_ms(WindowKind::Monthly),
            Some(30 * 24 * 3_600_000)
        );
        assert_eq!(period_len_ms(WindowKind::FiveHour), None);
        assert_eq!(period_len_ms(WindowKind::Daily), None);
    }

    #[test]
    fn quota_ratio_prefers_used_limit_over_percent() {
        let w = used_limit(WindowKind::Weekly, 20.0, 100.0);
        assert_eq!(quota_ratio(&w), Some(0.2));
        // used_percent 兜底：20% → 0.2。
        let w2 = percent(WindowKind::Weekly, 20.0);
        assert_eq!(quota_ratio(&w2), Some(0.2));
        // used/limit 同时存在时 used_percent 不参与。
        let mut w3 = w.clone();
        w3.used_percent = Some(50.0);
        assert_eq!(quota_ratio(&w3), Some(0.2));
    }

    #[test]
    fn quota_ratio_none_on_underivable_or_non_positive() {
        // limit 为 0：used/limit 不可折算，used_percent 缺失 → None。
        assert_eq!(
            quota_ratio(&used_limit(WindowKind::Weekly, 10.0, 0.0)),
            None
        );
        // 无任何字段（unavailable）→ None。
        assert_eq!(
            quota_ratio(&QuotaWindow::unavailable(WindowKind::Weekly)),
            None
        );
        // used 为 0 → 比例 0，非正过滤 → None。
        assert_eq!(
            quota_ratio(&used_limit(WindowKind::Weekly, 0.0, 100.0)),
            None
        );
    }

    #[test]
    fn estimate_arithmetic_and_trust_gate() {
        // 10 token ÷ 0.2 = 50（round 取整）。
        assert_eq!(estimated_total(10, Some(0.2)), Some(50));
        // 非整除走 round：7 ÷ 0.2 = 35.000…? 用 1÷0.3≈3.33 → 3。
        assert_eq!(estimated_total(1, Some(0.3)), Some(3));
        // 信任边界：used=0 不可预估；比例缺失不可预估；used>0 且比例有 → 可。
        assert!(!is_estimatable(0, Some(0.2)));
        assert!(!is_estimatable(10, None));
        assert!(is_estimatable(10, Some(0.2)));
        // estimated_total 与 is_estimatable 在响应层一致：不可预估时输出 None。
        assert_eq!(estimated_total(0, Some(0.2)), Some(0));
    }
}

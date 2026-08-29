//! 用量查询的统一数据类型（`GET /api/providers/{id}/usage` 的 data 字段）。

use chrono::{DateTime, Utc};
use serde::Serialize;

/// 归一化后的用量数据：订阅制走 `windows`，按量付费走 `balances`。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageData {
    pub provider_id: i32,
    pub fetched_at: DateTime<Utc>,
    pub kind: UsageKind,
    /// 套餐档位（如智谱 level、ZenMux tier），无则缺省。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    /// 订阅制窗口用量。固定输出 5h/周/月 三个元素，
    /// 厂商不提供的窗口 `available=false`（如 Kimi For Coding 无月窗）。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub windows: Vec<QuotaWindow>,
    /// 按量付费余额条目（可多币种/多账户）。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub balances: Vec<BalanceItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageKind {
    Quota,
    Balance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowKind {
    FiveHour,
    Weekly,
    Monthly,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindow {
    pub window: WindowKind,
    /// 厂商是否提供该窗口数据；false 时其余字段均缺省。
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remaining_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

impl QuotaWindow {
    pub fn unavailable(window: WindowKind) -> Self {
        Self {
            window,
            available: false,
            used_percent: None,
            remaining_percent: None,
            resets_at: None,
            used: None,
            limit: None,
            unit: None,
        }
    }

    /// 已知已用百分比。
    pub fn from_used_percent(
        window: WindowKind,
        used_percent: f64,
        resets_at: Option<DateTime<Utc>>,
    ) -> Self {
        let used_percent = clamp_percent(used_percent);
        Self {
            available: true,
            used_percent: Some(used_percent),
            remaining_percent: Some(100.0 - used_percent),
            resets_at,
            ..Self::unavailable(window)
        }
    }

    /// 已知剩余百分比。
    pub fn from_remaining_percent(
        window: WindowKind,
        remaining_percent: f64,
        resets_at: Option<DateTime<Utc>>,
    ) -> Self {
        let remaining_percent = clamp_percent(remaining_percent);
        Self {
            available: true,
            used_percent: Some(100.0 - remaining_percent),
            remaining_percent: Some(remaining_percent),
            resets_at,
            ..Self::unavailable(window)
        }
    }

    /// 已知已用量与总量；总量为 0 时只保留绝对值、不产出百分比。
    pub fn from_used_limit(
        window: WindowKind,
        used: f64,
        limit: f64,
        resets_at: Option<DateTime<Utc>>,
        unit: Option<&str>,
    ) -> Self {
        // 保留两位小数，避免 69.04+0.946=69.98599… 这类浮点噪声。
        let used = round2(used);
        let limit = round2(limit);
        let mut w = Self {
            available: true,
            used: Some(used),
            limit: Some(limit),
            unit: unit.map(str::to_string),
            resets_at,
            ..Self::unavailable(window)
        };
        if limit > 0.0 {
            let used_percent = clamp_percent(used / limit * 100.0);
            w.used_percent = Some(used_percent);
            w.remaining_percent = Some(100.0 - used_percent);
        }
        w
    }
}

/// 生成 5h/周/月 三个不可用窗口，fetcher 按需替换其中元素。
pub fn empty_windows() -> Vec<QuotaWindow> {
    vec![
        QuotaWindow::unavailable(WindowKind::FiveHour),
        QuotaWindow::unavailable(WindowKind::Weekly),
        QuotaWindow::unavailable(WindowKind::Monthly),
    ]
}

/// 将窗口写入三窗数组（按 window 种类定位替换）。
pub fn set_window(windows: &mut [QuotaWindow], w: QuotaWindow) {
    if let Some(slot) = windows.iter_mut().find(|slot| slot.window == w.window) {
        *slot = w;
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BalanceItem {
    pub label: String,
    pub amount: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
}

/// fetcher 的返回：quota（订阅窗口）或 balance（余额条目）。
#[derive(Debug, Clone)]
pub enum FetchOutput {
    Quota {
        plan: Option<String>,
        windows: Vec<QuotaWindow>,
    },
    Balance {
        items: Vec<BalanceItem>,
    },
}

fn clamp_percent(p: f64) -> f64 {
    if p.is_nan() {
        0.0
    } else {
        // 保留两位小数，避免 0.58*100=57.999… 这类浮点噪声进入展示层。
        round2(p.clamp(0.0, 100.0))
    }
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// 毫秒时间戳转 UTC；非法值返回 None。
pub fn ts_ms(ms: i64) -> Option<DateTime<Utc>> {
    DateTime::from_timestamp_millis(ms)
}

/// 秒级时间戳转 UTC；非法值返回 None。
pub fn ts_secs(secs: i64) -> Option<DateTime<Utc>> {
    DateTime::from_timestamp(secs, 0)
}

/// ISO 8601 / RFC 3339 字符串转 UTC；非法值返回 None。
pub fn ts_iso(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn used_percent_derives_remaining() {
        let w = QuotaWindow::from_used_percent(WindowKind::FiveHour, 42.0, None);
        assert!(w.available);
        assert_eq!(w.used_percent, Some(42.0));
        assert_eq!(w.remaining_percent, Some(58.0));
    }

    #[test]
    fn remaining_percent_derives_used() {
        let w = QuotaWindow::from_remaining_percent(WindowKind::Weekly, 80.0, None);
        assert_eq!(w.used_percent, Some(20.0));
        assert_eq!(w.remaining_percent, Some(80.0));
    }

    #[test]
    fn used_limit_zero_limit_has_no_percent() {
        let w = QuotaWindow::from_used_limit(WindowKind::Monthly, 10.0, 0.0, None, None);
        assert!(w.available);
        assert_eq!(w.used_percent, None);
        assert_eq!(w.used, Some(10.0));
    }

    #[test]
    fn percent_is_clamped() {
        let w = QuotaWindow::from_used_percent(WindowKind::FiveHour, 130.0, None);
        assert_eq!(w.used_percent, Some(100.0));
        assert_eq!(w.remaining_percent, Some(0.0));
    }

    #[test]
    fn set_window_replaces_matching_slot() {
        let mut windows = empty_windows();
        assert!(windows.iter().all(|w| !w.available));
        set_window(
            &mut windows,
            QuotaWindow::from_used_percent(WindowKind::Weekly, 10.0, None),
        );
        assert!(!windows[0].available);
        assert!(windows[1].available);
        assert!(!windows[2].available);
    }

    #[test]
    fn serialization_shape() {
        let data = UsageData {
            provider_id: 1,
            fetched_at: Utc::now(),
            kind: UsageKind::Quota,
            plan: Some("pro".to_string()),
            windows: empty_windows(),
            balances: vec![],
        };
        let v = serde_json::to_value(&data).unwrap();
        assert_eq!(v["kind"], "quota");
        assert_eq!(v["windows"][0]["window"], "five_hour");
        assert_eq!(v["windows"][0]["available"], false);
        // 不可用窗口不输出数值字段
        assert!(v["windows"][0].get("usedPercent").is_none());
        // 空 balances 不输出
        assert!(v.get("balances").is_none());
    }
}

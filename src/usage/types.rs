//! 用量查询的统一数据类型（`GET /api/providers/{id}/usage` 的 data 字段）。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::i18n::Lang;

/// 归一化后的用量数据：订阅制走 `windows`，按量付费走 `balances`。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageData {
    pub provider_id: i32,
    pub fetched_at: DateTime<Utc>,
    pub kind: UsageKind,
    /// 套餐档位（如智谱 level、ZenMux tier），无则缺省。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    /// 订阅制窗口用量。通用 fetcher 固定输出 5h/周/月三个元素，
    /// 厂商可额外产出 Daily 或同类多窗口；未提供的固定窗口 `available=false`。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub windows: Vec<QuotaWindow>,
    /// 按量付费余额条目（可多币种/多账户）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub balances: Vec<BalanceItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageKind {
    Quota,
    Balance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowKind {
    FiveHour,
    Daily,
    Weekly,
    Monthly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// 窗口所属容量容器标注（如商汤积分池名）；无标注即厂商整体口径。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl QuotaWindow {
    /// 该窗口的剩余百分比（0-100）：优先取 remaining_percent，其次由
    /// used_percent 推导，最后用 used/limit 直接算；窗口不可用或无法推导时
    /// 返回 None。出口统一四舍五入保留 2 位小数（如 63.499 → 63.5），与
    /// clamp_percent 的 round2 口径一致，前端只负责展示不再取整。供订阅制
    /// 排序（`src/proxy/usage_rank.rs`）与额度耗尽判定（`src/usage/persist.rs`）共用。
    pub fn remaining_percent_value(&self) -> Option<f64> {
        if !self.available {
            return None;
        }
        let value = if let Some(p) = self.remaining_percent {
            p
        } else if let Some(p) = self.used_percent {
            100.0 - p
        } else {
            match (self.used, self.limit) {
                (Some(used), Some(limit)) if limit > 0.0 => {
                    ((limit - used) / limit * 100.0).clamp(0.0, 100.0)
                }
                _ => return None,
            }
        };
        Some(round2(value))
    }

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
            label: None,
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

impl UsageData {
    /// 订阅制「当前是否可用」判定：全部厂商已提供的窗口剩余 > 0 → true；
    /// 任一已提供窗口剩余为 0 → false；无任何可用窗口数据（无法判定）→ None。
    /// 调用方在 None 时必须保持原状，避免上游抖动误伤。
    /// （用量门控 `src/usage/persist.rs` 与 LB 选路 `src/proxy/mod.rs` 共用。）
    pub fn subscription_usable(&self) -> Option<bool> {
        if self.kind != UsageKind::Quota {
            return None;
        }
        let mut saw_available = false;
        for window in &self.windows {
            if let Some(p) = window.remaining_percent_value() {
                saw_available = true;
                if p <= 0.0 {
                    return Some(false);
                }
            }
        }
        saw_available.then_some(true)
    }

    /// LB 比较用的主余额金额：取 fetcher 标记的 primary 条目；旧缓存数据无
    /// 标记时回退取第一条（各 fetcher 的条目顺序本就以主字段打头）。
    pub fn primary_balance(&self) -> Option<f64> {
        if self.kind != UsageKind::Balance {
            return None;
        }
        self.balances
            .iter()
            .find(|b| b.primary)
            .or_else(|| self.balances.first())
            .map(|b| b.amount)
    }

    /// 按量付费「当前是否可用」判定：查得到余额且主余额字段 > 0 → true；
    /// 主余额字段 = 0 → false（余额耗尽，不参与负载均衡）；
    /// 非 balance 形态 / 完全查不到余额（无法判定）→ None，视为可用，避免上游抖动误伤。
    pub fn balance_usable(&self) -> Option<bool> {
        if self.kind != UsageKind::Balance {
            return None;
        }
        let amount = self.primary_balance()?;
        Some(amount > 0.0)
    }

    /// 返回 remaining_percent 已按 remaining_percent_value() 推导并取整的副本：
    /// 接口出口统一调用，前端直接使用该字段，无需自行推导/取整。
    pub fn with_normalized_remaining(&self) -> Self {
        let mut data = self.clone();
        for window in &mut data.windows {
            if let Some(p) = window.remaining_percent_value() {
                window.remaining_percent = Some(p);
            }
        }
        data
    }

    /// 按管理后台语言本地化用户可见字段（当前只有 balance label）的副本。
    pub fn with_localized_labels(&self, lang: Lang) -> Self {
        let mut data = self.clone();
        data.balances = data
            .balances
            .iter()
            .map(|item| item.with_localized_label(lang))
            .collect();
        data
    }
}

/// 生成 5h/周/月三个兼容槽位，fetcher 按需替换其中元素。
pub fn empty_windows() -> Vec<QuotaWindow> {
    vec![
        QuotaWindow::unavailable(WindowKind::FiveHour),
        QuotaWindow::unavailable(WindowKind::Weekly),
        QuotaWindow::unavailable(WindowKind::Monthly),
    ]
}

/// 按 window 种类定位并替换已有槽位。
pub fn set_window(windows: &mut [QuotaWindow], w: QuotaWindow) {
    if let Some(slot) = windows.iter_mut().find(|slot| slot.window == w.window) {
        *slot = w;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BalanceItem {
    pub label: String,
    pub amount: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    /// 是否为该供应商参与 LB 比较的主余额字段（fetcher 侧标记，每家至多一条）。
    #[serde(default)]
    pub primary: bool,
}

/// fetcher 侧中文 label → 英文 label 的映射（未知 label 原样返回）。
/// 数据库缓存里存的是中文 label；响应层按当前语言翻译。
fn translate_balance_label(label: &str, lang: Lang) -> &str {
    if lang == Lang::Zh {
        return label;
    }
    match label {
        "余额" => "Balance",
        "可用余额" => "Available Balance",
        "可用总余额" => "Total Available Balance",
        "钱包余额" => "Wallet Balance",
        "福利余额" => "Welfare Balance",
        "充值余额" => "Topped-up Balance",
        "赠送余额" => "Granted Balance",
        "现金余额" => "Cash Balance",
        "代金券余额" => "Voucher Balance",
        "信控额度" => "Credit Limit",
        "冻结金额" => "Frozen Amount",
        "欠费金额" => "Arrears",
        "剩余额度" => "Remaining Credits",
        "已使用" => "Used",
        "总充值" => "Total Top-up",
        "透支额度" => "Overdraft Limit",
        "剩余透支额度" => "Remaining Overdraft",
        "累计充值" => "Total Top-up",
        _ => label,
    }
}

impl BalanceItem {
    /// 按管理后台语言生成用户可见的 label 副本。fetcher 侧可能已拼上
    /// `（币种）`/` (currency)` 后缀（如「余额（CNY）」），这里剥离后缀
    /// 翻译 base 后按当前语言重拼；无后缀的纯 label 直接翻译。
    pub fn with_localized_label(&self, lang: Lang) -> Self {
        let (base, suffix) = split_label_suffix(&self.label);
        let base = translate_balance_label(base, lang);
        let label = match suffix {
            Some(currency) => match lang {
                Lang::Zh => format!("{base}（{currency}）"),
                Lang::En => format!("{base} ({currency})"),
            },
            None => base.to_string(),
        };
        Self {
            label,
            amount: self.amount,
            currency: self.currency.clone(),
            primary: self.primary,
        }
    }
}

/// 拆出 label 尾部已拼的币种后缀（`（xxx）` 或 ` (xxx)`）。
fn split_label_suffix(label: &str) -> (&str, Option<&str>) {
    if let Some(rest) = label.strip_suffix('）')
        && let Some(open) = rest.rfind('（')
    {
        return (&rest[..open], Some(&rest[open + '（'.len_utf8()..]));
    }
    if let Some(rest) = label.strip_suffix(')')
        && let Some(open) = rest.rfind(" (")
    {
        return (&rest[..open], Some(&rest[open + 2..]));
    }
    (label, None)
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
    fn remaining_percent_value_rounds_to_two_decimals() {
        // 直取 remaining_percent：长尾值被取整。
        let w = QuotaWindow {
            window: WindowKind::Weekly,
            available: true,
            used_percent: None,
            remaining_percent: Some(63.499),
            resets_at: None,
            used: None,
            limit: None,
            unit: None,
            label: None,
        };
        assert_eq!(w.remaining_percent_value(), Some(63.5));
        // used_percent 推导：100 - 42.5 = 57.5。
        let w = QuotaWindow::from_used_percent(WindowKind::Weekly, 42.5, None);
        assert_eq!(w.remaining_percent_value(), Some(57.5));
        // used/limit 计算：浮点噪声被取整（(100-30)/100*100 = 70.00000001 → 70）。
        let w = QuotaWindow {
            window: WindowKind::Weekly,
            available: true,
            used_percent: None,
            remaining_percent: None,
            resets_at: None,
            used: Some(30.0),
            limit: Some(100.0),
            unit: None,
            label: None,
        };
        assert_eq!(w.remaining_percent_value(), Some(70.0));
        // 不可用窗口返回 None。
        let w = QuotaWindow::unavailable(WindowKind::Weekly);
        assert_eq!(w.remaining_percent_value(), None);
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

    #[test]
    fn balance_usable_three_way() {
        let balance = |amounts: &[f64]| UsageData {
            provider_id: 1,
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
        };
        // 查得到余额且主余额字段 > 0 → 可用。
        assert_eq!(balance(&[100.0]).balance_usable(), Some(true));
        assert_eq!(balance(&[50.0, 0.5]).balance_usable(), Some(true));
        // 查得到余额且主余额字段 = 0 → 不可用（余额耗尽）。
        assert_eq!(balance(&[0.0]).balance_usable(), Some(false));
        assert_eq!(balance(&[0.0, 0.0]).balance_usable(), Some(false));
        // 完全查不到余额（空 balances）→ 无法判定，视为可用。
        assert_eq!(balance(&[]).balance_usable(), None);
        // 非 balance 形态 → 无法判定。
        let quota = UsageData {
            provider_id: 1,
            fetched_at: Utc::now(),
            kind: UsageKind::Quota,
            plan: None,
            windows: empty_windows(),
            balances: vec![],
        };
        assert_eq!(quota.balance_usable(), None);
    }
}

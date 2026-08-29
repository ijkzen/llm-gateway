//! 小米 MiMo（CookieCloud 提供 platform.xiaomimimo.com 登录态）。
//!
//! 两个形态：
//! - `Xiaomi`（usage_type=0）：GET /api/v1/balance → 余额（balance/cashBalance/giftBalance 为字符串）。
//! - `Xiaomi Token Plan *`（usage_type=1）：GET /api/v1/tokenPlan/detail + /api/v1/tokenPlan/usage
//!   → 月窗已用百分比（monthUsage.percent）+ 账期结束时间。
//!
//! `code != 0` 为业务错误（401 登录态失效）；3xx 重定向视为会话过期。

use serde_json::Value;

use super::{Credentials, num, reset_ts, snippet};
use crate::usage::cookiecloud;
use crate::usage::error::UsageError;
use crate::usage::http::{HttpReply, UsageHttp, parse_json};
use crate::usage::types::{BalanceItem, FetchOutput, QuotaWindow, WindowKind, empty_windows, set_window};

const API_BASE: &str = "https://platform.xiaomimimo.com/api/v1";
const REFERER: &str = "https://platform.xiaomimimo.com/";

fn headers(cookie: &str) -> [(&'static str, String); 2] {
    [
        ("Cookie", cookie.to_string()),
        ("Referer", REFERER.to_string()),
    ]
}

fn ensure_ok(reply: &HttpReply) -> Result<(), UsageError> {
    if reply.status == 401 || reply.status == 403 || (300..400).contains(&reply.status) {
        return Err(UsageError::Auth);
    }
    if reply.status != 200 {
        return Err(UsageError::Upstream(reply.status, snippet(&reply.body)));
    }
    Ok(())
}

pub async fn fetch_xiaomi_balance(
    http: &UsageHttp,
    creds: &Credentials<'_>,
) -> Result<FetchOutput, UsageError> {
    let (cfg, domain) = creds.cookiecloud()?;
    let cookies = cookiecloud::fetch_cookies(http, &cfg, &domain).await?;
    let cookie = cookiecloud::cookie_header(&cookies);

    let reply = http
        .get(&format!("{API_BASE}/balance"), &headers(&cookie))
        .await?;
    ensure_ok(&reply)?;
    parse_xiaomi_balance(&reply.body)
}

fn parse_xiaomi_balance(body: &str) -> Result<FetchOutput, UsageError> {
    let v: Value = serde_json::from_str(body)
        .map_err(|e| UsageError::Parse(format!("响应不是合法 JSON：{e}")))?;
    if v.get("code").and_then(Value::as_i64).unwrap_or(-1) != 0 {
        return Err(UsageError::Upstream(200, snippet(body)));
    }
    let data = v
        .get("data")
        .ok_or_else(|| UsageError::Parse("缺少 data 字段".to_string()))?;
    let currency = data
        .get("currency")
        .and_then(Value::as_str)
        .map(str::to_string);

    let mut items = Vec::new();
    for (key, label) in [
        ("balance", "余额"),
        ("cashBalance", "现金余额"),
        ("giftBalance", "赠送余额"),
        ("frozenBalance", "冻结金额"),
        ("overdraftLimit", "透支额度"),
        ("remainingOverdraftLimit", "剩余透支额度"),
    ] {
        if let Some(amount) = data.get(key).and_then(num) {
            items.push(BalanceItem {
                label: label.to_string(),
                amount,
                currency: currency.clone(),
            });
        }
    }
    if items.is_empty() {
        return Err(UsageError::Parse("data 中没有余额字段".to_string()));
    }
    Ok(FetchOutput::Balance { items })
}

pub async fn fetch_xiaomi_token_plan(
    http: &UsageHttp,
    creds: &Credentials<'_>,
) -> Result<FetchOutput, UsageError> {
    let (cfg, domain) = creds.cookiecloud()?;
    let cookies = cookiecloud::fetch_cookies(http, &cfg, &domain).await?;
    let cookie = cookiecloud::cookie_header(&cookies);

    // 套餐详情失败不阻塞用量展示（仅影响套餐名与账期结束时间）。
    let detail = match http
        .get(&format!("{API_BASE}/tokenPlan/detail"), &headers(&cookie))
        .await
    {
        Ok(reply) if reply.status == 200 => parse_json(&reply).ok(),
        _ => None,
    };
    let usage = http
        .get(&format!("{API_BASE}/tokenPlan/usage"), &headers(&cookie))
        .await?;
    ensure_ok(&usage)?;

    parse_xiaomi_token_plan(detail.as_ref(), &usage.body)
}

fn parse_xiaomi_token_plan(
    detail: Option<&Value>,
    usage_body: &str,
) -> Result<FetchOutput, UsageError> {
    let v: Value = serde_json::from_str(usage_body)
        .map_err(|e| UsageError::Parse(format!("响应不是合法 JSON：{e}")))?;
    if v.get("code").and_then(Value::as_i64).unwrap_or(-1) != 0 {
        return Err(UsageError::Upstream(200, snippet(usage_body)));
    }

    let plan = detail.and_then(|d| {
        d.get("data")
            .and_then(|data| data.get("planCode"))
            .and_then(Value::as_str)
            .map(str::to_string)
    });
    let resets_at = detail.and_then(|d| {
        d.get("data")
            .and_then(|data| data.get("currentPeriodEnd"))
            .and_then(reset_ts)
    });

    let mut windows = empty_windows();
    if let Some(percent) = v
        .get("data")
        .and_then(|d| d.get("monthUsage"))
        .and_then(|m| m.get("percent"))
        .and_then(num)
    {
        set_window(
            &mut windows,
            QuotaWindow::from_used_percent(WindowKind::Monthly, percent, resets_at),
        );
    }
    Ok(FetchOutput::Quota { plan, windows })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balance_string_amounts() {
        let body = r#"{ "code": 0, "data": { "balance": "123.45", "currency": "CNY", "cashBalance": "100.00", "giftBalance": "23.45", "frozenBalance": "1.00", "overdraftLimit": "10.00", "remainingOverdraftLimit": "9.00" } }"#;
        let FetchOutput::Balance { items } = parse_xiaomi_balance(body).unwrap() else {
            panic!("expected balance")
        };
        assert_eq!(items.len(), 6);
        assert_eq!(items[0].amount, 123.45);
        assert_eq!(items[0].currency.as_deref(), Some("CNY"));
        assert_eq!(items[3].label, "冻结金额");
        assert_eq!(items[4].label, "透支额度");
        assert_eq!(items[5].label, "剩余透支额度");
    }

    #[test]
    fn token_plan_monthly_only() {
        let detail: Value = serde_json::from_str(
            r#"{ "code": 0, "data": { "planCode": "pro", "currentPeriodEnd": "2026-09-14 00:00", "expired": false } }"#,
        )
        .unwrap();
        let usage = r#"{ "code": 0, "data": { "monthUsage": { "percent": 12.3 } } }"#;
        let FetchOutput::Quota { plan, windows } =
            parse_xiaomi_token_plan(Some(&detail), usage).unwrap()
        else {
            panic!("expected quota")
        };
        assert_eq!(plan.as_deref(), Some("pro"));
        assert!(!windows[0].available);
        assert!(!windows[1].available);
        assert!(windows[2].available);
        assert_eq!(windows[2].used_percent, Some(12.3));
        assert!(windows[2].resets_at.is_some());
    }

    #[test]
    fn redirect_means_session_expired() {
        let reply = HttpReply { status: 302, body: String::new() };
        assert!(matches!(ensure_ok(&reply), Err(UsageError::Auth)));
    }
}

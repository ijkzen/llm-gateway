//! 阶跃 StepFun Step Plan（CookieCloud 提供 platform.stepfun.com 登录态）。
//!
//! 两个 proto-RPC 风格接口（POST，JSON 空对象 body）：
//! - GetStepPlanStatus → plan_type 套餐名
//! - QueryStepPlanRateLimit → 各窗口【剩余比例】（0–1）：five_hour/weekly 映射为
//!   5h/周窗，subscription_credit_left_rate 视作月窗。
//!
//! 需要 `oasis-webid` 请求头（取自同名 cookie）。

use serde_json::Value;

use super::{Credentials, num, reset_ts, snippet};
use crate::usage::cookiecloud;
use crate::usage::error::UsageError;
use crate::usage::http::{HttpReply, UsageHttp, parse_json};
use crate::usage::types::{FetchOutput, QuotaWindow, WindowKind, empty_windows, set_window};

const API_BASE: &str = "https://platform.stepfun.com/api/step.openapi.devcenter.Dashboard";

pub async fn fetch_stepfun(
    http: &UsageHttp,
    creds: &Credentials<'_>,
) -> Result<FetchOutput, UsageError> {
    let (cfg, domain) = creds.cookiecloud()?;
    let cookies = cookiecloud::fetch_cookies(http, &cfg, &domain).await?;
    let cookie = cookiecloud::cookie_header(&cookies);
    let webid = cookiecloud::find_cookie(&cookies, "oasis-webid").unwrap_or_default();

    let mut headers = vec![("Cookie", cookie)];
    if !webid.is_empty() {
        headers.push(("oasis-webid", webid.to_string()));
    }

    // 套餐状态失败不阻塞窗口数据。
    let plan = match http
        .post_json(&format!("{API_BASE}/GetStepPlanStatus"), &headers, "{}")
        .await
    {
        Ok(reply) if reply.status == 200 => parse_json(&reply).ok().and_then(|v| {
            v.get("plan_type")
                .and_then(Value::as_str)
                .map(str::to_string)
        }),
        _ => None,
    };

    let reply = http
        .post_json(
            &format!("{API_BASE}/QueryStepPlanRateLimit"),
            &headers,
            "{}",
        )
        .await?;
    ensure_ok(&reply)?;
    parse_stepfun_rate_limit(&reply.body, plan)
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

fn parse_stepfun_rate_limit(body: &str, plan: Option<String>) -> Result<FetchOutput, UsageError> {
    let v: Value = serde_json::from_str(body)
        .map_err(|e| UsageError::Parse(format!("响应不是合法 JSON：{e}")))?;

    let mut windows = empty_windows();
    for (rate_key, reset_key, kind) in [
        (
            "five_hour_usage_left_rate",
            "five_hour_usage_reset_time",
            WindowKind::FiveHour,
        ),
        (
            "weekly_usage_left_rate",
            "weekly_usage_reset_time",
            WindowKind::Weekly,
        ),
        (
            "subscription_credit_left_rate",
            "subscription_credit_reset_time",
            WindowKind::Monthly,
        ),
    ] {
        let Some(left_rate) = v.get(rate_key).and_then(num) else {
            continue;
        };
        let resets_at = v.get(reset_key).and_then(reset_ts);
        set_window(
            &mut windows,
            QuotaWindow::from_remaining_percent(kind, left_rate * 100.0, resets_at),
        );
    }
    if windows.iter().all(|w| !w.available) {
        return Err(UsageError::Parse("响应中没有窗口剩余比例字段".to_string()));
    }
    Ok(FetchOutput::Quota { plan, windows })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn left_rate_to_remaining_percent() {
        let body = r#"{
          "five_hour_usage_left_rate": 0.58, "five_hour_usage_reset_time": "2026-08-30T18:00:00Z",
          "weekly_usage_left_rate": 0.80,    "weekly_usage_reset_time": "2026-09-01T00:00:00Z",
          "subscription_credit_left_rate": 0.9,
          "topup_credit_left_rate": 1.0
        }"#;
        let FetchOutput::Quota { plan, windows } =
            parse_stepfun_rate_limit(body, Some("pro".to_string())).unwrap()
        else {
            panic!("expected quota")
        };
        assert_eq!(plan.as_deref(), Some("pro"));
        assert!(windows.iter().all(|w| w.available));
        assert_eq!(windows[0].remaining_percent, Some(58.0));
        assert_eq!(windows[1].remaining_percent, Some(80.0));
        assert_eq!(windows[2].remaining_percent, Some(90.0));
    }

    #[test]
    fn missing_all_rates_is_parse_error() {
        assert!(matches!(
            parse_stepfun_rate_limit("{}", None),
            Err(UsageError::Parse(_))
        ));
    }
}

//! GitHub Copilot（OAuth token，extra.oauth_token）。
//!
//! GET https://api.github.com/copilot_internal/user（`Authorization: token <oauth_token>`，
//! 注意前缀是 token 不是 Bearer）。quota_snapshots.premium_interactions 映射为月窗；
//! unlimited=true 时额度字段可能缺失，窗口标为不可用。

use serde_json::Value;

use super::{Credentials, num, reset_ts, snippet};
use crate::usage::error::UsageError;
use crate::usage::http::{UsageHttp, ensure_not_auth_error};
use crate::usage::types::{FetchOutput, QuotaWindow, WindowKind, empty_windows, set_window};

pub async fn fetch_copilot(
    http: &UsageHttp,
    creds: &Credentials<'_>,
) -> Result<FetchOutput, UsageError> {
    let token = creds.require("oauth_token")?;
    let reply = http
        .get(
            "https://api.github.com/copilot_internal/user",
            &[("Authorization", format!("token {token}"))],
        )
        .await?;
    ensure_not_auth_error(&reply)?;
    if reply.status != 200 {
        return Err(UsageError::Upstream(reply.status, snippet(&reply.body)));
    }
    parse_copilot(&reply.body)
}

fn parse_copilot(body: &str) -> Result<FetchOutput, UsageError> {
    let v: Value = serde_json::from_str(body)
        .map_err(|e| UsageError::Parse(format!("响应不是合法 JSON：{e}")))?;
    let plan = v
        .get("copilot_plan")
        .and_then(Value::as_str)
        .map(str::to_string);
    let resets_at = v.get("quota_reset_date").and_then(reset_ts);

    let mut windows = empty_windows();
    if let Some(premium) = v
        .get("quota_snapshots")
        .and_then(|q| q.get("premium_interactions"))
        && premium.get("unlimited").and_then(Value::as_bool) != Some(true)
    {
        // 优先 percent_remaining；缺失时用 entitlement − remaining 兜底。
        let window = if let Some(remaining_percent) =
            premium.get("percent_remaining").and_then(num)
        {
            let mut w =
                QuotaWindow::from_remaining_percent(WindowKind::Monthly, remaining_percent, resets_at);
            w.limit = premium.get("entitlement").and_then(num);
            w.used = premium
                .get("credits_used")
                .and_then(num)
                .or_else(|| match (w.limit, premium.get("remaining").and_then(num)) {
                    (Some(total), Some(remaining)) => Some(total - remaining),
                    _ => None,
                });
            w
        } else if let (Some(entitlement), Some(remaining)) = (
            premium.get("entitlement").and_then(num),
            premium.get("remaining").and_then(num),
        ) {
            QuotaWindow::from_used_limit(
                WindowKind::Monthly,
                entitlement - remaining,
                entitlement,
                resets_at,
                Some("credits"),
            )
        } else {
            QuotaWindow::unavailable(WindowKind::Monthly)
        };
        set_window(&mut windows, window);
    }
    Ok(FetchOutput::Quota { plan, windows })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::types::WindowKind;

    #[test]
    fn premium_interactions_to_monthly_window() {
        let body = r#"{
          "copilot_plan": "individual_pro",
          "quota_reset_date": "2026-09-01T00:00:00Z",
          "quota_snapshots": {
            "premium_interactions": { "entitlement": 300, "remaining": 250, "percent_remaining": 83.3, "credits_used": 50, "unlimited": false },
            "chat": { "unlimited": true }
          }
        }"#;
        let FetchOutput::Quota { plan, windows } = parse_copilot(body).unwrap() else {
            panic!("expected quota")
        };
        assert_eq!(plan.as_deref(), Some("individual_pro"));
        assert!(!windows[0].available);
        assert!(!windows[1].available);
        assert!(windows[2].available);
        assert_eq!(windows[2].window, WindowKind::Monthly);
        assert_eq!(windows[2].remaining_percent, Some(83.3));
        assert_eq!(windows[2].limit, Some(300.0));
    }

    #[test]
    fn unlimited_premium_is_unavailable() {
        let body = r#"{
          "copilot_plan": "business",
          "quota_snapshots": {
            "premium_interactions": { "unlimited": true }
          }
        }"#;
        let FetchOutput::Quota { windows, .. } = parse_copilot(body).unwrap() else {
            panic!("expected quota")
        };
        assert!(windows.iter().all(|w| !w.available));
    }

    #[test]
    fn fallback_to_entitlement_minus_remaining() {
        let body = r#"{
          "quota_snapshots": {
            "premium_interactions": { "entitlement": 1000, "remaining": 400, "unlimited": false }
          }
        }"#;
        let FetchOutput::Quota { windows, .. } = parse_copilot(body).unwrap() else {
            panic!("expected quota")
        };
        assert!(windows[2].available);
        assert_eq!(windows[2].used, Some(600.0));
        assert_eq!(windows[2].used_percent, Some(60.0));
    }
}

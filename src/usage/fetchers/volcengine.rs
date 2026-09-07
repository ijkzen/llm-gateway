//! 火山方舟 Agent Plan / Coding Plan（AK/SK 签名 V4，控制面 OpenAPI）。
//!
//! 两个 Action 共用同一份 AK/SK：先 `GetCodingPlanUsage`（只有百分比，字段名
//! 官方未给规格，防御式匹配），失败且非鉴权错误时回退 `GetAFPUsage`（绝对额度）。
//! 鉴权类错误（401/403 或错误码含 auth/signature/denied）直接失败，不再试另一个。

use serde_json::Value;

use super::{Credentials, num, reset_ts_of, snippet};
use crate::usage::error::UsageError;
use crate::usage::http::{HttpReply, UsageHttp, ensure_not_auth_error, parse_json};
use crate::usage::types::{FetchOutput, QuotaWindow, WindowKind, empty_windows, set_window};
use crate::usage::volcengine_sign;

const GATEWAY_HOST: &str = "open.volcengineapi.com";
const VERSION: &str = "2024-01-01";
const REGION: &str = "cn-beijing";

pub async fn fetch_volcengine(
    http: &UsageHttp,
    creds: &Credentials<'_>,
) -> Result<FetchOutput, UsageError> {
    let ak = creds.require("ak")?;
    let sk = creds.require("sk")?;

    let coding = call_action(http, "GetCodingPlanUsage", ak, sk).await?;
    if let Ok(output) = parse_coding_plan(&coding) {
        return Ok(output);
    }
    // 鉴权硬失败：不必再试另一个 Action。
    auth_error(&coding)?;

    let afp = call_action(http, "GetAFPUsage", ak, sk).await?;
    auth_error(&afp)?;
    parse_afp(&afp)
}

async fn call_action(
    http: &UsageHttp,
    action: &str,
    ak: &str,
    sk: &str,
) -> Result<HttpReply, UsageError> {
    let body = "";
    let sig = volcengine_sign::sign(
        "POST",
        GATEWAY_HOST,
        "ark",
        action,
        VERSION,
        REGION,
        ak,
        sk,
        body.as_bytes(),
        chrono::Utc::now(),
    );
    let url = format!("https://{GATEWAY_HOST}/?Action={action}&Version={VERSION}&Region={REGION}");
    http.post_json(
        &url,
        &[
            ("Authorization", sig.authorization),
            ("X-Date", sig.x_date),
            ("X-Content-Sha256", sig.payload_hash),
        ],
        body,
    )
    .await
}

/// 鉴权类错误判定：HTTP 401/403，或错误码/消息含 auth/signature/denied。
fn auth_error(reply: &HttpReply) -> Result<(), UsageError> {
    ensure_not_auth_error(reply)?;
    if let Ok(v) = parse_json(reply)
        && let Some(err) = v.get("ResponseMetadata").and_then(|m| m.get("Error"))
    {
        let code = err.get("Code").and_then(Value::as_str).unwrap_or_default();
        let message = err
            .get("Message")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let text = format!("{code} {message}").to_ascii_lowercase();
        if text.contains("auth") || text.contains("signature") || text.contains("denied") {
            return Err(UsageError::Auth);
        }
        return Err(UsageError::Upstream(
            reply.status,
            format!("{code} {message}"),
        ));
    }
    if reply.status != 200 {
        return Err(UsageError::Upstream(reply.status, snippet(&reply.body)));
    }
    Ok(())
}

/// GetCodingPlanUsage：Result 下的数组键名见过 QuotaUsage/Usages/Details 多种包裹；
/// 条目窗口键 Window/Label/Type，百分比键 Percent/UsagePercent/UsedPercent（已用）。
fn parse_coding_plan(reply: &HttpReply) -> Result<FetchOutput, UsageError> {
    auth_error(reply)?;
    let v = parse_json(reply)?;
    let result = v
        .get("Result")
        .ok_or_else(|| UsageError::Parse("缺少 Result 字段".to_string()))?;

    let entries = ["QuotaUsage", "Usages", "Details"]
        .iter()
        .find_map(|key| result.get(key).and_then(Value::as_array))
        .ok_or_else(|| UsageError::Parse("Result 中没有用量数组".to_string()))?;

    let mut windows = empty_windows();
    for entry in entries {
        // 实测字段是 `Level`（session/weekly/monthly）；Window/Label/Type 仅作防御式兜底。
        let label = ["Level", "Window", "Label", "Type"]
            .iter()
            .find_map(|k| entry.get(k).and_then(Value::as_str))
            .unwrap_or_default()
            .to_ascii_lowercase();
        let kind =
            if label.contains("session") || label.contains("5h") || label.contains("fivehour") {
                WindowKind::FiveHour
            } else if label.contains("weekly") || label.contains("week") || label.contains("7d") {
                WindowKind::Weekly
            } else if label.contains("monthly") || label.contains("month") {
                WindowKind::Monthly
            } else {
                continue;
            };
        let Some(percent) = ["Percent", "UsagePercent", "UsedPercent"]
            .iter()
            .find_map(|k| entry.get(k).and_then(num))
        else {
            continue;
        };
        let resets_at = reset_ts_of(entry, &["ResetTimestamp", "ResetTime", "resetTime"]);
        set_window(
            &mut windows,
            QuotaWindow::from_used_percent(kind, percent, resets_at),
        );
    }
    if windows.iter().all(|w| !w.available) {
        return Err(UsageError::Parse("没有可识别的窗口条目".to_string()));
    }
    Ok(FetchOutput::Quota {
        plan: None,
        windows,
    })
}

/// GetAFPUsage：Result.AFPFiveHour/AFPWeekly/AFPMonthly = { Quota, Used, ResetTime }。
fn parse_afp(reply: &HttpReply) -> Result<FetchOutput, UsageError> {
    auth_error(reply)?;
    let v = parse_json(reply)?;
    let result = v
        .get("Result")
        .ok_or_else(|| UsageError::Parse("缺少 Result 字段".to_string()))?;
    let plan = result
        .get("PlanType")
        .and_then(Value::as_str)
        .map(str::to_string);

    let mut windows = empty_windows();
    for (key, kind) in [
        ("AFPFiveHour", WindowKind::FiveHour),
        ("AFPWeekly", WindowKind::Weekly),
        ("AFPMonthly", WindowKind::Monthly),
    ] {
        let Some(entry) = result.get(key) else {
            continue;
        };
        let (Some(used), Some(quota)) = (
            entry.get("Used").and_then(num),
            entry.get("Quota").and_then(num),
        ) else {
            continue;
        };
        let resets_at = reset_ts_of(entry, &["ResetTime", "ResetTimestamp"]);
        set_window(
            &mut windows,
            QuotaWindow::from_used_limit(kind, used, quota, resets_at, None),
        );
    }
    if windows.iter().all(|w| !w.available) {
        return Err(UsageError::Parse("没有可识别的 AFP 窗口".to_string()));
    }
    Ok(FetchOutput::Quota { plan, windows })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok_reply(body: &str) -> HttpReply {
        HttpReply {
            status: 200,
            body: body.to_string(),
        }
    }

    #[test]
    fn coding_plan_defensive_keys() {
        let body = r#"{
          "ResponseMetadata": {},
          "Result": {
            "Usages": [
              { "Window": "session", "Percent": 42.0, "ResetTimestamp": 1700000000 },
              { "Label": "weekly", "UsagePercent": 10.0, "ResetTimestamp": 1700100000 },
              { "Type": "monthly", "UsedPercent": 5.0 }
            ]
          }
        }"#;
        let FetchOutput::Quota { windows, .. } = parse_coding_plan(&ok_reply(body)).unwrap() else {
            panic!("expected quota")
        };
        assert!(windows.iter().all(|w| w.available));
        assert_eq!(windows[0].used_percent, Some(42.0));
        assert_eq!(windows[1].used_percent, Some(10.0));
        assert_eq!(windows[2].used_percent, Some(5.0));
    }

    #[test]
    fn coding_plan_real_volcengine_response_with_level() {
        // 实测 GetCodingPlanUsage 真实返回：窗口键是 `Level`，session 的
        // ResetTimestamp=-1（未计时占位），周/月为秒级时间戳。
        let body = r#"{
          "ResponseMetadata": {},
          "Result": {
            "Status": "Running",
            "UpdateTimestamp": 1788768130,
            "QuotaUsage": [
              { "Level": "session", "Percent": 12.5, "ResetTimestamp": -1, "Cap": 100, "RewardTotalPercent": 0 },
              { "Level": "weekly", "Percent": 3.0, "ResetTimestamp": 1789315200, "Cap": 100, "RewardTotalPercent": 0 },
              { "Level": "monthly", "Percent": 0.5, "ResetTimestamp": 1791388799, "Cap": 100, "RewardTotalPercent": 0 }
            ],
            "HasReward": false
          }
        }"#;
        let FetchOutput::Quota { plan, windows } = parse_coding_plan(&ok_reply(body)).unwrap()
        else {
            panic!("expected quota")
        };
        assert_eq!(plan, None);
        assert_eq!(windows.len(), 3);
        assert!(windows.iter().all(|w| w.available));
        // session 窗口：percent 透传，-1 重置时间应被忽略（None）。
        assert_eq!(windows[0].used_percent, Some(12.5));
        assert_eq!(windows[0].resets_at, None);
        // 周/月窗口保留秒级重置时间。
        assert_eq!(windows[1].used_percent, Some(3.0));
        assert_eq!(
            windows[1].resets_at,
            Some(crate::usage::types::ts_secs(1789315200).unwrap())
        );
        assert_eq!(windows[2].used_percent, Some(0.5));
    }

    #[test]
    fn afp_absolute_quota() {
        let body = r#"{
          "Result": {
            "PlanType": "pro",
            "AFPFiveHour": { "Quota": 1000, "Used": 120, "ResetTime": "2026-08-30T18:00:00Z" },
            "AFPWeekly":   { "Quota": 5000, "Used": 300 },
            "AFPMonthly":  { "Quota": 20000, "Used": 900 }
          }
        }"#;
        let FetchOutput::Quota { plan, windows } = parse_afp(&ok_reply(body)).unwrap() else {
            panic!("expected quota")
        };
        assert_eq!(plan.as_deref(), Some("pro"));
        assert_eq!(windows[0].used_percent, Some(12.0));
        assert_eq!(windows[0].limit, Some(1000.0));
        assert_eq!(windows[2].used_percent, Some(4.5));
    }

    #[test]
    fn auth_error_detection() {
        let body = r#"{
          "ResponseMetadata": { "Error": { "Code": "InvalidAuthorization", "Message": "denied" } }
        }"#;
        assert!(matches!(auth_error(&ok_reply(body)), Err(UsageError::Auth)));

        let http_err = HttpReply {
            status: 403,
            body: String::new(),
        };
        assert!(matches!(auth_error(&http_err), Err(UsageError::Auth)));
    }
}

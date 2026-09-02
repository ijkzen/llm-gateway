//! SenseNova Token Plan（控制台 OAuth，extra.refresh_token）。
//!
//! 续期：POST /oauth2/token（form：grant_type=refresh_token、client_id=nova），
//! refresh_token 每次刷新轮换，返回的新值由调用方写回 provider extra。
//! 查询：GET /lite/console/v1/tokenplan/pool-usage（Bearer access_token），
//! 逐积分池产出 5h/7d 窗口（label=池名）；sk- 密钥不能调用量接口（401）。

use chrono::{DateTime, Utc};
use serde_json::Value;

use super::{Credentials, num, snippet};
use crate::usage::error::UsageError;
use crate::usage::http::{UsageHttp, ensure_not_auth_error};
use crate::usage::types::{FetchOutput, QuotaWindow, WindowKind, ts_secs};

const TOKEN_URL: &str = "https://platform.sensenova.cn/oauth2/token";
const USAGE_URL: &str = "https://platform.sensenova.cn/lite/console/v1/tokenplan/pool-usage";

/// 返回 (用量数据, 轮换出的新 refresh_token；与旧值相同或缺失时为 None)。
pub async fn fetch_sensenova(
    http: &UsageHttp,
    creds: &Credentials<'_>,
) -> Result<(FetchOutput, Option<String>), UsageError> {
    let refresh_token = creds.require("refresh_token")?;
    // refresh_token 为 JWT/base64url 字符，均为 form 安全字符，直接内嵌。
    let form = format!("grant_type=refresh_token&client_id=nova&refresh_token={refresh_token}");
    let reply = http.post_form(TOKEN_URL, &[], &form).await?;
    ensure_not_auth_error(&reply)?;
    if reply.status != 200 {
        return Err(UsageError::Upstream(reply.status, snippet(&reply.body)));
    }
    let token: Value = serde_json::from_str(&reply.body)
        .map_err(|e| UsageError::Parse(format!("续期响应不是合法 JSON：{e}")))?;
    let access_token = token
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or(UsageError::Auth)?;
    let rotated = token
        .get("refresh_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != refresh_token)
        .map(str::to_string);

    let reply = http
        .get(
            USAGE_URL,
            &[
                ("Authorization", format!("Bearer {access_token}")),
                ("Accept-Language", "zh-CN".to_string()),
            ],
        )
        .await?;
    ensure_not_auth_error(&reply)?;
    if reply.status != 200 {
        return Err(UsageError::Upstream(reply.status, snippet(&reply.body)));
    }
    parse_pool_usage(&reply.body).map(|output| (output, rotated))
}

fn parse_pool_usage(body: &str) -> Result<FetchOutput, UsageError> {
    let v: Value = serde_json::from_str(body)
        .map_err(|e| UsageError::Parse(format!("响应不是合法 JSON：{e}")))?;
    let plan = v
        .get("plan")
        .and_then(|p| p.get("name"))
        .and_then(Value::as_str)
        .map(str::to_string);

    let mut windows = Vec::new();
    let pools = v.get("pools").and_then(Value::as_array);
    for pool in pools.into_iter().flatten() {
        let label = pool.get("name").and_then(Value::as_str).map(str::to_string);
        for (key, kind) in [
            ("window_5h", WindowKind::FiveHour),
            ("window_7d", WindowKind::Weekly),
        ] {
            if let Some(w) = pool.get(key).and_then(|d| window_from(kind, d)) {
                windows.push(QuotaWindow {
                    label: label.clone(),
                    ..w
                });
            }
        }
    }
    Ok(FetchOutput::Quota { plan, windows })
}

/// 单池窗口：limit/used/remaining 均为字符串数字，reset_at 为秒级时间戳字符串。
fn window_from(kind: WindowKind, detail: &Value) -> Option<QuotaWindow> {
    let limit = num(detail.get("limit")?)?;
    let used = num(detail.get("used")?)?;
    let resets_at = detail.get("reset_at").and_then(reset_secs);
    Some(QuotaWindow::from_used_limit(
        kind,
        used,
        limit,
        resets_at,
        Some("积分"),
    ))
}

/// 秒级时间戳（字符串或数字）→ UTC。
fn reset_secs(v: &Value) -> Option<DateTime<Utc>> {
    let n = match v {
        Value::String(s) => s.trim().parse::<i64>().ok()?,
        Value::Number(n) => n.as_i64()?,
        _ => return None,
    };
    ts_secs(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multi_pool_parses_to_labeled_windows() {
        let body = r#"{
          "plan": { "id": "free", "name": "Free Plan", "type": "TOKEN_PLAN_PLAN_TYPE_FREE" },
          "pools": [
            { "id": "pool_a", "name": "通用积分池", "pool_type": "default",
              "window_5h": { "limit": "60000", "used": "33586.30032", "remaining": "26413.69968", "reset_at": "1788365437" },
              "window_7d": { "limit": "600000", "used": "51388.65712", "remaining": "548611.34288", "reset_at": "1788862237" } },
            { "id": "pool_b", "name": "Flash-Lite积分池", "pool_type": "dedicated",
              "window_5h": { "limit": "10000", "used": "9999", "remaining": "1", "reset_at": "1788365437" } }
          ]
        }"#;
        let FetchOutput::Quota { plan, windows } = parse_pool_usage(body).unwrap() else {
            panic!("expected quota")
        };
        assert_eq!(plan.as_deref(), Some("Free Plan"));
        assert_eq!(windows.len(), 3);
        // 每池独立产出，label = 池名。
        assert_eq!(windows[0].window, WindowKind::FiveHour);
        assert_eq!(windows[0].label.as_deref(), Some("通用积分池"));
        assert_eq!(windows[0].used, Some(33586.3));
        assert_eq!(windows[0].limit, Some(60000.0));
        assert_eq!(windows[0].unit.as_deref(), Some("积分"));
        assert_eq!(windows[1].window, WindowKind::Weekly);
        assert_eq!(windows[1].label.as_deref(), Some("通用积分池"));
        assert_eq!(windows[2].window, WindowKind::FiveHour);
        assert_eq!(windows[2].label.as_deref(), Some("Flash-Lite积分池"));
        assert_eq!(windows[2].remaining_percent_value(), Some(0.01));
    }

    #[test]
    fn reset_at_seconds_string_becomes_utc() {
        let body = r#"{ "pools": [ { "name": "通用积分池",
          "window_5h": { "limit": "60000", "used": "0", "remaining": "60000", "reset_at": "1788365437" } } ] }"#;
        let FetchOutput::Quota { windows, .. } = parse_pool_usage(body).unwrap() else {
            panic!("expected quota")
        };
        assert_eq!(
            windows[0].resets_at.unwrap(),
            chrono::DateTime::parse_from_rfc3339("2026-09-02T16:10:37Z").unwrap()
        );
    }

    #[test]
    fn pool_without_windows_is_skipped() {
        let body = r#"{ "pools": [
            { "name": "空池" },
            { "name": "残缺池", "window_5h": { "limit": "100" } }
        ] }"#;
        let FetchOutput::Quota { windows, .. } = parse_pool_usage(body).unwrap() else {
            panic!("expected quota")
        };
        assert!(windows.is_empty());
    }

    #[test]
    fn invalid_json_is_parse_error() {
        assert!(parse_pool_usage("not json").is_err());
    }
}

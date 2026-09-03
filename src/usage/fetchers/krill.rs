//! Krill（krill-code.com 控制台）用量查询。
//!
//! - 模型 API host（`api-slb.krill-ai.net` / `api.krill-ai.net` /
//!   `api.cdn-krill-ai.com` / `api-slb.krill-code.net`）都分发到这里；控制台接口固定走
//!   `https://www.krill-code.com`，模型 Base URL 只用于识别 Krill Provider。
//! - `GET /api/subscription` 返回统一账户总览（钱包/福利/套餐/计次配额）。
//!   展示模式只由 `provider.billing_mode` 决定：0=按量（credit+welfare 合计为主余额，
//!   另给钱包/福利明细），1=订阅（只处理 `status=active` 的套餐，逐份产出带套餐名
//!   label 的窗口；无 active 套餐或无可解析窗口时 windows 为空）。
//! - JWT 状态机：`extra.jwt` 非空先直接查 subscription；只有 HTTP 401/403 或
//!   可解析的业务 code=401 才用 email/password 登录换取新 JWT；登录成功后先把新
//!   JWT 合并回写加密 extra（保留其他字段），再用它重试 subscription 一次。其余
//!   （网络、5xx、非认证业务错、JSON/字段解析错误）一律不登录。TOTP 明确报鉴权失败。

use chrono::{DateTime, Utc};
use serde_json::Value;

use super::{Credentials, num, reset_ts, snippet};
use crate::usage::error::UsageError;
use crate::usage::http::{HttpReply, UsageHttp};
use crate::usage::types::{BalanceItem, FetchOutput, QuotaWindow, WindowKind};

/// 控制台固定端点。
const CONSOLE_BASE: &str = "https://www.krill-code.com";
const SUBSCRIPTION_PATH: &str = "/api/subscription";
const LOGIN_PATH: &str = "/api/auth/login";

/// 一次 HTTP 调用是否需要触发登录。仅在「明确认证失败」时返回 true：
/// HTTP 401/403，或 HTTP 200 且能解析出业务 code=401。网络/5xx/解析失败不算。
fn login_reason(reply: &HttpReply) -> bool {
    reply.status == 401 || reply.status == 403 || {
        if reply.status != 200 {
            return false;
        }
        serde_json::from_str::<Value>(&reply.body)
            .ok()
            .and_then(|v| v.get("code").and_then(Value::as_i64))
            == Some(401)
    }
}

/// 使用 JWT 查询并按 billing_mode 归一化 subscription 响应。
pub async fn fetch_subscription(
    http: &UsageHttp,
    jwt: &str,
    billing_mode: i32,
) -> Result<FetchOutput, UsageError> {
    let reply = subscription_reply(http, jwt).await?;
    parse_subscription_by_mode(&reply.body, billing_mode)
}

/// 使用邮箱密码登录并返回新 JWT。调用方负责先持久化，再重试 subscription。
pub async fn login(http: &UsageHttp, creds: &Credentials<'_>) -> Result<String, UsageError> {
    let email = creds.require("email")?;
    let password = creds.require("password")?;
    let body = serde_json::json!({ "email": email, "password": password }).to_string();
    let reply = http
        .post_json(&format!("{CONSOLE_BASE}{LOGIN_PATH}"), &[], &body)
        .await?;
    if login_reason(&reply) {
        return Err(UsageError::Auth);
    }
    let value = ok_reply(&reply)?;
    login_token(&value, &reply.body)
}

/// 查询 subscription。HTTP 401/403 或业务 code=401 → Auth；其余非 200 报上游错误。
async fn subscription_reply(http: &UsageHttp, jwt: &str) -> Result<HttpReply, UsageError> {
    let reply = http
        .get(
            &format!("{CONSOLE_BASE}{SUBSCRIPTION_PATH}"),
            &[
                ("Authorization", format!("Bearer {jwt}")),
                ("Accept-Language", "zh-CN".to_string()),
            ],
        )
        .await?;
    if login_reason(&reply) {
        return Err(UsageError::Auth);
    }
    if reply.status != 200 {
        return Err(UsageError::Upstream(reply.status, snippet(&reply.body)));
    }
    Ok(reply)
}

/// HTTP 200 + body 为合法 JSON（业务判定交给调用方/解析函数）。
fn ok_reply(reply: &HttpReply) -> Result<Value, UsageError> {
    if reply.status != 200 {
        return Err(UsageError::Upstream(reply.status, snippet(&reply.body)));
    }
    serde_json::from_str(&reply.body)
        .map_err(|e| UsageError::Parse(format!("响应不是合法 JSON：{e}")))
}

/// 统一成功包裹判定：业务 code=401 → Auth；success!=true 或 code!=0 → 上游错误。
fn check_business(v: &Value, body: &str) -> Result<(), UsageError> {
    let code = v.get("code").and_then(Value::as_i64).unwrap_or(-1);
    if code == 401 {
        return Err(UsageError::Auth);
    }
    if v.get("success").and_then(Value::as_bool) != Some(true) || code != 0 {
        return Err(UsageError::Upstream(200, snippet(body)));
    }
    Ok(())
}

/// 提取登录响应中的 token。HTTP 200、success=true、code=0 且存在非空
/// `data.token` 才算成功；`requires_totp=true` 明确映射为鉴权失败。
fn login_token(v: &Value, body: &str) -> Result<String, UsageError> {
    if v.get("requires_totp").and_then(Value::as_bool) == Some(true) {
        return Err(UsageError::Auth);
    }
    check_business(v, body)?;
    v.get("data")
        .and_then(|d| d.get("token"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or(UsageError::Auth)
}

/// 从 body 解析出「成功响应」的 data 对象；失败按 check_business 语义返回。
fn success_data(body: &str) -> Result<Value, UsageError> {
    let v: Value = serde_json::from_str(body)
        .map_err(|e| UsageError::Parse(format!("响应不是合法 JSON：{e}")))?;
    check_business(&v, body)?;
    v.get("data")
        .filter(|d| d.is_object())
        .cloned()
        .ok_or_else(|| UsageError::Parse("成功响应缺少 data 对象".to_string()))
}

/// 按 billing_mode 解析 subscription 响应：0 → 按量余额，1 → 订阅窗口。
/// 纯函数，供单元测试直接覆盖四类窗口与降级路径。
pub fn parse_subscription_by_mode(
    body: &str,
    billing_mode: i32,
) -> Result<FetchOutput, UsageError> {
    if billing_mode == 1 {
        parse_subscription(body)
    } else {
        parse_balance(body)
    }
}

/// 按量余额：主项金额 = credit + welfare（USD），另给钱包/福利明细。
/// 数值兼容 JSON 数字与字符串；缺失字段视为解析失败而非默认零。
fn parse_balance(body: &str) -> Result<FetchOutput, UsageError> {
    let data = success_data(body)?;
    let credit = data
        .get("credit_balance_usd")
        .and_then(num)
        .ok_or_else(|| UsageError::Parse("缺少 credit_balance_usd".to_string()))?;
    let welfare = data
        .get("welfare_balance_usd")
        .and_then(num)
        .ok_or_else(|| UsageError::Parse("缺少 welfare_balance_usd".to_string()))?;
    Ok(FetchOutput::Balance {
        items: vec![
            BalanceItem {
                label: "可用总余额".to_string(),
                amount: credit + welfare,
                currency: Some("USD".to_string()),
                primary: true,
            },
            BalanceItem {
                label: "钱包余额".to_string(),
                amount: credit,
                currency: Some("USD".to_string()),
                primary: false,
            },
            BalanceItem {
                label: "福利余额".to_string(),
                amount: welfare,
                currency: Some("USD".to_string()),
                primary: false,
            },
        ],
    })
}

/// 订阅：只处理 `status=active` 的套餐，逐份产出窗口（label=plan.name）。
/// `request_count` 套餐在 subscriptions 里不产出重复窗口，只记名给账户级
/// `request_count_quota` 使用（只输出一次三窗）。无 active 套餐返回空 windows。
fn parse_subscription(body: &str) -> Result<FetchOutput, UsageError> {
    let data = success_data(body)?;
    let mut windows = Vec::new();
    let mut plan_name: Option<String> = None;
    let mut req_label: Option<String> = None;

    for sub in data
        .get("subscriptions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|s| s.get("status").and_then(Value::as_str) == Some("active"))
    {
        let Some(sub) = sub.as_object() else {
            continue;
        };
        let name = sub
            .get("plan")
            .and_then(|p| p.get("name"))
            .and_then(Value::as_str)
            .map(str::to_string);
        if plan_name.is_none() {
            plan_name = name.clone();
        }
        // 重置时间优先 quota.window_reset_at，回退 subscription_end_at；
        // 非法时间只省略 resets_at，不导致整个响应失败。
        let resets_at = sub
            .get("quota")
            .and_then(|q| q.get("window_reset_at"))
            .and_then(reset_ts)
            .or_else(|| sub.get("subscription_end_at").and_then(reset_ts));
        let billing_type = sub
            .get("plan")
            .and_then(|p| p.get("billing_type"))
            .and_then(Value::as_str);
        match billing_type {
            Some("usd_daily") => {
                if let Some(w) = daily_window(sub, name, resets_at) {
                    windows.push(w);
                }
            }
            Some("usd_weekly") => {
                if let Some(w) = weekly_window(sub, name, resets_at) {
                    windows.push(w);
                }
            }
            Some("usd_monthly") => {
                if let Some(w) = monthly_window(sub, name, resets_at) {
                    windows.push(w);
                }
            }
            Some("request_count") if req_label.is_none() => {
                req_label = name;
            }
            _ => {}
        }
    }

    // 账户级计次配额只输出一次，label = 计次套餐名（缺套餐名则跳过，避免无标签重复）。
    if let Some(label) = req_label
        && let Some(q) = data.get("request_count_quota")
    {
        for (used_key, limit_key, kind) in [
            ("used_5h", "limit_5h", WindowKind::FiveHour),
            ("used_weekly", "limit_weekly", WindowKind::Weekly),
            ("used_monthly", "limit_monthly", WindowKind::Monthly),
        ] {
            if let (Some(used), Some(limit)) = (
                q.get(used_key).and_then(num),
                q.get(limit_key).and_then(num),
            ) {
                windows.push(QuotaWindow {
                    label: Some(label.clone()),
                    ..QuotaWindow::from_used_limit(kind, used, limit, reset_of_cycle(q), None)
                });
            }
        }
    }

    Ok(FetchOutput::Quota {
        plan: plan_name,
        windows,
    })
}

/// 计次配额周期重置时间。
fn reset_of_cycle(q: &Value) -> Option<DateTime<Utc>> {
    reset_ts(q.get("cycle_end")?)
}

/// usd_daily：quota.used_usd / quota.daily_limit_usd → Daily。
fn daily_window(
    sub: &serde_json::Map<String, Value>,
    label: Option<String>,
    resets_at: Option<DateTime<Utc>>,
) -> Option<QuotaWindow> {
    let quota = sub.get("quota")?;
    let used = quota.get("used_usd").and_then(num)?;
    let limit = quota.get("daily_limit_usd").and_then(num)?;
    Some(QuotaWindow {
        label,
        ..QuotaWindow::from_used_limit(WindowKind::Daily, used, limit, resets_at, Some("USD"))
    })
}

/// usd_weekly / usd_monthly：文档未单独确认周/月套餐内 quota 的周期 used/limit 字段，
/// 保守使用顶层 total_used_usd / total_limit_usd（Krill 的额度周期语义）。
fn weekly_window(
    sub: &serde_json::Map<String, Value>,
    label: Option<String>,
    resets_at: Option<DateTime<Utc>>,
) -> Option<QuotaWindow> {
    let used = sub.get("total_used_usd").and_then(num)?;
    let limit = sub.get("total_limit_usd").and_then(num)?;
    Some(QuotaWindow {
        label,
        ..QuotaWindow::from_used_limit(WindowKind::Weekly, used, limit, resets_at, Some("USD"))
    })
}

fn monthly_window(
    sub: &serde_json::Map<String, Value>,
    label: Option<String>,
    resets_at: Option<DateTime<Utc>>,
) -> Option<QuotaWindow> {
    let used = sub.get("total_used_usd").and_then(num)?;
    let limit = sub.get("total_limit_usd").and_then(num)?;
    Some(QuotaWindow {
        label,
        ..QuotaWindow::from_used_limit(WindowKind::Monthly, used, limit, resets_at, Some("USD"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(status: u16, body: &str) -> HttpReply {
        HttpReply {
            status,
            body: body.to_string(),
        }
    }

    #[test]
    fn login_reason_only_for_auth_failures() {
        // HTTP 401/403 → 需要登录。
        assert!(login_reason(&reply(401, "{}")));
        assert!(login_reason(&reply(403, "{}")));
        // HTTP 200 + 可解析业务 code=401 → 需要登录。
        assert!(login_reason(&reply(
            200,
            r#"{"success":false,"code":401,"message":"invalid credentials"}"#
        )));
        // 网络/5xx/非认证业务错/解析失败 → 不登录。
        assert!(!login_reason(&reply(500, "{}")));
        assert!(!login_reason(&reply(200, "{}")));
        assert!(!login_reason(&reply(
            200,
            r#"{"success":false,"code":403,"message":"no permission"}"#
        )));
        assert!(!login_reason(&reply(200, "not json")));
    }

    #[test]
    fn login_success_requires_http200_success_code0_token() {
        let ok = serde_json::json!({ "success": true, "code": 0, "data": { "token": "jwt-1" } });
        assert_eq!(login_token(&ok, "").unwrap(), "jwt-1");
        let no_token = serde_json::json!({ "success": true, "code": 0, "data": {} });
        assert!(matches!(login_token(&no_token, ""), Err(UsageError::Auth)));
        let bad_code =
            serde_json::json!({ "success": true, "code": 7, "data": { "token": "jwt" } });
        assert!(matches!(
            login_token(&bad_code, ""),
            Err(UsageError::Upstream(200, _))
        ));
        let requires_totp = serde_json::json!({ "requires_totp": true });
        assert!(matches!(
            login_token(&requires_totp, ""),
            Err(UsageError::Auth)
        ));
    }

    #[test]
    fn business_code_401_maps_to_auth() {
        let body = r#"{"success":false,"code":401,"message":"invalid credentials","data":{"type":"unauthorized_error"}}"#;
        assert!(matches!(
            parse_subscription_by_mode(body, 1),
            Err(UsageError::Auth)
        ));
    }

    #[test]
    fn non_auth_business_error_is_upstream() {
        let body = r#"{"success":false,"code":5000,"message":"server error"}"#;
        assert!(matches!(
            parse_subscription_by_mode(body, 1),
            Err(UsageError::Upstream(200, _))
        ));
    }

    #[test]
    fn payg_string_amounts_sum_to_primary_balance() {
        let body = r#"{
          "success": true, "code": 0,
          "data": {
            "subscriptions": [],
            "summary": { "subscription_end_at": null },
            "credit_balance_usd": "24.938388",
            "welfare_balance_usd": "0",
            "request_count_quota": null
          }
        }"#;
        let FetchOutput::Balance { items } = parse_subscription_by_mode(body, 0).unwrap() else {
            panic!("expected balance")
        };
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].label, "可用总余额");
        assert_eq!(items[0].amount, 24.938388);
        assert_eq!(items[0].currency.as_deref(), Some("USD"));
        assert!(items[0].primary);
        assert_eq!(items[1].label, "钱包余额");
        assert_eq!(items[1].amount, 24.938388);
        assert!(!items[1].primary);
        assert_eq!(items[2].label, "福利余额");
        assert_eq!(items[2].amount, 0.0);
    }

    #[test]
    fn payg_missing_balance_field_is_parse_error_not_zero() {
        let body = r#"{ "success": true, "code": 0, "data": { "subscriptions": [], "credit_balance_usd": "1", "request_count_quota": null } }"#;
        assert!(matches!(
            parse_subscription_by_mode(body, 0),
            Err(UsageError::Parse(_))
        ));
    }

    #[test]
    fn quota_plan_parses_active_subscription_window() {
        let body = r#"{
          "success": true, "code": 0,
          "data": {
            "subscriptions": [
              {
                "subscription_id": "s1",
                "status": "active",
                "subscription_start_at": "2026-09-03T06:22:00Z",
                "subscription_end_at": "2026-10-03T06:22:00Z",
                "plan": { "name": "标准月套餐", "billing_type": "usd_monthly" },
                "quota": { "used_usd": "0", "daily_limit_usd": "0", "window_start_at": "2026-09-03T06:22:00Z", "window_reset_at": "2026-10-03T06:22:00Z" },
                "total_used_usd": "12.34",
                "total_limit_usd": "100",
                "total_remaining_usd": "87.66"
              }
            ],
            "summary": {},
            "credit_balance_usd": "0",
            "welfare_balance_usd": "0",
            "request_count_quota": null
          }
        }"#;
        let FetchOutput::Quota { plan, windows } = parse_subscription_by_mode(body, 1).unwrap()
        else {
            panic!("expected quota")
        };
        assert_eq!(plan.as_deref(), Some("标准月套餐"));
        assert_eq!(windows.len(), 1);
        let w = &windows[0];
        assert_eq!(w.window, WindowKind::Monthly);
        assert!(w.available);
        assert_eq!(w.label.as_deref(), Some("标准月套餐"));
        assert_eq!(w.used, Some(12.34));
        assert_eq!(w.limit, Some(100.0));
        assert_eq!(
            w.resets_at.unwrap(),
            DateTime::parse_from_rfc3339("2026-10-03T06:22:00Z").unwrap()
        );
    }

    #[test]
    fn quota_daily_weekly_and_request_count_windows() {
        let body = r#"{
          "success": true, "code": 0,
          "data": {
            "subscriptions": [
              {
                "subscription_id": "s1",
                "status": "active",
                "plan": { "name": "日套餐", "billing_type": "usd_daily" },
                "quota": { "used_usd": "3", "daily_limit_usd": "10", "window_reset_at": "2026-09-04T06:22:00Z" }
              },
              {
                "subscription_id": "s2",
                "status": "active",
                "plan": { "name": "周套餐", "billing_type": "usd_weekly" },
                "quota": { "used_usd": "7", "daily_limit_usd": "0", "window_reset_at": "2026-09-07T06:22:00Z" },
                "total_used_usd": "7",
                "total_limit_usd": "30",
                "total_remaining_usd": "23"
              },
              {
                "subscription_id": "s3",
                "status": "active",
                "plan": { "name": "计次套餐", "billing_type": "request_count" },
                "quota": {}
              },
              {
                "subscription_id": "frozen",
                "status": "frozen",
                "plan": { "name": "冻结套餐", "billing_type": "usd_monthly" },
                "quota": {},
                "total_used_usd": "0",
                "total_limit_usd": "0"
              }
            ],
            "summary": {},
            "credit_balance_usd": "0",
            "welfare_balance_usd": "0",
            "request_count_quota": {
              "used_5h": 1, "limit_5h": 100,
              "used_weekly": 5, "limit_weekly": 1000,
              "used_monthly": 20, "limit_monthly": 3000,
              "cycle_start": "2026-09-03T00:00:00Z",
              "cycle_end": "2026-10-03T00:00:00Z"
            }
          }
        }"#;
        let FetchOutput::Quota { plan, windows } = parse_subscription_by_mode(body, 1).unwrap()
        else {
            panic!("expected quota")
        };
        assert_eq!(plan.as_deref(), Some("日套餐"));
        // 日套餐 Daily + 周套餐 Weekly + 计次套餐账户级 3 窗 = 5 条；frozen 被忽略。
        assert_eq!(windows.len(), 5);

        let daily = windows
            .iter()
            .find(|w| w.window == WindowKind::Daily && w.label.as_deref() == Some("日套餐"))
            .unwrap();
        assert_eq!(daily.used, Some(3.0));
        assert_eq!(daily.limit, Some(10.0));
        assert_eq!(
            daily.resets_at.unwrap(),
            DateTime::parse_from_rfc3339("2026-09-04T06:22:00Z").unwrap()
        );

        let weekly_plan = windows
            .iter()
            .find(|w| w.window == WindowKind::Weekly && w.label.as_deref() == Some("周套餐"))
            .unwrap();
        assert_eq!(weekly_plan.used, Some(7.0));
        assert_eq!(weekly_plan.limit, Some(30.0));

        // 账户级计次配额：5h/周/月三窗只输出一次，label = 计次套餐名。
        let five_hour: Vec<_> = windows
            .iter()
            .filter(|w| w.window == WindowKind::FiveHour && w.available)
            .collect();
        assert_eq!(five_hour.len(), 1);
        assert_eq!(five_hour[0].label.as_deref(), Some("计次套餐"));
        assert_eq!(five_hour[0].used, Some(1.0));
        assert_eq!(five_hour[0].limit, Some(100.0));
        let rc_weekly = windows
            .iter()
            .find(|w| w.window == WindowKind::Weekly && w.label.as_deref() == Some("计次套餐"))
            .unwrap();
        assert_eq!(rc_weekly.limit, Some(1000.0));
        let rc_monthly = windows
            .iter()
            .find(|w| w.window == WindowKind::Monthly && w.label.as_deref() == Some("计次套餐"))
            .unwrap();
        assert_eq!(rc_monthly.limit, Some(3000.0));
        // 计次配额重置时间取 request_count_quota.cycle_end。
        assert_eq!(
            five_hour[0].resets_at.unwrap(),
            DateTime::parse_from_rfc3339("2026-10-03T00:00:00Z").unwrap()
        );
    }

    #[test]
    fn quota_no_active_or_missing_fields_returns_empty_windows() {
        // 只有 frozen/过期套餐 → 无窗口。
        let frozen = r#"{
          "success": true, "code": 0,
          "data": {
            "subscriptions": [
              { "subscription_id": "f", "status": "frozen", "plan": { "name": "冻结", "billing_type": "usd_monthly" }, "quota": {}, "total_used_usd": "0", "total_limit_usd": "100" }
            ],
            "summary": {}, "credit_balance_usd": "0", "welfare_balance_usd": "0",
            "request_count_quota": null
          }
        }"#;
        let FetchOutput::Quota { windows, .. } = parse_subscription_by_mode(frozen, 1).unwrap()
        else {
            panic!("expected quota")
        };
        assert!(windows.is_empty());

        // active 但缺 used/limit 字段 → 跳过该窗口。
        let incomplete = r#"{
          "success": true, "code": 0,
          "data": {
            "subscriptions": [
              { "subscription_id": "a", "status": "active", "plan": { "name": "残缺日", "billing_type": "usd_daily" }, "quota": { "used_usd": "1" } },
              { "subscription_id": "b", "status": "active", "plan": { "name": "残缺月", "billing_type": "usd_monthly" }, "quota": {}, "total_used_usd": "5" }
            ],
            "summary": {}, "credit_balance_usd": "0", "welfare_balance_usd": "0",
            "request_count_quota": null
          }
        }"#;
        let FetchOutput::Quota { windows, .. } = parse_subscription_by_mode(incomplete, 1).unwrap()
        else {
            panic!("expected quota")
        };
        assert!(windows.is_empty());
    }

    #[test]
    fn request_count_quota_without_active_request_count_plan_is_skipped() {
        // 账户级计次配额存在但没有 active 的 request_count 套餐名 → 无法打 label，跳过。
        let body = r#"{
          "success": true, "code": 0,
          "data": {
            "subscriptions": [
              { "subscription_id": "s1", "status": "active", "plan": { "name": "月套餐", "billing_type": "usd_monthly" }, "quota": {}, "total_used_usd": "1", "total_limit_usd": "10" }
            ],
            "summary": {}, "credit_balance_usd": "0", "welfare_balance_usd": "0",
            "request_count_quota": { "used_5h": 1, "limit_5h": 10 }
          }
        }"#;
        let FetchOutput::Quota { windows, .. } = parse_subscription_by_mode(body, 1).unwrap()
        else {
            panic!("expected quota")
        };
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].window, WindowKind::Monthly);
    }

    #[test]
    fn invalid_reset_time_only_omits_resets_at() {
        let body = r#"{
          "success": true, "code": 0,
          "data": {
            "subscriptions": [
              { "subscription_id": "s1", "status": "active", "plan": { "name": "月套餐", "billing_type": "usd_monthly" }, "quota": { "window_reset_at": "not-a-time" }, "total_used_usd": "1", "total_limit_usd": "10" }
            ],
            "summary": {}, "credit_balance_usd": "0", "welfare_balance_usd": "0",
            "request_count_quota": null
          }
        }"#;
        let FetchOutput::Quota { windows, .. } = parse_subscription_by_mode(body, 1).unwrap()
        else {
            panic!("expected quota")
        };
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].window, WindowKind::Monthly);
        assert!(windows[0].resets_at.is_none());
        assert!(windows[0].available);
    }
}

//! API key 直查的订阅制 fetcher：
//! OpenCode Go / Kimi For Coding / 智谱(Z.AI) / MiniMax / ZenMux / Command Code。

use serde_json::Value;

use super::{Credentials, num, reset_ts, reset_ts_of, snippet};
use crate::usage::error::UsageError;
use crate::usage::http::{HttpReply, UsageHttp, ensure_not_auth_error, parse_json};
use crate::usage::types::{FetchOutput, QuotaWindow, WindowKind, empty_windows, set_window, ts_ms};

// ── OpenCode Go ─────────────────────────────────────────────
// GET https://opencode.ai/zen/go/v1/usage（只认 Bearer）
// 响应 usage.rolling/weekly/monthly = { status, percent(已用), resetsAt }
// resetsAt 在 percent=0 时是占位值，不代表真实重置时间。

pub async fn fetch_opencode_go(
    http: &UsageHttp,
    creds: &Credentials<'_>,
) -> Result<FetchOutput, UsageError> {
    let reply = http
        .get(
            "https://opencode.ai/zen/go/v1/usage",
            &[(
                "Authorization",
                format!("Bearer {}", creds.api_key_required()?),
            )],
        )
        .await?;
    ensure_not_auth_error(&reply)?;
    if reply.status != 200 {
        return Err(UsageError::Upstream(reply.status, snippet(&reply.body)));
    }
    parse_opencode_go(&reply.body)
}

fn parse_opencode_go(body: &str) -> Result<FetchOutput, UsageError> {
    let v: Value = serde_json::from_str(body)
        .map_err(|e| UsageError::Parse(format!("响应不是合法 JSON：{e}")))?;
    let usage = v
        .get("usage")
        .ok_or_else(|| UsageError::Parse("缺少 usage 字段".to_string()))?;

    let mut windows = empty_windows();
    for (key, kind) in [
        ("rolling", WindowKind::FiveHour),
        ("weekly", WindowKind::Weekly),
        ("monthly", WindowKind::Monthly),
    ] {
        if let Some(entry) = usage.get(key)
            && let Some(percent) = entry.get("percent").and_then(num)
        {
            // percent=0 时 resetsAt 是占位值（now+窗口），不展示。
            let resets_at = if percent > 0.0 {
                entry.get("resetsAt").and_then(reset_ts)
            } else {
                None
            };
            set_window(
                &mut windows,
                QuotaWindow::from_used_percent(kind, percent, resets_at),
            );
        }
    }
    Ok(FetchOutput::Quota {
        plan: None,
        windows,
    })
}

// ── Kimi For Coding ─────────────────────────────────────────
// GET https://api.kimi.com/coding/v1/usages（Bearer）
// usage = 周限额（数值为字符串）；limits[] 首个条目 detail = 5h 窗口；无月窗。

pub async fn fetch_kimi(
    http: &UsageHttp,
    creds: &Credentials<'_>,
) -> Result<FetchOutput, UsageError> {
    let reply = http
        .get(
            "https://api.kimi.com/coding/v1/usages",
            &[(
                "Authorization",
                format!("Bearer {}", creds.api_key_required()?),
            )],
        )
        .await?;
    ensure_not_auth_error(&reply)?;
    if reply.status != 200 {
        return Err(UsageError::Upstream(reply.status, snippet(&reply.body)));
    }
    parse_kimi(&reply.body)
}

fn parse_kimi(body: &str) -> Result<FetchOutput, UsageError> {
    let v: Value = serde_json::from_str(body)
        .map_err(|e| UsageError::Parse(format!("响应不是合法 JSON：{e}")))?;

    let mut windows = empty_windows();
    if let Some(detail) = v
        .get("limits")
        .and_then(Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(|entry| entry.get("detail"))
    {
        let window = window_from_detail(WindowKind::FiveHour, detail);
        if let Some(window) = window {
            set_window(&mut windows, window);
        }
    }
    if let Some(usage) = v.get("usage") {
        let window = window_from_detail(WindowKind::Weekly, usage);
        if let Some(window) = window {
            set_window(&mut windows, window);
        }
    }
    Ok(FetchOutput::Quota {
        plan: None,
        windows,
    })
}

/// Kimi 的 limit/used/remaining 结构 → 窗口（数值是字符串）。
fn window_from_detail(kind: WindowKind, detail: &Value) -> Option<QuotaWindow> {
    let limit = detail.get("limit").and_then(num)?;
    let used = detail
        .get("used")
        .and_then(num)
        .or_else(|| detail.get("remaining").and_then(num).map(|r| limit - r))?;
    let resets_at = reset_ts_of(detail, &["resetTime", "resetAt", "reset_time", "reset_at"]);
    Some(QuotaWindow::from_used_limit(
        kind, used, limit, resets_at, None,
    ))
}

// ── 智谱 GLM Coding Plan / Z.AI ─────────────────────────────
// GET https://{host}/api/monitor/usage/quota/limit（Authorization 直放 key，不加 Bearer）
// data.limits[]：type ∈ TOKENS_LIMIT/CREDIT_LIMIT；unit:3 → 5h 窗，unit:6 → 周窗。
// 注意不能按 nextResetTime 排序猜窗口（周期末尾周窗可能先重置）。无月窗。

pub async fn fetch_zhipu(
    http: &UsageHttp,
    creds: &Credentials<'_>,
    host: &str,
) -> Result<FetchOutput, UsageError> {
    let url = format!("https://{host}/api/monitor/usage/quota/limit");
    let reply = http
        .get(
            &url,
            &[("Authorization", creds.api_key_required()?.to_string())],
        )
        .await?;
    ensure_not_auth_error(&reply)?;
    if reply.status != 200 {
        return Err(UsageError::Upstream(reply.status, snippet(&reply.body)));
    }
    parse_zhipu(&reply.body)
}

fn parse_zhipu(body: &str) -> Result<FetchOutput, UsageError> {
    let v: Value = serde_json::from_str(body)
        .map_err(|e| UsageError::Parse(format!("响应不是合法 JSON：{e}")))?;
    if v.get("success").and_then(Value::as_bool) == Some(false) {
        let msg = v.get("msg").and_then(Value::as_str).unwrap_or("未知错误");
        return Err(UsageError::Upstream(200, msg.to_string()));
    }
    let data = v
        .get("data")
        .ok_or_else(|| UsageError::Parse("缺少 data 字段".to_string()))?;
    let plan = data
        .get("level")
        .and_then(Value::as_str)
        .map(str::to_string);

    let mut windows = empty_windows();
    if let Some(limits) = data.get("limits").and_then(Value::as_array) {
        for entry in limits {
            let limit_type = entry
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_ascii_uppercase();
            if limit_type != "TOKENS_LIMIT" && limit_type != "CREDIT_LIMIT" {
                continue;
            }
            let kind = match entry.get("unit").and_then(Value::as_i64) {
                Some(3) => WindowKind::FiveHour,
                Some(6) => WindowKind::Weekly,
                _ => continue,
            };
            let Some(percentage) = entry.get("percentage").and_then(num) else {
                continue;
            };
            let resets_at = entry
                .get("nextResetTime")
                .and_then(Value::as_i64)
                .and_then(ts_ms);
            set_window(
                &mut windows,
                QuotaWindow::from_used_percent(kind, percentage, resets_at),
            );
        }
    }
    Ok(FetchOutput::Quota { plan, windows })
}

// ── MiniMax Coding Plan / Token Plan ────────────────────────
// 先 GET /v1/token_plan/remains，失败回退 /v1/api/openplatform/coding_plan/remains。
// 百分比是【剩余】；5h 窗恒有；周桶仅 current_weekly_status == 1 时存在。无月窗。

pub async fn fetch_minimax(
    http: &UsageHttp,
    creds: &Credentials<'_>,
    host: &str,
) -> Result<FetchOutput, UsageError> {
    let auth = format!("Bearer {}", creds.api_key_required()?);
    let endpoints = [
        format!("https://{host}/v1/token_plan/remains"),
        format!("https://{host}/v1/api/openplatform/coding_plan/remains"),
    ];
    let mut last_err = None;
    for url in endpoints {
        let reply = http.get(&url, &[("Authorization", auth.clone())]).await?;
        ensure_not_auth_error(&reply)?;
        match parse_minimax(&reply) {
            Ok(output) => return Ok(output),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or(UsageError::Parse("无可用端点".to_string())))
}

fn parse_minimax(reply: &HttpReply) -> Result<FetchOutput, UsageError> {
    if reply.status != 200 {
        return Err(UsageError::Upstream(reply.status, snippet(&reply.body)));
    }
    let v = parse_json(reply)?;
    let status_code = v
        .get("base_resp")
        .and_then(|b| b.get("status_code"))
        .and_then(Value::as_i64)
        .unwrap_or(-1);
    if status_code != 0 {
        let msg = v
            .get("base_resp")
            .and_then(|b| b.get("status_msg"))
            .and_then(Value::as_str)
            .unwrap_or("未知错误");
        return Err(UsageError::Upstream(200, msg.to_string()));
    }
    let remains = v
        .get("model_remains")
        .and_then(Value::as_array)
        .ok_or_else(|| UsageError::Parse("缺少 model_remains 字段".to_string()))?;
    // 编程套餐取 general，跳过 video 等；找不到则取第一条。
    let entry = remains
        .iter()
        .find(|m| m.get("model_name").and_then(Value::as_str) == Some("general"))
        .or_else(|| remains.first())
        .ok_or_else(|| UsageError::Parse("model_remains 为空".to_string()))?;

    let mut windows = empty_windows();
    if let Some(remaining) = entry
        .get("current_interval_remaining_percent")
        .and_then(num)
    {
        let resets_at = entry
            .get("end_time")
            .and_then(Value::as_i64)
            .and_then(ts_ms);
        set_window(
            &mut windows,
            QuotaWindow::from_remaining_percent(WindowKind::FiveHour, remaining, resets_at),
        );
    }
    // 周桶：status 3 表示该套餐无周限额，不展示。
    if entry.get("current_weekly_status").and_then(Value::as_i64) == Some(1)
        && let Some(remaining) = entry.get("current_weekly_remaining_percent").and_then(num)
    {
        let resets_at = entry
            .get("weekly_end_time")
            .and_then(Value::as_i64)
            .and_then(ts_ms);
        set_window(
            &mut windows,
            QuotaWindow::from_remaining_percent(WindowKind::Weekly, remaining, resets_at),
        );
    }
    Ok(FetchOutput::Quota {
        plan: None,
        windows,
    })
}

// ── ZenMux ──────────────────────────────────────────────────
// GET https://zenmux.ai/api/v1/management/subscription/detail（Bearer Management key）
// usage_percentage 是 0–1 小数；月窗为固定 cap，可能无实时用量。

pub async fn fetch_zenmux(
    http: &UsageHttp,
    creds: &Credentials<'_>,
) -> Result<FetchOutput, UsageError> {
    let reply = http
        .get(
            "https://zenmux.ai/api/v1/management/subscription/detail",
            &[(
                "Authorization",
                format!("Bearer {}", creds.api_key_required()?),
            )],
        )
        .await?;
    ensure_not_auth_error(&reply)?;
    if reply.status != 200 {
        return Err(UsageError::Upstream(reply.status, snippet(&reply.body)));
    }
    parse_zenmux(&reply.body)
}

fn parse_zenmux(body: &str) -> Result<FetchOutput, UsageError> {
    let v: Value = serde_json::from_str(body)
        .map_err(|e| UsageError::Parse(format!("响应不是合法 JSON：{e}")))?;
    if v.get("success").and_then(Value::as_bool) == Some(false) {
        return Err(UsageError::Upstream(200, snippet(body)));
    }
    let data = v
        .get("data")
        .ok_or_else(|| UsageError::Parse("缺少 data 字段".to_string()))?;
    let plan = data
        .get("plan")
        .and_then(|p| p.get("tier"))
        .and_then(Value::as_str)
        .map(str::to_string);

    let mut windows = empty_windows();
    for (key, kind) in [
        ("quota_5_hour", WindowKind::FiveHour),
        ("quota_7_day", WindowKind::Weekly),
        ("quota_monthly", WindowKind::Monthly),
    ] {
        let Some(quota) = data.get(key) else { continue };
        let resets_at = quota.get("resets_at").and_then(reset_ts);
        let window = if let Some(p) = quota.get("usage_percentage").and_then(num) {
            let mut w = QuotaWindow::from_used_percent(kind, p * 100.0, resets_at);
            w.used = quota.get("used_flows").and_then(num);
            w.limit = quota.get("max_flows").and_then(num);
            if w.used.is_some() || w.limit.is_some() {
                w.unit = Some("flows".to_string());
            }
            w
        } else if let (Some(used), Some(limit)) = (
            quota.get("used_flows").and_then(num),
            quota.get("max_flows").and_then(num),
        ) {
            QuotaWindow::from_used_limit(kind, used, limit, resets_at, Some("flows"))
        } else {
            continue; // 月窗固定 cap、无实时用量时保持 unavailable
        };
        set_window(&mut windows, window);
    }
    Ok(FetchOutput::Quota { plan, windows })
}

// ── Command Code ────────────────────────────────────────────
// whoami 拿组织 ID（无组织账号 org 为 null，省略 orgId 参数）→ credits + subscriptions。
// credits 没有 remaining 字段：剩余 = cap − used；resetAt 是毫秒时间戳。无月窗。

const COMMAND_CODE_BASE: &str = "https://api.commandcode.ai";

pub async fn fetch_command_code(
    http: &UsageHttp,
    creds: &Credentials<'_>,
) -> Result<FetchOutput, UsageError> {
    let auth = format!("Bearer {}", creds.api_key_required()?);

    let whoami = http
        .get(
            &format!("{COMMAND_CODE_BASE}/alpha/whoami"),
            &[("Authorization", auth.clone())],
        )
        .await?;
    ensure_not_auth_error(&whoami)?;
    if whoami.status != 200 {
        return Err(UsageError::Upstream(whoami.status, snippet(&whoami.body)));
    }
    let whoami_json = parse_json(&whoami)?;
    let org_id = whoami_json
        .get("org")
        .and_then(|o| o.get("id"))
        .and_then(Value::as_str);

    let org_query = org_id.map(|id| format!("?orgId={id}")).unwrap_or_default();
    let credits = http
        .get(
            &format!("{COMMAND_CODE_BASE}/alpha/billing/credits{org_query}"),
            &[("Authorization", auth.clone())],
        )
        .await?;
    ensure_not_auth_error(&credits)?;
    if credits.status != 200 {
        return Err(UsageError::Upstream(credits.status, snippet(&credits.body)));
    }

    // subscriptions 用于套餐名与「本期已用」计算；失败不阻塞窗口数据。
    let subscription = match http
        .get(
            &format!("{COMMAND_CODE_BASE}/alpha/billing/subscriptions{org_query}"),
            &[("Authorization", auth.clone())],
        )
        .await
    {
        Ok(reply) if reply.status == 200 => parse_json(&reply).ok().map(|v| {
            // 实测有 success/data 包装，文档样例为裸 data，两种都兼容。
            let data = v.get("data").unwrap_or(&v);
            (
                data.get("planId")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                data.get("currentPeriodStart")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            )
        }),
        _ => None,
    };
    let (plan, period_start) = match subscription {
        Some((plan, start)) => (plan, start),
        None => (None, None),
    };

    // 本期已用（USD）：以订阅周期起始作为 since；失败只影响月窗总额。
    let total_cost = match &period_start {
        Some(since) => match http
            .get(
                &format!(
                    "{COMMAND_CODE_BASE}/alpha/usage/summary?since={}",
                    urlencode(since)
                ),
                &[("Authorization", auth)],
            )
            .await
        {
            Ok(reply) if reply.status == 200 => parse_json(&reply)
                .ok()
                .and_then(|v| v.get("totalCost").and_then(num)),
            _ => None,
        },
        None => None,
    };

    parse_command_code_credits(&credits.body, plan, total_cost)
}

/// usage/summary 的 since 必须是 ISO 8601；简单 query 编码即可。
fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b':' | b'Z' | b'T')
        {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn parse_command_code_credits(
    body: &str,
    plan: Option<String>,
    total_cost: Option<f64>,
) -> Result<FetchOutput, UsageError> {
    let v: Value = serde_json::from_str(body)
        .map_err(|e| UsageError::Parse(format!("响应不是合法 JSON：{e}")))?;
    let limits = v
        .get("windowLimits")
        .ok_or_else(|| UsageError::Parse("缺少 windowLimits 字段".to_string()))?;

    let mut windows = empty_windows();
    for (key, kind) in [
        ("fiveHour", WindowKind::FiveHour),
        ("weekly", WindowKind::Weekly),
    ] {
        let Some(entry) = limits.get(key) else {
            continue;
        };
        let (Some(used), Some(cap)) = (
            entry.get("used").and_then(num),
            entry.get("cap").and_then(num),
        ) else {
            continue;
        };
        let resets_at = entry.get("resetAt").and_then(Value::as_i64).and_then(ts_ms);
        set_window(
            &mut windows,
            QuotaWindow::from_used_limit(kind, used, cap, resets_at, Some("USD")),
        );
    }

    // 月窗：monthlyCredits 是「本月剩余」（USD），无总额字段；
    // 月总额 = monthlyCredits + 本期已用（usage/summary.totalCost）。
    if let Some(remaining) = v
        .get("credits")
        .and_then(|c| c.get("monthlyCredits"))
        .and_then(num)
        && let Some(total) = total_cost.map(|used| remaining + used)
        && total > 0.0
    {
        set_window(
            &mut windows,
            QuotaWindow::from_used_limit(
                WindowKind::Monthly,
                total - remaining,
                total,
                None,
                Some("USD"),
            ),
        );
    }

    Ok(FetchOutput::Quota { plan, windows })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opencode_go_three_windows() {
        let body = r#"{
          "usage": {
            "rolling": { "status": "ok", "percent": 0,  "resetsAt": "2026-08-29T00:03:30.118Z" },
            "weekly":  { "status": "ok", "percent": 16, "resetsAt": "2026-08-31T00:00:00.118Z" },
            "monthly": { "status": "ok", "percent": 95, "resetsAt": "2026-09-14T08:53:55.118Z" }
          }
        }"#;
        let FetchOutput::Quota { windows, .. } = parse_opencode_go(body).unwrap() else {
            panic!("expected quota")
        };
        assert!(windows.iter().all(|w| w.available));
        assert_eq!(windows[0].used_percent, Some(0.0));
        // percent=0 时 resetsAt 是占位值，不展示
        assert_eq!(windows[0].resets_at, None);
        assert_eq!(windows[1].used_percent, Some(16.0));
        assert!(windows[1].resets_at.is_some());
        assert_eq!(windows[2].remaining_percent, Some(5.0));
    }

    #[test]
    fn kimi_string_numbers_and_no_monthly() {
        let body = r#"{
          "usage": { "limit": "100", "used": "25", "remaining": "75", "resetTime": "2026-09-01T00:00:00Z" },
          "limits": [
            { "scope": "5h", "detail": { "limit": "20", "used": "5", "remaining": "15", "resetTime": "2026-08-29T05:00:00Z" } }
          ]
        }"#;
        let FetchOutput::Quota { windows, .. } = parse_kimi(body).unwrap() else {
            panic!("expected quota")
        };
        assert!(windows[0].available);
        assert_eq!(windows[0].used_percent, Some(25.0));
        assert!(windows[1].available);
        assert_eq!(windows[1].used_percent, Some(25.0));
        // Kimi 无月窗
        assert!(!windows[2].available);
    }

    #[test]
    fn zhipu_window_by_unit_not_reset_time() {
        // 周窗（unit:6）的 nextResetTime 早于 5h 窗时也不能标反。
        let body = r#"{
          "success": true, "msg": "ok",
          "data": {
            "level": "pro",
            "limits": [
              { "type": "TOKENS_LIMIT", "unit": 6, "number": 7, "percentage": 10, "nextResetTime": 1700000000000 },
              { "type": "TOKENS_LIMIT", "unit": 3, "number": 5, "percentage": 42, "nextResetTime": 1700100000000 },
              { "type": "OTHER_LIMIT", "unit": 1, "number": 1, "percentage": 99 }
            ]
          }
        }"#;
        let FetchOutput::Quota { plan, windows } = parse_zhipu(body).unwrap() else {
            panic!("expected quota")
        };
        assert_eq!(plan.as_deref(), Some("pro"));
        assert_eq!(windows[0].window, WindowKind::FiveHour);
        assert_eq!(windows[0].used_percent, Some(42.0));
        assert_eq!(windows[1].used_percent, Some(10.0));
        assert!(!windows[2].available);
    }

    #[test]
    fn zhipu_business_error() {
        let body = r#"{ "success": false, "msg": "密钥无效" }"#;
        assert!(matches!(parse_zhipu(body), Err(UsageError::Upstream(_, _))));
    }

    #[test]
    fn minimax_remaining_percent_and_weekly_gate() {
        let body = r#"{
          "base_resp": { "status_code": 0, "status_msg": "success" },
          "model_remains": [
            { "model_name": "video", "current_interval_remaining_percent": 1.0 },
            {
              "model_name": "general",
              "current_interval_remaining_percent": 93.0,
              "end_time": 1700000000000,
              "current_weekly_status": 3,
              "current_weekly_remaining_percent": 100.0
            }
          ]
        }"#;
        let reply = HttpReply {
            status: 200,
            body: body.to_string(),
        };
        let FetchOutput::Quota { windows, .. } = parse_minimax(&reply).unwrap() else {
            panic!("expected quota")
        };
        // 取 general 条目；百分比是剩余，需换算
        assert_eq!(windows[0].remaining_percent, Some(93.0));
        assert_eq!(windows[0].used_percent, Some(7.0));
        // status=3 无周限额，不展示
        assert!(!windows[1].available);
        assert!(!windows[2].available);
    }

    #[test]
    fn minimax_business_error() {
        let body = r#"{ "base_resp": { "status_code": 1004, "status_msg": "invalid api key" } }"#;
        let reply = HttpReply {
            status: 200,
            body: body.to_string(),
        };
        assert!(matches!(
            parse_minimax(&reply),
            Err(UsageError::Upstream(_, _))
        ));
    }

    #[test]
    fn zenmux_percentage_zero_to_one_and_monthly_cap_only() {
        let body = r#"{
          "success": true,
          "data": {
            "plan": { "tier": "ultra" },
            "quota_5_hour": { "max_flows": 800, "used_flows": 57.2, "usage_percentage": 0.0715, "resets_at": "2026-03-24T08:35:09.000Z" },
            "quota_7_day": { "max_flows": 5000, "used_flows": 500, "usage_percentage": 0.1, "resets_at": "2026-03-31T00:00:00.000Z" },
            "quota_monthly": { "max_flows": 24000, "max_value_usd": 200 }
          }
        }"#;
        let FetchOutput::Quota { plan, windows } = parse_zenmux(body).unwrap() else {
            panic!("expected quota")
        };
        assert_eq!(plan.as_deref(), Some("ultra"));
        assert_eq!(windows[0].used_percent, Some(7.15));
        assert_eq!(windows[0].unit.as_deref(), Some("flows"));
        assert_eq!(windows[1].used_percent, Some(10.0));
        // 月窗只有固定 cap、无实时用量 → unavailable
        assert!(!windows[2].available);
    }

    #[test]
    fn command_code_windows_from_cap_minus_used() {
        let body = r#"{
          "credits": { "monthlyCredits": 69.04 },
          "windowLimits": {
            "fiveHour": { "used": 0.96, "cap": 14, "resetAt": 1787817057032 },
            "weekly":   { "used": 0.96, "cap": 35, "resetAt": 1788403857032 }
          }
        }"#;
        let FetchOutput::Quota { plan, windows } =
            parse_command_code_credits(body, Some("individual-goat".to_string()), Some(0.946))
                .unwrap()
        else {
            panic!("expected quota")
        };
        assert_eq!(plan.as_deref(), Some("individual-goat"));
        assert!(windows[0].available);
        assert_eq!(windows[0].limit, Some(14.0));
        assert!(windows[1].available);
        // 月窗：剩余 69.04 + 本期已用 0.946 = 总额 69.99（两位舍入），已用 0.95（≈1.36%）。
        assert!(windows[2].available);
        assert_eq!(windows[2].remaining_percent, Some(98.64));
        assert_eq!(windows[2].limit, Some(69.99));
        assert_eq!(windows[2].used, Some(0.95));
        assert_eq!(windows[2].unit.as_deref(), Some("USD"));
    }

    #[test]
    fn command_code_monthly_unavailable_without_summary() {
        // summary 拉取失败（total_cost=None）时月窗不可用，5h/周照常展示。
        let body = r#"{
          "credits": { "monthlyCredits": 69.04 },
          "windowLimits": {
            "fiveHour": { "used": 0.96, "cap": 14, "resetAt": 1787817057032 },
            "weekly":   { "used": 0.96, "cap": 35, "resetAt": 1788403857032 }
          }
        }"#;
        let FetchOutput::Quota { windows, .. } =
            parse_command_code_credits(body, None, None).unwrap()
        else {
            panic!("expected quota")
        };
        assert!(windows[0].available);
        assert!(windows[1].available);
        assert!(!windows[2].available);
    }
}

//! 阿里云 Coding Plan / Token Plan 个人版（CookieCloud 提供控制台登录态）。
//!
//! 两者都是控制台内部 RPC（`{console}/data/api.json`），鉴权三件套：
//! - Cookie（CookieCloud 同步，域名见 extra.domain，如 .aliyun.com / .alibabacloud.com）
//! - `x-xsrf-token` 头：取自 `login_aliyunid_csrf` cookie
//! - `sec_token` query 参数：从控制台首页 HTML 的 `window.ALIYUN_CONSOLE_CONFIG.SEC_TOKEN`
//!   提取（请求必须带 `Sec-Fetch-Mode: navigate` 等导航头服务端才渲染该配置）
//!
//! 注意：以上均为逆向接口，形态可能漂移；解析一律防御式，失败给出明确错误。

use serde_json::Value;

use super::{Credentials, num, snippet};
use crate::usage::cookiecloud::{self, Cookie};
use crate::usage::error::UsageError;
use crate::usage::http::{UsageHttp, ensure_not_auth_error};
use crate::usage::types::{FetchOutput, QuotaWindow, WindowKind, empty_windows, set_window, ts_ms};

/// 各区域形态的控制台/API 宿主与商品码。
struct AlibabaSite {
    /// 控制台宿主（取 HTML 中的 sec_token + Coding Plan RPC）。
    console: &'static str,
    /// Token Plan RPC 宿主（与控制台不同）。
    token_api: &'static str,
    /// Token Plan RPC 的 action 名。
    token_action: &'static str,
    region: &'static str,
    coding_commodity: &'static str,
    /// cornerstoneParam.consoleSite；国际站是历史拼写 MODELSTUDIO_ALBABACLOUD（少个 A），必须照抄。
    console_site: &'static str,
}

const CHINA: AlibabaSite = AlibabaSite {
    console: "https://bailian.console.aliyun.com",
    token_api: "https://bailian-cs.console.aliyun.com",
    token_action: "BroadScopeAspnGateway",
    region: "cn-beijing",
    coding_commodity: "sfm_codingplan_public_cn",
    console_site: "CN",
};

const INTL: AlibabaSite = AlibabaSite {
    console: "https://modelstudio.console.alibabacloud.com",
    token_api: "https://bailian-singapore-cs.alibabacloud.com",
    token_action: "IntlBroadScopeAspnGateway",
    region: "ap-southeast-1",
    coding_commodity: "sfm_codingplan_public_intl",
    console_site: "MODELSTUDIO_ALBABACLOUD",
};

/// 拉取控制台登录态三件套。
async fn console_auth(
    http: &UsageHttp,
    creds: &Credentials<'_>,
    site: &AlibabaSite,
) -> Result<(String, String, String), UsageError> {
    let (cfg, domain) = creds.cookiecloud()?;
    let cookies = cookiecloud::fetch_cookies(http, &cfg, &domain).await?;
    let cookie = cookiecloud::cookie_header(&cookies);

    // 导航头：服务端只在"浏览器导航"请求里渲染 ALIYUN_CONSOLE_CONFIG。
    let nav_headers = [
        ("Cookie", cookie.clone()),
        ("Accept", "text/html,application/xhtml+xml".to_string()),
        ("Sec-Fetch-Mode", "navigate".to_string()),
        ("Sec-Fetch-Dest", "document".to_string()),
        ("Sec-Fetch-Site", "none".to_string()),
        ("Upgrade-Insecure-Requests", "1".to_string()),
    ];
    let html_reply = http
        .get(&format!("{}/", site.console), &nav_headers)
        .await?;
    if html_reply.status == 401
        || html_reply.status == 403
        || (300..400).contains(&html_reply.status)
    {
        return Err(UsageError::Auth);
    }
    let sec_token = extract_sec_token(&html_reply.body).ok_or_else(|| {
        UsageError::Parse("无法从控制台页面提取 sec_token（登录态可能已失效）".to_string())
    })?;

    let xsrf = xsrf_token(&cookies).ok_or_else(|| {
        UsageError::MissingCredential(
            "login_aliyunid_csrf cookie（请确认控制台已登录并同步）".to_string(),
        )
    })?;
    Ok((cookie, xsrf, sec_token))
}

/// 从控制台 HTML 中提取 `SEC_TOKEN: "..."`（兼容带引号 key 的 JSON 写法）。
fn extract_sec_token(html: &str) -> Option<String> {
    let idx = html.find("SEC_TOKEN")?;
    let after = &html[idx + "SEC_TOKEN".len()..];
    let colon = after.find(':')?;
    let after = &after[colon + 1..];
    let start = after.find(['"', '\''])?;
    let quote = after.as_bytes()[start] as char;
    let rest = &after[start + 1..];
    let end = rest.find(quote)?;
    let token = rest[..end].trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

/// `login_aliyunid_csrf` cookie → x-xsrf-token（值可能 percent 编码）。
fn xsrf_token(cookies: &[Cookie]) -> Option<String> {
    cookiecloud::find_cookie(cookies, "login_aliyunid_csrf").map(percent_decode)
}

fn percent_decode(s: &str) -> String {
    if !s.contains('%') {
        return s.to_string();
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// form 值编码（`application/x-www-form-urlencoded`；qianwenai fetcher 共用）。
pub(super) fn form_encode_value(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b'*') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

// ── Coding Plan ─────────────────────────────────────────────
// POST {console}/data/api.json?action=zeldaEasy...queryCodingPlanInstanceInfoV2...
// quota 三窗口：per5Hour* / perWeek* / perBillMonth*（毫秒重置时间戳）。

pub async fn fetch_alibaba_coding(
    http: &UsageHttp,
    creds: &Credentials<'_>,
    intl: bool,
) -> Result<FetchOutput, UsageError> {
    let site = if intl { &INTL } else { &CHINA };
    let (cookie, xsrf, sec_token) = console_auth(http, creds, site).await?;

    // sec_token 走 percent-encode（与 Token Plan 的 form 路径同规则）：token 可能
    // 含 + / = 等保留字符，裸拼 query 会被曲解（+ → 空格、= 截断参数）。
    let url = format!(
        "{}/data/api.json?action=zeldaEasy.broadscope-bailian.codingPlan.queryCodingPlanInstanceInfoV2&product=broadscope-bailian&api=queryCodingPlanInstanceInfoV2&currentRegionId={}&sec_token={}",
        site.console,
        site.region,
        form_encode_value(&sec_token)
    );
    let body = format!(
        r#"{{"queryCodingPlanInstanceInfoRequest": {{"commodityCode": "{}"}}}}"#,
        site.coding_commodity
    );
    let reply = http
        .post_json(&url, &[("Cookie", cookie), ("x-xsrf-token", xsrf)], &body)
        .await?;
    ensure_not_auth_error(&reply)?;
    if reply.status != 200 {
        return Err(UsageError::Upstream(reply.status, snippet(&reply.body)));
    }
    parse_alibaba_coding(&reply.body)
}

fn parse_alibaba_coding(body: &str) -> Result<FetchOutput, UsageError> {
    let v: Value = serde_json::from_str(body)
        .map_err(|e| UsageError::Parse(format!("响应不是合法 JSON：{e}")))?;
    if v.get("status_code").and_then(Value::as_i64).unwrap_or(-1) != 0 {
        return Err(UsageError::Upstream(200, snippet(body)));
    }
    let data = v
        .get("data")
        .ok_or_else(|| UsageError::Parse("缺少 data 字段".to_string()))?;

    // quota 可能在 data 顶层，也可能嵌在某个有效实例里（多实例账号）。
    let mut plan = None;
    let quota = if let Some(q) = data.get("codingPlanQuotaInfo") {
        q
    } else {
        data.get("codingPlanInstanceInfos")
            .and_then(Value::as_array)
            .and_then(|instances| {
                instances.iter().find(|inst| {
                    matches!(
                        inst.get("status").and_then(Value::as_str),
                        Some("VALID") | Some("ACTIVE")
                    )
                })
            })
            .and_then(|inst| {
                plan = inst
                    .get("planName")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                inst.get("codingPlanQuotaInfo")
            })
            .ok_or_else(|| UsageError::Parse("data 中没有 quota 信息".to_string()))?
    };

    let mut windows = empty_windows();
    for (prefix, kind) in [
        ("per5Hour", WindowKind::FiveHour),
        ("perWeek", WindowKind::Weekly),
        ("perBillMonth", WindowKind::Monthly),
    ] {
        let used = quota.get(format!("{prefix}UsedQuota")).and_then(num);
        let total = quota.get(format!("{prefix}TotalQuota")).and_then(num);
        if let (Some(used), Some(total)) = (used, total) {
            let resets_at = quota
                .get(format!("{prefix}QuotaNextRefreshTime"))
                .and_then(Value::as_i64)
                .and_then(ts_ms);
            set_window(
                &mut windows,
                QuotaWindow::from_used_limit(kind, used, total, resets_at, None),
            );
        }
    }
    if windows.iter().all(|w| !w.available) {
        return Err(UsageError::Parse("quota 中没有窗口数据".to_string()));
    }
    Ok(FetchOutput::Quota { plan, windows })
}

// ── Token Plan 个人版 ───────────────────────────────────────
// POST {token_api}/data/api.json?action={token_action}&product=sfm_bailian&api=zeldaHttp...
// form body：params=<json>&region=...&sec_token=...
// 响应：OneConsole 信封，载荷在 data.DataV2.data.data，data 可能是 JSON 字符串需二次解析。
// 解析与千问品牌（qianwenai）共用 `parse_token_plan_usage`。

pub async fn fetch_alibaba_token(
    http: &UsageHttp,
    creds: &Credentials<'_>,
    intl: bool,
) -> Result<FetchOutput, UsageError> {
    let site = if intl { &INTL } else { &CHINA };
    let (cookie, xsrf, sec_token) = console_auth(http, creds, site).await?;

    let api = "zeldaHttp.apikeyMgr./tokenplan/personal/api/v2/usage";
    let params = format!(
        r#"{{"Api":"{api}","V":"1.0","Data":{{"cornerstoneParam":{{"consoleSite":"{}"}}}}}}"#,
        site.console_site
    );
    let form = format!(
        "params={}&region={}&sec_token={}",
        form_encode_value(&params),
        site.region,
        form_encode_value(&sec_token)
    );
    let url = format!(
        "{}/data/api.json?action={}&product=sfm_bailian&api={}",
        site.token_api, site.token_action, api
    );
    let reply = http
        .post_form(&url, &[("Cookie", cookie), ("x-xsrf-token", xsrf)], &form)
        .await?;
    ensure_not_auth_error(&reply)?;
    if reply.status != 200 {
        return Err(UsageError::Upstream(reply.status, snippet(&reply.body)));
    }
    parse_token_plan_usage(&reply.body)
}

/// Token Plan 个人版用量解析（CN/INTL 与千问品牌共用）。
///
/// 信封：`{ code, successResponse, data: { success, errorCode, errorMsg,
/// DataV2: { data: { data: {…} } } } }`，载荷位于 `data.DataV2.data.data`
/// （外层 `data` 兼容字符串二次解析）。
/// 窗口字段 `per5Hour*` / `per1Week*` / `per1Month*` 按上游实际下发防御性填充
/// （控制台会轮换窗口组合，当前常见为仅月窗口）；`*Percentage` 是 0–1 的**已用**比例。
pub(super) fn parse_token_plan_usage(body: &str) -> Result<FetchOutput, UsageError> {
    // 登录失效是 HTTP 200 + 体内标记（非传输状态码），先于成功判定识别。
    let lower = body.to_ascii_lowercase();
    if [
        "bailiangateway.login.notlogined",
        "consoleneedlogin",
        "no_login",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
    {
        return Err(UsageError::Auth);
    }

    let v: Value = serde_json::from_str(body)
        .map_err(|e| UsageError::Parse(format!("响应不是合法 JSON：{e}")))?;
    let ok = v.get("successResponse").and_then(Value::as_bool) == Some(true)
        || v.get("code").and_then(Value::as_str) == Some("200");
    if !ok {
        return Err(UsageError::Upstream(200, snippet(body)));
    }
    // data 可能是 JSON 字符串，需二次解析。
    let data = match v.get("data") {
        Some(Value::String(s)) => serde_json::from_str::<Value>(s)
            .map_err(|e| UsageError::Parse(format!("内层 data 不是合法 JSON：{e}")))?,
        Some(other) => other.clone(),
        None => return Err(UsageError::Parse("缺少 data 字段".to_string())),
    };
    // 外层 200 成功信封里内嵌 data 帧可能 success:false 带真错误。
    if data.get("success").and_then(Value::as_bool) == Some(false) {
        let msg = data
            .get("errorMsg")
            .or_else(|| data.get("errorCode"))
            .and_then(Value::as_str)
            .unwrap_or("未知错误");
        return Err(UsageError::Upstream(200, msg.to_string()));
    }
    let mut windows = empty_windows();
    if let Some(payload) = data.pointer("/DataV2/data/data") {
        for (prefix, kind) in [
            ("per5Hour", WindowKind::FiveHour),
            ("per1Week", WindowKind::Weekly),
            ("per1Month", WindowKind::Monthly),
        ] {
            let Some(raw) = payload.get(format!("{prefix}Percentage")).and_then(num) else {
                continue;
            };
            // 上游下发 0–1 已用比例；>1 视为已是百分比（防单位漂移），越界由构造器收敛。
            let used_percent = if raw > 1.0 { raw } else { raw * 100.0 };
            let resets_at = payload
                .get(format!("{prefix}ResetTime"))
                .and_then(Value::as_i64)
                .and_then(ts_ms);
            set_window(
                &mut windows,
                QuotaWindow::from_used_percent(kind, used_percent, resets_at),
            );
        }
    }
    if windows.iter().all(|w| !w.available) {
        // 该网关偶发返回 200 但无窗口字段（瞬时空包）。
        return Err(UsageError::Upstream(
            200,
            "上游返回了空用量数据（瞬时空包，请稍后重试）".to_string(),
        ));
    }
    Ok(FetchOutput::Quota {
        plan: None,
        windows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_sec_token_variants() {
        assert_eq!(
            extract_sec_token(r#"window.ALIYUN_CONSOLE_CONFIG = {"SEC_TOKEN":"abc123def"}"#),
            Some("abc123def".to_string())
        );
        assert_eq!(
            extract_sec_token("SEC_TOKEN: 'xyz789',"),
            Some("xyz789".to_string())
        );
        assert_eq!(extract_sec_token("no token here"), None);
        assert_eq!(extract_sec_token(r#"SEC_TOKEN: """#), None);
    }

    #[test]
    fn percent_decode_only_when_needed() {
        assert_eq!(percent_decode("abc"), "abc");
        assert_eq!(percent_decode("a%3Db%25"), "a=b%");
    }

    #[test]
    fn coding_top_level_quota() {
        let body = r#"{
          "status_code": 0,
          "data": {
            "codingPlanQuotaInfo": {
              "per5HourUsedQuota": 52, "per5HourTotalQuota": 1000, "per5HourQuotaNextRefreshTime": 1700000300000,
              "perWeekUsedQuota": 800, "perWeekTotalQuota": 5000, "perWeekQuotaNextRefreshTime": 1700100000000,
              "perBillMonthUsedQuota": 1200, "perBillMonthTotalQuota": 20000
            }
          }
        }"#;
        let FetchOutput::Quota { windows, .. } = parse_alibaba_coding(body).unwrap() else {
            panic!("expected quota")
        };
        assert!(windows.iter().all(|w| w.available));
        assert_eq!(windows[0].used_percent, Some(5.2));
        assert!(windows[0].resets_at.is_some());
        assert_eq!(windows[2].used_percent, Some(6.0));
    }

    #[test]
    fn coding_quota_nested_in_valid_instance() {
        let body = r#"{
          "status_code": 0,
          "data": {
            "codingPlanInstanceInfos": [
              { "planName": "Old Plan", "status": "EXPIRED", "codingPlanQuotaInfo": { "per5HourUsedQuota": 1, "per5HourTotalQuota": 10 } },
              { "planName": "Alibaba Coding Plan Pro", "status": "VALID",
                "codingPlanQuotaInfo": { "perWeekUsedQuota": 800, "perWeekTotalQuota": 5000 } }
            ]
          }
        }"#;
        let FetchOutput::Quota { plan, windows } = parse_alibaba_coding(body).unwrap() else {
            panic!("expected quota")
        };
        assert_eq!(plan.as_deref(), Some("Alibaba Coding Plan Pro"));
        assert!(!windows[0].available);
        assert!(windows[1].available);
        assert_eq!(windows[1].used_percent, Some(16.0));
    }

    /// 生产实测响应原样（2026-10-07，千问品牌）：仅月窗口，0–1 已用比例。
    const TOKEN_LIVE_MONTHLY: &str = r#"{
      "code": "200",
      "data": {
        "DataV2": {
          "ret": ["SUCCESS::接口调用成功"],
          "data": {
            "msg": "Success.", "code": "SUCCESS", "success": true,
            "data": {
              "per1MonthPercentage": 0.00946406956521739,
              "per1MonthResetTime": 1794067200000
            },
            "requestId": "6fcc2c27-69d1-98b2-ba43-1463651405a1"
          }
        },
        "success": true, "httpStatus": 200, "errorCode": "", "errorMsg": "",
        "api": "zeldaHttp.apikeyMgr./tokenplan/personal/api/v2/usage"
      },
      "httpStatusCode": "200",
      "requestId": "6fcc2c27-69d1-48b2-ba43-1463651405a1",
      "successResponse": true
    }"#;

    #[test]
    fn token_monthly_window_from_live_capture() {
        let FetchOutput::Quota { windows, .. } =
            parse_token_plan_usage(TOKEN_LIVE_MONTHLY).unwrap()
        else {
            panic!("expected quota")
        };
        assert!(!windows[0].available, "5 小时窗口未下发");
        assert!(!windows[1].available, "周窗口未下发");
        let month = &windows[2];
        assert!(month.available);
        assert_eq!(month.window, WindowKind::Monthly);
        let used = month.used_percent.unwrap();
        // 构造器按 2 位小数收敛（0.9464…% → 0.95%），与展示层口径一致。
        assert_eq!(used, 0.95);
        assert!((month.remaining_percent.unwrap() - 99.05).abs() < 1e-9);
        assert_eq!(month.resets_at, ts_ms(1794067200000));
    }

    #[test]
    fn token_five_hour_and_week_windows() {
        let body = r#"{ "code": "200", "successResponse": true,
          "data": { "success": true, "DataV2": { "data": { "success": true, "data": {
            "per5HourPercentage": 0.5, "per5HourResetTime": 1700000300000,
            "per1WeekPercentage": 0.25, "per1WeekResetTime": 1700100000000 } } } } }"#;
        let FetchOutput::Quota { windows, .. } = parse_token_plan_usage(body).unwrap() else {
            panic!("expected quota")
        };
        assert_eq!(windows[0].used_percent, Some(50.0));
        assert_eq!(windows[0].resets_at, ts_ms(1700000300000));
        assert_eq!(windows[1].used_percent, Some(25.0));
        assert!(!windows[2].available);
    }

    #[test]
    fn token_mixed_window_sets() {
        // 控制台轮换窗口组合期间可能同时下发 5 小时与月窗口。
        let body = r#"{ "code": "200", "successResponse": true,
          "data": { "success": true, "DataV2": { "data": { "success": true, "data": {
            "per5HourPercentage": 1.0,
            "per1MonthPercentage": 0.25, "per1MonthResetTime": 1700179200000 } } } } }"#;
        let FetchOutput::Quota { windows, .. } = parse_token_plan_usage(body).unwrap() else {
            panic!("expected quota")
        };
        assert_eq!(windows[0].used_percent, Some(100.0));
        assert!(!windows[1].available);
        assert_eq!(windows[2].used_percent, Some(25.0));
        assert!(windows[2].resets_at.is_some());
    }

    #[test]
    fn token_percentage_units_are_used_ratios() {
        // 1.0 = 100% 已用（耗尽），不是 1%。
        let exhausted = r#"{ "code": "200", "successResponse": true,
          "data": { "success": true, "DataV2": { "data": { "success": true,
          "data": { "per5HourPercentage": 1.0 } } } } }"#;
        let FetchOutput::Quota { windows, .. } = parse_token_plan_usage(exhausted).unwrap() else {
            panic!("expected quota")
        };
        assert_eq!(windows[0].used_percent, Some(100.0));
        assert_eq!(windows[0].remaining_percent, Some(0.0));

        // >1 视为已是百分比（防上游单位漂移）；越界由构造器收敛到 100。
        let percent_scaled = r#"{ "code": "200", "successResponse": true,
          "data": { "success": true, "DataV2": { "data": { "success": true,
          "data": { "per1WeekPercentage": 42.5 } } } } }"#;
        let FetchOutput::Quota { windows, .. } = parse_token_plan_usage(percent_scaled).unwrap()
        else {
            panic!("expected quota")
        };
        assert_eq!(windows[1].used_percent, Some(42.5));

        let over = r#"{ "code": "200", "successResponse": true,
          "data": { "success": true, "DataV2": { "data": { "success": true,
          "data": { "per1WeekPercentage": 250.0 } } } } }"#;
        let FetchOutput::Quota { windows, .. } = parse_token_plan_usage(over).unwrap() else {
            panic!("expected quota")
        };
        assert_eq!(windows[1].used_percent, Some(100.0));
    }

    #[test]
    fn token_data_string_double_parse() {
        let inner = r#"{"success": true, "DataV2": {"data": {"data": {"per5HourPercentage": 0.5, "per5HourResetTime": 1700000300000, "per1WeekPercentage": 0.25}}}}"#;
        let body = format!(
            r#"{{ "code": "200", "successResponse": true, "data": {} }}"#,
            serde_json::to_string(inner).unwrap()
        );
        let FetchOutput::Quota { windows, .. } = parse_token_plan_usage(&body).unwrap() else {
            panic!("expected quota")
        };
        assert_eq!(windows[0].used_percent, Some(50.0));
        assert_eq!(windows[1].used_percent, Some(25.0));
        assert!(!windows[2].available);
    }

    #[test]
    fn token_login_expired_markers_map_to_auth() {
        for marker in [
            "BailianGateway.Login.NotLogined",
            "ConsoleNeedLogin",
            "NO_LOGIN",
        ] {
            let body = format!(
                r#"{{ "code": "200", "successResponse": true,
                  "data": {{ "success": false, "errorCode": "{marker}", "errorMsg": "{marker}" }} }}"#
            );
            assert!(
                matches!(parse_token_plan_usage(&body), Err(UsageError::Auth)),
                "marker={marker}"
            );
        }
    }

    #[test]
    fn token_inner_error_frame() {
        let body = r#"{ "code": "200", "successResponse": true,
          "data": { "success": false, "errorCode": "BailianGateway.Workspace.NotAuthorised" } }"#;
        assert!(matches!(
            parse_token_plan_usage(body),
            Err(UsageError::Upstream(_, _))
        ));
    }

    #[test]
    fn token_transient_empty_frame() {
        let body = r#"{ "code": "200", "successResponse": true, "data": {} }"#;
        assert!(matches!(
            parse_token_plan_usage(body),
            Err(UsageError::Upstream(_, _))
        ));
    }
}

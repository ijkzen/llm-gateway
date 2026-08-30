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
use crate::usage::http::UsageHttp;
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

/// form 值编码（`application/x-www-form-urlencoded`）。
fn form_encode_value(s: &str) -> String {
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

    let url = format!(
        "{}/data/api.json?action=zeldaEasy.broadscope-bailian.codingPlan.queryCodingPlanInstanceInfoV2&product=broadscope-bailian&api=queryCodingPlanInstanceInfoV2&currentRegionId={}&sec_token={}",
        site.console, site.region, sec_token
    );
    let body = format!(
        r#"{{"queryCodingPlanInstanceInfoRequest": {{"commodityCode": "{}"}}}}"#,
        site.coding_commodity
    );
    let reply = http
        .post_json(&url, &[("Cookie", cookie), ("x-xsrf-token", xsrf)], &body)
        .await?;
    if reply.status == 401 || reply.status == 403 {
        return Err(UsageError::Auth);
    }
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
// 响应：OneConsole 信封，data 可能是 JSON 字符串需二次解析。

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
    if reply.status == 401 || reply.status == 403 {
        return Err(UsageError::Auth);
    }
    if reply.status != 200 {
        return Err(UsageError::Upstream(reply.status, snippet(&reply.body)));
    }
    parse_alibaba_token(&reply.body)
}

fn parse_alibaba_token(body: &str) -> Result<FetchOutput, UsageError> {
    let v: Value = serde_json::from_str(body)
        .map_err(|e| UsageError::Parse(format!("响应不是合法 JSON：{e}")))?;
    let code = v.get("code").and_then(Value::as_str).unwrap_or_default();
    if code != "200" {
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
    if let Some(percent) = data.get("per5HourPercentage").and_then(num) {
        let resets_at = data
            .get("per5HourResetTime")
            .and_then(Value::as_i64)
            .and_then(ts_ms);
        set_window(
            &mut windows,
            QuotaWindow::from_used_percent(WindowKind::FiveHour, percent, resets_at),
        );
    }
    if let Some(percent) = data.get("per1WeekPercentage").and_then(num) {
        let resets_at = data
            .get("per1WeekResetTime")
            .and_then(Value::as_i64)
            .and_then(ts_ms);
        set_window(
            &mut windows,
            QuotaWindow::from_used_percent(WindowKind::Weekly, percent, resets_at),
        );
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

    #[test]
    fn token_data_string_double_parse() {
        let inner = r#"{"per5HourPercentage": 42.5, "per5HourResetTime": 1700000300000, "per1WeekPercentage": 10.0}"#;
        let body = format!(
            r#"{{ "code": "200", "successResponse": true, "data": {} }}"#,
            serde_json::to_string(inner).unwrap()
        );
        let FetchOutput::Quota { windows, .. } = parse_alibaba_token(&body).unwrap() else {
            panic!("expected quota")
        };
        assert!(windows[0].available);
        assert_eq!(windows[0].used_percent, Some(42.5));
        assert!(windows[1].available);
        assert!(!windows[2].available);
    }

    #[test]
    fn token_inner_error_frame() {
        let body = r#"{ "code": "200", "successResponse": true,
          "data": { "success": false, "errorCode": "BailianGateway.Workspace.NotAuthorised" } }"#;
        assert!(matches!(
            parse_alibaba_token(body),
            Err(UsageError::Upstream(_, _))
        ));
    }

    #[test]
    fn token_transient_empty_frame() {
        let body = r#"{ "code": "200", "successResponse": true, "data": {} }"#;
        assert!(matches!(
            parse_alibaba_token(body),
            Err(UsageError::Upstream(_, _))
        ));
    }
}

//! 千问 Token Plan（千问AI平台品牌，host `token-plan.maas.qianwenaiapi.com`）用量查询。
//!
//! 用量不在推理接口里（`sk-sp-` Key 查不到，控制台网关回 `BailianGateway.Login.NotLogined`），
//! 走控制台 RPC，凭据 = CookieCloud 同步的 `platform.qianwenai.com` 登录态 Cookie：
//! 1. `GET {home}/tool/user/info.json` → `data.secToken`；
//! 2. `POST {cs}/data/api.json`（query/form 见下）→ OneConsole 信封，载荷 `data.DataV2.data.data`。
//!
//! 解析与 CN/INTL 品牌共用 `alibaba::parse_token_plan_usage`（同一业务 API、同一信封）。
//! 逆向接口，形态可能漂移；解析一律防御式，失败给出明确错误。

use serde_json::Value;

use super::alibaba::{form_encode_value, parse_token_plan_usage};
use super::{Credentials, snippet};
use crate::usage::cookiecloud;
use crate::usage::error::UsageError;
use crate::usage::http::{UsageHttp, ensure_not_auth_error};
use crate::usage::types::FetchOutput;

/// 登录态用户信息接口（取 secToken）。
const SEC_TOKEN_URL: &str = "https://platform-home.qianwenai.com/tool/user/info.json";
/// 用量 RPC 网关（与控制台宿主不同）。
const USAGE_API_URL: &str = "https://cs-data.qianwenai.com/data/api.json";
/// 控制台页面地址（Origin/Referer 头用）。
const CONSOLE_ORIGIN: &str = "https://platform.qianwenai.com";
const CONSOLE_REFERER: &str =
    "https://platform.qianwenai.com/home/billing/subscription/token-plan-individual";
/// 被网关转发的业务 API 名。
const USAGE_API: &str = "zeldaHttp.apikeyMgr./tokenplan/personal/api/v2/usage";

pub async fn fetch_qianwenai(
    http: &UsageHttp,
    creds: &Credentials<'_>,
) -> Result<FetchOutput, UsageError> {
    let (cfg, domain) = creds.cookiecloud()?;
    let cookies = cookiecloud::fetch_cookies(http, &cfg, &domain).await?;
    let cookie = cookiecloud::cookie_header(&cookies);

    let sec_token = fetch_sec_token(http, &cookie).await?;

    // cornerstoneParam 六字段为网关校验所需（实测契约）。
    let params = format!(
        r#"{{"Api":"{USAGE_API}","Data":{{"cornerstoneParam":{{"domain":"platform.qianwenai.com","consoleSite":"QIANWENAI","console":"ONE_CONSOLE","xsp_lang":"zh-CN","protocol":"V2","productCode":"p_efm"}}}},"V":"1.0"}}"#
    );
    let form = format!(
        "product=sfm_bailian&action=BroadScopeAspnGateway&sec_token={}&region=cn-beijing&params={}&api={}",
        form_encode_value(&sec_token),
        form_encode_value(&params),
        form_encode_value(USAGE_API)
    );
    let url = format!(
        "{USAGE_API_URL}?product=sfm_bailian&action=BroadScopeAspnGateway&api={}&_t={}",
        form_encode_value(USAGE_API),
        now_ms()
    );
    let reply = http
        .post_form(
            &url,
            &[
                ("Cookie", cookie),
                ("Accept", "application/json, text/plain, */*".to_string()),
                ("Cache-Control", "no-cache, no-store, max-age=0".to_string()),
                ("Pragma", "no-cache".to_string()),
                ("Origin", CONSOLE_ORIGIN.to_string()),
                ("Referer", CONSOLE_REFERER.to_string()),
            ],
            &form,
        )
        .await?;
    ensure_not_auth_error(&reply)?;
    if reply.status != 200 {
        return Err(UsageError::Upstream(reply.status, snippet(&reply.body)));
    }
    parse_token_plan_usage(&reply.body)
}

/// 取 secToken；缺失/为空视为登录态失效（HTTP 200 + 体内标记，非传输状态码）。
async fn fetch_sec_token(http: &UsageHttp, cookie: &str) -> Result<String, UsageError> {
    let url = format!("{SEC_TOKEN_URL}?_t={}", now_ms());
    let reply = http
        .get(
            &url,
            &[
                ("Cookie", cookie.to_string()),
                ("Accept", "application/json, text/plain, */*".to_string()),
                ("Cache-Control", "no-cache, no-store, max-age=0".to_string()),
                ("Pragma", "no-cache".to_string()),
                ("Referer", CONSOLE_REFERER.to_string()),
            ],
        )
        .await?;
    ensure_not_auth_error(&reply)?;
    if reply.status != 200 {
        return Err(UsageError::Upstream(reply.status, snippet(&reply.body)));
    }
    let v: Value = serde_json::from_str(&reply.body)
        .map_err(|e| UsageError::Parse(format!("secToken 响应不是合法 JSON：{e}")))?;
    v.pointer("/data/secToken")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or(UsageError::Auth)
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

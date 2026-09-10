//! AgentRouter（agentrouter.org，New-API 公益站）用量查询。
//!
//! 站点面板接口（实测 2026-09-07）：
//! `GET https://agentrouter.org/api/user/self` 返回账户 `quota`（剩余配额）。
//! 鉴权走登录态两件套：`Cookie`（CookieCloud 同步的 agentrouter.org session 与
//! acw_tc cookie）+ `New-Api-User`（用户在 extra 手填的账户号）。
//!
//! 该接口在阿里云 WAF 之后，脚本 UA 短时多请求会触发 JS 人机挑战
//! （200 + HTML），因此出站固定用浏览器 UA（与真实浏览器登录态同源）。
//! quota 按站点 QuotaPerUnit（50 万 = $1）换算为美元余额，单条 primary 输出，
//! 供按量门控/LB 取剩余额度。

use serde_json::Value;

use super::{Credentials, num, snippet};
use crate::usage::cookiecloud;
use crate::usage::error::UsageError;
use crate::usage::http::{UsageHttp, is_session_invalid_status};
use crate::usage::types::{BalanceItem, FetchOutput};

const SELF_URL: &str = "https://agentrouter.org/api/user/self";

/// 站点 QuotaPerUnit：50 万 quota = $1（2026-09-05 双向对账定论）。
const QUOTA_PER_USD: f64 = 500000.0;

/// 浏览器 UA：规避该域 /api/* 背后阿里云 WAF 对脚本 UA 的 JS 人机挑战。
const BROWSER_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
    AppleWebKit/537.36 (KHTML, like Gecko) Chrome/152.0.0.0 Safari/537.36";

pub async fn fetch_agentrouter(
    http: &UsageHttp,
    creds: &Credentials<'_>,
) -> Result<FetchOutput, UsageError> {
    let (cfg, domain) = creds.cookiecloud()?;
    let cookies = cookiecloud::fetch_cookies(http, &cfg, &domain).await?;
    let cookie = cookiecloud::cookie_header(&cookies);
    let user = creds.require("new_api_user")?;

    let reply = http
        .get(
            SELF_URL,
            &[
                ("Cookie", cookie),
                ("New-Api-User", user.to_string()),
                ("User-Agent", BROWSER_USER_AGENT.to_string()),
            ],
        )
        .await?;
    // 登录态失效 / 被重定向到登录页（401/403/3xx）→ Auth，提示重新同步 CookieCloud。
    if is_session_invalid_status(reply.status) {
        return Err(UsageError::Auth);
    }
    if reply.status != 200 {
        return Err(UsageError::Upstream(reply.status, snippet(&reply.body)));
    }
    parse_agentrouter_self(&reply.body)
}

fn parse_agentrouter_self(body: &str) -> Result<FetchOutput, UsageError> {
    let v: Value = serde_json::from_str(body)
        .map_err(|e| UsageError::Parse(format!("响应不是合法 JSON：{e}")))?;
    if v.get("success").and_then(Value::as_bool) != Some(true) {
        // 业务包络失效特征：success=false 且 message 指明登录态失效（归位一）。
        let message = v.get("message").and_then(Value::as_str).unwrap_or("");
        if message.contains("登录") && (message.contains("失效") || message.contains("过期"))
        {
            return Err(UsageError::Auth);
        }
        return Err(UsageError::Upstream(200, snippet(body)));
    }
    let quota = v
        .get("data")
        .and_then(|d| d.get("quota"))
        .and_then(num)
        .ok_or_else(|| UsageError::Parse("响应缺少 data.quota 字段".to_string()))?;
    Ok(FetchOutput::Balance {
        items: vec![BalanceItem {
            label: "剩余额度".to_string(),
            amount: round2(quota / QUOTA_PER_USD),
            currency: Some("USD".to_string()),
            primary: true,
        }],
    })
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_real_response_2026_09_07() {
        // 用户真实账号实测响应（2026-09-07）。quota=99998626 → /500000 = 199.997252 → 200.0。
        let body = r#"{
          "data": {
            "id": 591449, "username": "github_591449", "password": "", "original_password": "",
            "display_name": "IJKZEN", "role": 1, "status": 1, "email": "ijkzen@outlook.com",
            "github_id": "ijkzen", "github_user_id": "31531836", "access_token": "x",
            "quota": 99998626, "used_quota": 1374, "request_count": 7, "group": "default",
            "last_login_time": 1788757629, "created_at": 1788411954
          },
          "message": "", "success": true
        }"#;
        let FetchOutput::Balance { items } = parse_agentrouter_self(body).unwrap() else {
            panic!("expected balance")
        };
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "剩余额度");
        assert!(items[0].primary);
        assert_eq!(items[0].currency.as_deref(), Some("USD"));
        assert_eq!(items[0].amount, 200.0);
    }

    #[test]
    fn parse_quota_zero_is_balance_zero() {
        // quota=0（额度耗尽）仍产出单条余额 0，交由门控判定停用，而非报错。
        let body = r#"{"data":{"id":1,"quota":0,"used_quota":0},"success":true}"#;
        let FetchOutput::Balance { items } = parse_agentrouter_self(body).unwrap() else {
            panic!("expected balance")
        };
        assert_eq!(items[0].amount, 0.0);
        assert!(items[0].primary);
    }

    #[test]
    fn parse_missing_quota_is_parse_error() {
        // 响应没有 data.quota（如字段被站点调整）→ 解析错误。
        let body = r#"{"data":{"id":1},"success":true}"#;
        assert!(matches!(
            parse_agentrouter_self(body),
            Err(UsageError::Parse(_))
        ));
    }

    /// 归位一治理：success=false 且文案为登录态失效 → Auth（原判 Upstream，随批改）。
    #[test]
    fn parse_login_expired_message_is_auth_error() {
        let body = r#"{"data":null,"message":"登录状态已失效","success":false}"#;
        assert!(matches!(
            parse_agentrouter_self(body),
            Err(UsageError::Auth)
        ));
    }

    /// 其余业务失败（文本不含登录失效特征）仍是 Upstream。
    #[test]
    fn parse_success_false_other_message_is_upstream_error() {
        let body = r#"{"data":null,"message":"请求过于频繁","success":false}"#;
        assert!(matches!(
            parse_agentrouter_self(body),
            Err(UsageError::Upstream(_, _))
        ));
    }

    #[test]
    fn parse_waf_challenge_html_is_parse_error() {
        // WAF 挑战返回 200 + HTML → 不是合法 JSON → Parse。
        let body = "<html><body>js challenge</body></html>";
        assert!(matches!(
            parse_agentrouter_self(body),
            Err(UsageError::Parse(_))
        ));
    }
}

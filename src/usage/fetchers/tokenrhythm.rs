//! TokenRhythm（tokenrhythm.studio，「基元律动」自研路由站）钱包余额。
//!
//! 凭据：provider extra 的 `account`（账号，可为手机号）+ `password`。
//! 登录 `POST /api/auth/login` 换取会话 Cookie `tr_session`（HttpOnly），
//! 调用方拿到后回写 extra 复用；只有会话失效（HTTP 401/403/3xx 或业务
//! code=UNAUTHORIZED）才重新登录，避免频繁登录。
//!
//! `GET /api/wallet/summary` 返回钱包各分项余额（字符串、单位元），鉴权走
//! 会话 Cookie + 浏览器 UA（实测 2026-09-11：无 Cookie → HTTP 401
//! `{"code":"UNAUTHORIZED","message":"未认证或登录已过期"}`）。
//!
//! 归一化：只取 `data.availableBalanceCny`（当前可立即花的钱）作单条 primary
//! 余额；赠送锁定/待激活（giftLocked、giftStatus=pending_activation）、充值、
//! 欠费、冻结等分项不计入（门控/LB 只看可用余额）。

use serde_json::Value;

use super::{Credentials, num, snippet};
use crate::usage::error::UsageError;
use crate::usage::http::{UsageHttp, is_session_invalid_status};
use crate::usage::types::{BalanceItem, FetchOutput};

const LOGIN_URL: &str = "https://tokenrhythm.studio/api/auth/login";
const WALLET_URL: &str = "https://tokenrhythm.studio/api/wallet/summary";

/// 登录下发、后续请求复用的会话 Cookie 名。
pub const SESSION_COOKIE: &str = "tr_session";

/// 浏览器 UA：与真实浏览器登录态同源，规避站点反爬/人机校验。
const BROWSER_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
    AppleWebKit/537.36 (KHTML, like Gecko) Chrome/152.0.0.0 Safari/537.36";

/// 用会话 Cookie 拉钱包余额。会话失效（401/403/3xx、code=UNAUTHORIZED）→ Auth。
pub async fn fetch_wallet(http: &UsageHttp, session: &str) -> Result<FetchOutput, UsageError> {
    let reply = http
        .get(
            WALLET_URL,
            &[
                ("Cookie", format!("{SESSION_COOKIE}={session}")),
                ("User-Agent", BROWSER_USER_AGENT.to_string()),
            ],
        )
        .await?;
    if is_session_invalid_status(reply.status) {
        return Err(UsageError::Auth);
    }
    if reply.status != 200 {
        return Err(UsageError::Upstream(reply.status, snippet(&reply.body)));
    }
    parse_wallet(&reply.body)
}

/// 账号密码登录，返回新的会话 Cookie 值。调用方负责先持久化，再重试钱包查询。
///
/// 凭据错误与账号不存在上游返回同一个 HTTP 401，统一映射为 Auth。
pub async fn login(http: &UsageHttp, creds: &Credentials<'_>) -> Result<String, UsageError> {
    let account = creds.require("account")?;
    let password = creds.require("password")?;
    let body = serde_json::json!({ "account": account, "password": password }).to_string();
    let (reply, set_cookie) = http
        .post_json_capturing_cookies(
            LOGIN_URL,
            &[("User-Agent", BROWSER_USER_AGENT.to_string())],
            &body,
        )
        .await?;
    if is_session_invalid_status(reply.status) {
        return Err(UsageError::Auth);
    }
    if reply.status != 200 {
        return Err(UsageError::Upstream(reply.status, snippet(&reply.body)));
    }
    let value: Value = serde_json::from_str(&reply.body)
        .map_err(|e| UsageError::Parse(format!("登录响应不是合法 JSON：{e}")))?;
    if value.get("code").and_then(num) != Some(0.0) {
        return Err(UsageError::Upstream(200, snippet(&reply.body)));
    }
    extract_cookie(&set_cookie, SESSION_COOKIE)
        .ok_or_else(|| UsageError::Parse(format!("登录响应未下发 {SESSION_COOKIE} Cookie")))
}

/// 从 Set-Cookie 值列表取指定 Cookie 的值（`name=value` 首段，忽略属性）。
fn extract_cookie(set_cookie: &[String], name: &str) -> Option<String> {
    set_cookie.iter().find_map(|raw| {
        let (key, value) = raw.split(';').next()?.trim().split_once('=')?;
        (key.trim() == name).then(|| value.trim().to_string())
    })
}

fn parse_wallet(body: &str) -> Result<FetchOutput, UsageError> {
    let v: Value = serde_json::from_str(body)
        .map_err(|e| UsageError::Parse(format!("响应不是合法 JSON：{e}")))?;
    let code = v.get("code").and_then(num).unwrap_or(-1.0);
    // 业务包络失效码：字符串 "UNAUTHORIZED"（未认证/登录已过期）与数字 401
    // 都是登录态失效（归位一）。
    if v.get("code").and_then(Value::as_str) == Some("UNAUTHORIZED") || code == 401.0 {
        return Err(UsageError::Auth);
    }
    if code != 0.0 {
        return Err(UsageError::Upstream(200, snippet(body)));
    }
    let available = v
        .get("data")
        .and_then(|d| d.get("availableBalanceCny"))
        .and_then(num)
        .ok_or_else(|| UsageError::Parse("响应缺少 data.availableBalanceCny 字段".to_string()))?;
    Ok(FetchOutput::Balance {
        items: vec![BalanceItem {
            label: "可用余额".to_string(),
            amount: round2(available),
            currency: Some("CNY".to_string()),
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
    fn parse_real_response_2026_09_11() {
        // 用户真实账号实测响应（2026-09-11 登录后查询）。
        let body = r#"{
          "code": 0,
          "message": "ok",
          "data": {
            "currency": "CNY",
            "availableBalanceCny": "55.82539240",
            "giftAvailableCny": "55.82539240",
            "giftLockedCny": "0.00000000",
            "rechargeBalanceCny": "0.00000000",
            "debtBalanceCny": "0.00000000",
            "frozenBalanceCny": "0.00000000",
            "giftTotalCny": "55.82539240",
            "giftStatus": "active",
            "voidedGiftCny": "0.00000000",
            "asOf": "2026-09-11T07:33:10.838Z"
          },
          "traceId": "trace_eafd7bfd-1d1d-4657-855e-ff73c30895e3"
        }"#;
        let FetchOutput::Balance { items } = parse_wallet(body).unwrap() else {
            panic!("expected balance")
        };
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "可用余额");
        assert!(items[0].primary);
        assert_eq!(items[0].currency.as_deref(), Some("CNY"));
        assert!((items[0].amount - 55.83).abs() < 1e-9); // 55.82539240 → round2
    }

    #[test]
    fn parse_zero_balance_is_zero() {
        // 可用余额为 0（额度耗尽）仍产出单条余额 0，交由门控判定停用，而非报错。
        let body = r#"{"code":0,"message":"ok","data":{"currency":"CNY","availableBalanceCny":"0.00000000"}}"#;
        let FetchOutput::Balance { items } = parse_wallet(body).unwrap() else {
            panic!("expected balance")
        };
        assert_eq!(items[0].amount, 0.0);
        assert!(items[0].primary);
    }

    #[test]
    fn parse_missing_available_is_parse_error() {
        // 响应缺少 availableBalanceCny（字段被站点调整）→ 解析错误。
        let body = r#"{"code":0,"message":"ok","data":{"currency":"CNY"}}"#;
        assert!(matches!(parse_wallet(body), Err(UsageError::Parse(_))));
    }

    /// 归位一治理：code=401 为登录态失效 → Auth（原判 Upstream，随批改）。
    #[test]
    fn parse_401_code_is_auth_error() {
        let body = r#"{"code":401,"message":"登录状态已失效","data":null}"#;
        assert!(matches!(parse_wallet(body), Err(UsageError::Auth)));
    }

    /// 实测（2026-09-11）失效包络是字符串 "UNAUTHORIZED" → Auth。
    #[test]
    fn parse_unauthorized_string_code_is_auth_error() {
        let body = r#"{"code":"UNAUTHORIZED","message":"未认证或登录已过期","traceId":"trace_x"}"#;
        assert!(matches!(parse_wallet(body), Err(UsageError::Auth)));
    }

    /// 其余非零码仍是 Upstream。
    #[test]
    fn parse_other_nonzero_code_is_upstream_error() {
        let body = r#"{"code":500,"message":"服务器错误","data":null}"#;
        assert!(matches!(
            parse_wallet(body),
            Err(UsageError::Upstream(_, _))
        ));
    }

    #[test]
    fn parse_non_json_body_is_parse_error() {
        // 反爬/挑战返回 200 + HTML → 不是合法 JSON → Parse。
        let body = "<html><body>challenge</body></html>";
        assert!(matches!(parse_wallet(body), Err(UsageError::Parse(_))));
    }

    /// Set-Cookie 解析：取目标 Cookie 的 `name=value` 首段，忽略属性与其它 Cookie。
    #[test]
    fn extract_cookie_picks_target_value() {
        let set_cookie = vec![
            "tr_session=sess_abc123; Max-Age=2592000; HttpOnly; Path=/; SameSite=Lax; Secure"
                .to_string(),
            "tr_csrf=token.other; Max-Age=2592000; Path=/; SameSite=Lax; Secure".to_string(),
        ];
        assert_eq!(
            extract_cookie(&set_cookie, SESSION_COOKIE).as_deref(),
            Some("sess_abc123")
        );
        assert_eq!(
            extract_cookie(&set_cookie, "tr_csrf").as_deref(),
            Some("token.other")
        );
        assert!(extract_cookie(&set_cookie, "missing").is_none());
        assert!(extract_cookie(&[], SESSION_COOKIE).is_none());
    }
}

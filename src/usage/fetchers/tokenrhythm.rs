//! TokenRhythm（tokenrhythm.studio，「基元律动」自研路由站）钱包余额。
//!
//! 用户中心内部接口（实测 2026-09-07）：
//! `GET https://tokenrhythm.studio/api/wallet/summary` 返回钱包各分项余额
//! （字符串、单位元）。鉴权走登录态 Cookie（CookieCloud 同步的
//! tokenrhythm.studio tr_session/tr_csrf 等）+ 浏览器 UA。
//!
//! 归一化：只取 `data.availableBalanceCny`（当前可立即花的钱）作单条 primary
//! 余额；赠送锁定/待激活（giftLocked、giftStatus=pending_activation）、充值、
//! 欠费、冻结等分项不计入（门控/LB 只看可用余额）。

use serde_json::Value;

use super::{Credentials, num, snippet};
use crate::usage::cookiecloud;
use crate::usage::error::UsageError;
use crate::usage::http::{UsageHttp, is_session_invalid_status};
use crate::usage::types::{BalanceItem, FetchOutput};

const WALLET_URL: &str = "https://tokenrhythm.studio/api/wallet/summary";

/// 浏览器 UA：与真实浏览器登录态同源，规避站点反爬/人机校验。
const BROWSER_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
    AppleWebKit/537.36 (KHTML, like Gecko) Chrome/152.0.0.0 Safari/537.36";

pub async fn fetch_tokenrhythm_wallet(
    http: &UsageHttp,
    creds: &Credentials<'_>,
) -> Result<FetchOutput, UsageError> {
    let (cfg, domain) = creds.cookiecloud()?;
    let cookies = cookiecloud::fetch_cookies(http, &cfg, &domain).await?;
    let cookie = cookiecloud::cookie_header(&cookies);

    let reply = http
        .get(
            WALLET_URL,
            &[
                ("Cookie", cookie),
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
    parse_tokenrhythm_wallet(&reply.body)
}

fn parse_tokenrhythm_wallet(body: &str) -> Result<FetchOutput, UsageError> {
    let v: Value = serde_json::from_str(body)
        .map_err(|e| UsageError::Parse(format!("响应不是合法 JSON：{e}")))?;
    let code = v.get("code").and_then(num).unwrap_or(-1.0);
    // 业务包络失效码：401 为登录态失效（归位一）。
    if code == 401.0 {
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
    fn parse_real_response_2026_09_07() {
        // 用户真实账号实测响应（2026-09-07）。可用余额 9.74899120 元（赠送券），
        // 锁定 58 元待激活、充值 0 → 只出单条可用余额。
        let body = r#"{
          "code": 0,
          "message": "ok",
          "data": {
            "currency": "CNY",
            "availableBalanceCny": "9.74899120",
            "giftAvailableCny": "9.74899120",
            "giftLockedCny": "58.00000000",
            "rechargeBalanceCny": "0.00000000",
            "debtBalanceCny": "0.00000000",
            "frozenBalanceCny": "0.00000000",
            "giftTotalCny": "67.74899120",
            "giftStatus": "pending_activation",
            "voidedGiftCny": "0.00000000",
            "asOf": "2026-09-07T06:00:25.806Z"
          },
          "traceId": "trace_fc2e5a73-207f-46bf-9cb1-f00a85bf5be3"
        }"#;
        let FetchOutput::Balance { items } = parse_tokenrhythm_wallet(body).unwrap() else {
            panic!("expected balance")
        };
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "可用余额");
        assert!(items[0].primary);
        assert_eq!(items[0].currency.as_deref(), Some("CNY"));
        assert!((items[0].amount - 9.75).abs() < 1e-9); // 9.74899120 → round2 = 9.75
    }

    #[test]
    fn parse_zero_balance_is_zero() {
        // 可用余额为 0（额度耗尽）仍产出单条余额 0，交由门控判定停用，而非报错。
        let body = r#"{"code":0,"message":"ok","data":{"currency":"CNY","availableBalanceCny":"0.00000000"}}"#;
        let FetchOutput::Balance { items } = parse_tokenrhythm_wallet(body).unwrap() else {
            panic!("expected balance")
        };
        assert_eq!(items[0].amount, 0.0);
        assert!(items[0].primary);
    }

    #[test]
    fn parse_missing_available_is_parse_error() {
        // 响应缺少 availableBalanceCny（字段被站点调整）→ 解析错误。
        let body = r#"{"code":0,"message":"ok","data":{"currency":"CNY"}}"#;
        assert!(matches!(
            parse_tokenrhythm_wallet(body),
            Err(UsageError::Parse(_))
        ));
    }

    /// 归位一治理：code=401 为登录态失效 → Auth（原判 Upstream，随批改）。
    #[test]
    fn parse_401_code_is_auth_error() {
        let body = r#"{"code":401,"message":"登录状态已失效","data":null}"#;
        assert!(matches!(
            parse_tokenrhythm_wallet(body),
            Err(UsageError::Auth)
        ));
    }

    /// 其余非零码仍是 Upstream。
    #[test]
    fn parse_other_nonzero_code_is_upstream_error() {
        let body = r#"{"code":500,"message":"服务器错误","data":null}"#;
        assert!(matches!(
            parse_tokenrhythm_wallet(body),
            Err(UsageError::Upstream(_, _))
        ));
    }

    #[test]
    fn parse_non_json_body_is_parse_error() {
        // 反爬/挑战返回 200 + HTML → 不是合法 JSON → Parse。
        let body = "<html><body>challenge</body></html>";
        assert!(matches!(
            parse_tokenrhythm_wallet(body),
            Err(UsageError::Parse(_))
        ));
    }
}

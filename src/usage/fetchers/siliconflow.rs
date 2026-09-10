//! 硅基流动 SiliconFlow（中国站）钱包余额（CookieCloud 登录态 + X-Subject-Id）。
//!
//! 控制台内部钱包接口（实测 2026-09-07）：
//! `GET https://cloud.siliconflow.cn/walletd-server/api/v1/subject/wallets`
//! 鉴权两件套：`Cookie`（CookieCloud 同步的 .siliconflow.cn 登录态）+ `X-Subject-Id`
//! （账号主体 ID，用户手动填在 extra.x_subject_id）。
//!
//! 金额字段为字符串，单位 1 元 = 10^12（cap=16000000000000 即 16 元）。余额型钱包
//! （有 cap 上限）逐张产出明细；授信账户等无上限额度（cap=-1、无 balance）跳过。
//! 归一化为一条 primary「账户余额」合计 + 每张券明细，供按量门控/LB 取合计。

use serde_json::Value;

use super::{Credentials, num, snippet};
use crate::usage::cookiecloud;
use crate::usage::error::UsageError;
use crate::usage::http::{UsageHttp, is_session_invalid_status};
use crate::usage::types::{BalanceItem, FetchOutput};
const WALLETS_URL: &str =
    "https://cloud.siliconflow.cn/walletd-server/api/v1/subject/wallets?pageSize=50&visible=1";

/// 金额单位：1 元 = 10^12 微单位。
const UNIT: f64 = 1e12;

pub async fn fetch_siliconflow_wallets(
    http: &UsageHttp,
    creds: &Credentials<'_>,
) -> Result<FetchOutput, UsageError> {
    let (cfg, domain) = creds.cookiecloud()?;
    let cookies = cookiecloud::fetch_cookies(http, &cfg, &domain).await?;
    let cookie = cookiecloud::cookie_header(&cookies);
    let subject = creds.require("x_subject_id")?;

    let reply = http
        .get(
            WALLETS_URL,
            &[("Cookie", cookie), ("X-Subject-Id", subject.to_string())],
        )
        .await?;
    // 登录态失效/被重定向到登录页（401/403/3xx）→ Auth，提示重新同步 CookieCloud。
    if is_session_invalid_status(reply.status) {
        return Err(UsageError::Auth);
    }
    if reply.status != 200 {
        return Err(UsageError::Upstream(reply.status, snippet(&reply.body)));
    }
    parse_siliconflow_wallets(&reply.body)
}

fn parse_siliconflow_wallets(body: &str) -> Result<FetchOutput, UsageError> {
    let v: Value = serde_json::from_str(body)
        .map_err(|e| UsageError::Parse(format!("响应不是合法 JSON：{e}")))?;
    let code = v.get("code").and_then(num).unwrap_or(-1.0);
    // 业务包络失效码：40100 为登录态失效（归位一）。
    if code == 40100.0 {
        return Err(UsageError::Auth);
    }
    if code != 20000.0 {
        return Err(UsageError::Upstream(200, snippet(body)));
    }
    let wallets = v
        .get("data")
        .and_then(|d| d.get("wallets"))
        .and_then(Value::as_array)
        .ok_or_else(|| UsageError::Parse("响应缺少 data.wallets 字段".to_string()))?;

    // 余额型钱包（cap != -1 且 balance 可解析）逐张明细；cap=-1（无上限授信）跳过。
    // 仅计入 CNY（currency=99）钱包：跨币种不能直接相加合计，其它币种跳过展示。
    let mut detail_total = 0.0;
    let mut items: Vec<BalanceItem> = Vec::new();
    for wallet in wallets {
        let cap = wallet.get("cap").and_then(num).unwrap_or(-1.0);
        let Some(balance) = wallet.get("balance").and_then(num) else {
            continue;
        };
        if cap < 0.0 || wallet.get("currency").and_then(num).unwrap_or(-1.0) != 99.0 {
            continue; // 授信账户等无上限额度，或非 CNY 币种：不展示、不合计。
        }
        items.push(BalanceItem {
            label: wallet_label(wallet),
            amount: round2(balance / UNIT),
            currency: Some("CNY".to_string()),
            primary: false,
        });
        detail_total += balance / UNIT;
    }
    if items.is_empty() {
        return Err(UsageError::Parse("响应中没有余额型钱包".to_string()));
    }
    // 合计行置顶作为 primary（按量门控/LB 取 primary 即总余额）。
    items.insert(
        0,
        BalanceItem {
            label: "账户余额".to_string(),
            amount: round2(detail_total),
            currency: Some("CNY".to_string()),
            primary: true,
        },
    );
    Ok(FetchOutput::Balance { items })
}

/// 取钱包显示名：`name` 为多语言 JSON 字符串，取 zh-cn；解析失败回退 benefitId/walletId。
fn wallet_label(wallet: &Value) -> String {
    if let Some(name) = wallet.get("name").and_then(Value::as_str)
        && let Ok(map) = serde_json::from_str::<Value>(name)
        && let Some(zh) = map.get("zh-cn").and_then(Value::as_str)
    {
        return zh.to_string();
    }
    wallet
        .get("benefitId")
        .or_else(|| wallet.get("walletId"))
        .and_then(Value::as_str)
        .unwrap_or("钱包")
        .to_string()
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_normalizes_wallets_to_balance() {
        // 金额字符串，1 元 = 10^12；name 为多语言 JSON 取 zh-cn。
        let body = r#"{
          "code": 20000,
          "data": { "wallets": [
            {"walletId":"W1","name":"{\"zh-cn\":\"认证奖励券\",\"en-us\":\"Verification Gift Coupon\"}","cap":"16000000000000","used":"7181540480000","balance":"8818459520000","currency":99,"stage":3},
            {"walletId":"W2","name":"{\"zh-cn\":\"充值余额\"}","cap":"5000000000000","balance":"2500000000000","currency":99,"stage":3}
          ], "pagination": {"total": 2} }
        }"#;
        let FetchOutput::Balance { items } = parse_siliconflow_wallets(body).unwrap() else {
            panic!("expected balance")
        };
        assert_eq!(items.len(), 3);
        // 合计行置顶 + primary。
        assert_eq!(items[0].label, "账户余额");
        assert!(items[0].primary);
        assert!((items[0].amount - 11.32).abs() < 1e-9); // (8.81845952 + 2.5) → round2 = 11.32
        // 明细行。
        assert_eq!(items[1].label, "认证奖励券");
        assert!(!items[1].primary);
        assert!((items[1].amount - 8.82).abs() < 0.01);
        assert_eq!(items[2].label, "充值余额");
        assert_eq!(items[2].currency.as_deref(), Some("CNY"));
    }

    #[test]
    fn parse_skips_unlimited_credit_wallet() {
        // 授信账户：cap=-1（无上限）、无 balance → 跳过；仍产出 primary 合计。
        let body = r#"{
          "code": 20000,
          "data": { "wallets": [
            {"walletId":"W-credit","name":"{\"zh-cn\":\"授信账户\"}","cap":"-1","used":"0","factor":1,"stage":4,"currency":99},
            {"walletId":"W-gift","name":"{\"zh-cn\":\"认证奖励券\"}","cap":"16000000000000","balance":"8818459520000","stage":3,"currency":99},
            {"walletId":"W-usd","name":"{\"zh-cn\":\"美元券\"}","cap":"1000000000000","balance":"500000000000","stage":3,"currency":840}
          ] }
        }"#;
        let FetchOutput::Balance { items } = parse_siliconflow_wallets(body).unwrap() else {
            panic!("expected balance")
        };
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].label, "账户余额");
        assert_eq!(items[1].label, "认证奖励券");
        assert!((items[1].amount - 8.82).abs() < 0.01);
    }

    #[test]
    fn parse_no_balance_wallets_is_error() {
        let body = r#"{"code":20000,"data":{"wallets":[{"walletId":"W-credit","name":"{}","cap":"-1","factor":1,"stage":4}]}}"#;
        assert!(matches!(
            parse_siliconflow_wallets(body),
            Err(UsageError::Parse(_))
        ));
    }

    /// 归位一治理：40100 是登录态失效的业务码，判 Auth（400 提示重新同步）而非 Upstream。
    #[test]
    fn parse_40100_code_is_auth_error() {
        let body = r#"{"code":40100,"msg":"unauthorized","data":{}}"#;
        assert!(matches!(
            parse_siliconflow_wallets(body),
            Err(UsageError::Auth)
        ));
    }

    /// 其余非 20000 业务码仍是 Upstream（200 包络上游错误）。
    #[test]
    fn parse_other_non_20000_code_is_upstream_error() {
        let body = r#"{"code":50001,"msg":"internal","data":{}}"#;
        assert!(matches!(
            parse_siliconflow_wallets(body),
            Err(UsageError::Upstream(_, _))
        ));
    }

    #[test]
    fn parse_real_response_2026_09_07() {
        // 用户真实账号实测响应（2026-09-07）：单张认证奖励券，授信账户被跳过。
        // cap=16 元，used=7.18 元，balance=8.81845952 元；到期 1803398400000。
        let body = r#"{
          "code": 20000,
          "data": {"wallets": [
            {"walletId":"W202608270715090735000002578014",
             "name":"{\"en-us\":\"Verification Gift Coupon\",\"zh-cn\":\"认证奖励券\"}",
             "cap":"16000000000000","used":"7181540480000","balance":8818459520000,
             "currency":99,"stage":3,"benefitId":"bf-507f1f77bcf86cd799439018",
             "availableUses":-1,"visible":true,"packageId":5,
             "notBefore":1787760000000,"expiresAt":1803398400000,
             "createdAt":1787786109578,"status":0},
            {"walletId":"W202608270714020735000001240927",
             "name":"{\"zh-cn\":\"授信账户\",\"en-us\":\"Credit Account\"}",
             "cap":"-1","used":"0","factor":1,"currency":99,"stage":4,
             "benefitId":"sys-credit-wallet","availableUses":-1,"visible":true,
             "notBefore":1764514800000,"expiresAt":9223372036855,
             "createdAt":946656000000,"status":0}
          ], "pagination": {"total": 2}}
        }"#;
        let FetchOutput::Balance { items } = parse_siliconflow_wallets(body).unwrap() else {
            panic!("expected balance")
        };
        // 仅余额型钱包（授信账户跳过）→ 账户余额合计 + 认证奖励券明细，金额一致。
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].label, "账户余额");
        assert!(items[0].primary);
        assert_eq!(items[0].currency.as_deref(), Some("CNY"));
        assert_eq!(items[0].amount, 8.82); // 合计行 round2 保留两位小数
        assert_eq!(items[1].label, "认证奖励券");
        assert!(!items[1].primary);
        assert_eq!(items[1].amount, 8.82); // 明细行同 round2
    }
}

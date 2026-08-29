//! 按量付费余额 fetcher：DeepSeek / Moonshot / OpenRouter。

use serde_json::Value;

use super::{Credentials, num, snippet};
use crate::usage::error::UsageError;
use crate::usage::http::{UsageHttp, ensure_not_auth_error};
use crate::usage::types::{BalanceItem, FetchOutput};

// ── DeepSeek ────────────────────────────────────────────────
// GET https://api.deepseek.com/user/balance（Bearer）
// balance_infos[] 多币种，金额字段是字符串。

pub async fn fetch_deepseek(
    http: &UsageHttp,
    creds: &Credentials<'_>,
) -> Result<FetchOutput, UsageError> {
    let reply = http
        .get(
            "https://api.deepseek.com/user/balance",
            &[("Authorization", format!("Bearer {}", creds.api_key_required()?))],
        )
        .await?;
    ensure_not_auth_error(&reply)?;
    if reply.status != 200 {
        return Err(UsageError::Upstream(reply.status, snippet(&reply.body)));
    }
    parse_deepseek(&reply.body)
}

fn parse_deepseek(body: &str) -> Result<FetchOutput, UsageError> {
    let v: Value = serde_json::from_str(body)
        .map_err(|e| UsageError::Parse(format!("响应不是合法 JSON：{e}")))?;
    let infos = v
        .get("balance_infos")
        .and_then(Value::as_array)
        .ok_or_else(|| UsageError::Parse("缺少 balance_infos 字段".to_string()))?;

    let mut items = Vec::new();
    for info in infos {
        let currency = info
            .get("currency")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        for (key, label) in [
            ("total_balance", "余额"),
            ("topped_up_balance", "充值余额"),
            ("granted_balance", "赠送余额"),
        ] {
            if let Some(amount) = info.get(key).and_then(num) {
                items.push(BalanceItem {
                    label: format!("{label}（{currency}）"),
                    amount,
                    currency: Some(currency.clone()),
                });
            }
        }
    }
    if items.is_empty() {
        return Err(UsageError::Parse("balance_infos 为空".to_string()));
    }
    Ok(FetchOutput::Balance { items })
}

// ── Moonshot 开放平台 ───────────────────────────────────────
// GET https://{host}/v1/users/me/balance（Bearer），code != 0 为业务错误。

pub async fn fetch_moonshot(
    http: &UsageHttp,
    creds: &Credentials<'_>,
    host: &str,
) -> Result<FetchOutput, UsageError> {
    let reply = http
        .get(
            &format!("https://{host}/v1/users/me/balance"),
            &[("Authorization", format!("Bearer {}", creds.api_key_required()?))],
        )
        .await?;
    ensure_not_auth_error(&reply)?;
    if reply.status != 200 {
        return Err(UsageError::Upstream(reply.status, snippet(&reply.body)));
    }
    parse_moonshot(&reply.body)
}

fn parse_moonshot(body: &str) -> Result<FetchOutput, UsageError> {
    let v: Value = serde_json::from_str(body)
        .map_err(|e| UsageError::Parse(format!("响应不是合法 JSON：{e}")))?;
    if v.get("code").and_then(Value::as_i64).unwrap_or(-1) != 0 {
        return Err(UsageError::Upstream(200, snippet(body)));
    }
    let data = v
        .get("data")
        .ok_or_else(|| UsageError::Parse("缺少 data 字段".to_string()))?;

    let mut items = Vec::new();
    for (key, label) in [
        ("available_balance", "可用余额"),
        ("cash_balance", "现金余额"),
        ("voucher_balance", "代金券余额"),
    ] {
        if let Some(amount) = data.get(key).and_then(num) {
            items.push(BalanceItem {
                label: label.to_string(),
                amount,
                currency: None,
            });
        }
    }
    if items.is_empty() {
        return Err(UsageError::Parse("data 中没有余额字段".to_string()));
    }
    Ok(FetchOutput::Balance { items })
}

// ── OpenRouter ──────────────────────────────────────────────
// GET https://openrouter.ai/api/v1/credits（Bearer）
// 余额 = total_credits − total_usage。

pub async fn fetch_openrouter(
    http: &UsageHttp,
    creds: &Credentials<'_>,
) -> Result<FetchOutput, UsageError> {
    let reply = http
        .get(
            "https://openrouter.ai/api/v1/credits",
            &[("Authorization", format!("Bearer {}", creds.api_key_required()?))],
        )
        .await?;
    ensure_not_auth_error(&reply)?;
    if reply.status != 200 {
        return Err(UsageError::Upstream(reply.status, snippet(&reply.body)));
    }
    parse_openrouter(&reply.body)
}

fn parse_openrouter(body: &str) -> Result<FetchOutput, UsageError> {
    let v: Value = serde_json::from_str(body)
        .map_err(|e| UsageError::Parse(format!("响应不是合法 JSON：{e}")))?;
    let data = v
        .get("data")
        .ok_or_else(|| UsageError::Parse("缺少 data 字段".to_string()))?;
    let (Some(total), Some(used)) = (
        data.get("total_credits").and_then(num),
        data.get("total_usage").and_then(num),
    ) else {
        return Err(UsageError::Parse("缺少 total_credits/total_usage 字段".to_string()));
    };
    let usd = || Some("USD".to_string());
    Ok(FetchOutput::Balance {
        items: vec![
            BalanceItem {
                label: "剩余额度".to_string(),
                amount: (total - used).max(0.0),
                currency: usd(),
            },
            BalanceItem {
                label: "已使用".to_string(),
                amount: used,
                currency: usd(),
            },
            BalanceItem {
                label: "总充值".to_string(),
                amount: total,
                currency: usd(),
            },
        ],
    })
}

// ── 阶跃星辰（按量付费账户）─────────────────────────────────
// GET https://{host}/v1/accounts（Bearer）
// 响应 { object: "account", type: prepaid/postpaid, balance,
//        total_cash_balance(累计充值), total_voucher_balance(赠送) }（数值为 float）。
// 注意与 Step Plan（订阅制，走 stepfun.rs 的 cookie 通道）是两套体系。

pub async fn fetch_stepfun_account(
    http: &UsageHttp,
    creds: &Credentials<'_>,
    host: &str,
) -> Result<FetchOutput, UsageError> {
    let reply = http
        .get(
            &format!("https://{host}/v1/accounts"),
            &[("Authorization", format!("Bearer {}", creds.api_key_required()?))],
        )
        .await?;
    ensure_not_auth_error(&reply)?;
    if reply.status != 200 {
        return Err(UsageError::Upstream(reply.status, snippet(&reply.body)));
    }
    parse_stepfun_account(&reply.body)
}

fn parse_stepfun_account(body: &str) -> Result<FetchOutput, UsageError> {
    let v: Value = serde_json::from_str(body)
        .map_err(|e| UsageError::Parse(format!("响应不是合法 JSON：{e}")))?;
    let cny = || Some("CNY".to_string());
    let mut items = Vec::new();
    for (key, label) in [
        ("balance", "可用余额"),
        ("total_cash_balance", "累计充值"),
        ("total_voucher_balance", "赠送余额"),
    ] {
        if let Some(amount) = v.get(key).and_then(num) {
            items.push(BalanceItem {
                label: label.to_string(),
                amount,
                currency: cny(),
            });
        }
    }
    if items.is_empty() {
        return Err(UsageError::Parse("响应中没有余额字段".to_string()));
    }
    Ok(FetchOutput::Balance { items })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stepfun_account_balances() {
        let body = r#"{ "object": "account", "type": "prepaid", "balance": 850.0, "total_cash_balance": 1500.0, "total_voucher_balance": 200.0 }"#;
        let FetchOutput::Balance { items } = parse_stepfun_account(body).unwrap() else {
            panic!("expected balance")
        };
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].label, "可用余额");
        assert_eq!(items[0].amount, 850.0);
        assert_eq!(items[0].currency.as_deref(), Some("CNY"));
    }

    #[test]
    fn deepseek_multi_currency_string_amounts() {
        let body = r#"{
          "is_available": true,
          "balance_infos": [
            { "currency": "CNY", "total_balance": "110.00", "granted_balance": "10.00", "topped_up_balance": "100.00" },
            { "currency": "USD", "total_balance": "5.00", "granted_balance": "0.00", "topped_up_balance": "5.00" }
          ]
        }"#;
        let FetchOutput::Balance { items } = parse_deepseek(body).unwrap() else {
            panic!("expected balance")
        };
        assert_eq!(items.len(), 6);
        assert_eq!(items[0].label, "余额（CNY）");
        assert_eq!(items[0].amount, 110.0);
        assert_eq!(items[3].label, "余额（USD）");
    }

    #[test]
    fn moonshot_balances() {
        let body = r#"{ "code": 0, "data": { "available_balance": 50.0, "cash_balance": 30.0, "voucher_balance": 20.0 } }"#;
        let FetchOutput::Balance { items } = parse_moonshot(body).unwrap() else {
            panic!("expected balance")
        };
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].label, "可用余额");
        assert_eq!(items[0].amount, 50.0);
    }

    #[test]
    fn moonshot_business_error() {
        let body = r#"{ "code": 401, "message": "invalid key" }"#;
        assert!(matches!(parse_moonshot(body), Err(UsageError::Upstream(_, _))));
    }

    #[test]
    fn openrouter_remaining_is_total_minus_usage() {
        let body = r#"{ "data": { "total_credits": 100.0, "total_usage": 42.5 } }"#;
        let FetchOutput::Balance { items } = parse_openrouter(body).unwrap() else {
            panic!("expected balance")
        };
        assert_eq!(items[0].label, "剩余额度");
        assert_eq!(items[0].amount, 57.5);
        assert_eq!(items[0].currency.as_deref(), Some("USD"));
    }
}

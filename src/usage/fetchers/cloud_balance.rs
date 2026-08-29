//! 云厂商账户余额（AK/SK 控制面 OpenAPI）：
//! - 阿里云百炼：BSS `QueryAccountBalance`（RPC V1 签名，HMAC-SHA1）
//! - 火山方舟：费用中心 `QueryBalanceAcct`（V4 签名，service=`billing`，GET）
//!
//! 注意：查到的是整个云账号余额，不是单一产品线的余额。

use std::collections::BTreeMap;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use chrono::Utc;
use hmac::{Hmac, Mac};
use serde_json::Value;
use sha1::Sha1;

use super::{Credentials, num, snippet};
use crate::usage::error::UsageError;
use crate::usage::http::{UsageHttp, parse_json};
use crate::usage::types::{BalanceItem, FetchOutput};
use crate::usage::volcengine_sign;

// ── 阿里云 BSS QueryAccountBalance ──────────────────────────
// GET https://business.aliyuncs.com/?Action=QueryAccountBalance&Version=2017-12-14&...
// RPC V1 签名：StringToSign = GET&%2F&<percent_encode(升序 query)>，
// Signature = base64(HMAC-SHA1(sk + "&", StringToSign))。
// 注意：服务域名是 business.aliyuncs.com（bssopenapi.aliyuncs.com 不存在，DNS 解析失败）。

const ALIYUN_BSS_HOST: &str = "business.aliyuncs.com";

pub async fn fetch_aliyun_bss(
    http: &UsageHttp,
    creds: &Credentials<'_>,
) -> Result<FetchOutput, UsageError> {
    let ak = creds.require("ak")?;
    let sk = creds.require("sk")?;

    let mut params = BTreeMap::from([
        ("Action".to_string(), "QueryAccountBalance".to_string()),
        ("Version".to_string(), "2017-12-14".to_string()),
        ("Format".to_string(), "JSON".to_string()),
        ("AccessKeyId".to_string(), ak.to_string()),
        ("SignatureMethod".to_string(), "HMAC-SHA1".to_string()),
        ("SignatureVersion".to_string(), "1.0".to_string()),
        ("SignatureNonce".to_string(), uuid::Uuid::new_v4().to_string()),
        (
            "Timestamp".to_string(),
            Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        ),
    ]);
    let signature = aliyun_v1_signature("GET", &params, sk);
    params.insert("Signature".to_string(), signature);

    let query = params
        .iter()
        .map(|(k, v)| format!("{}={}", aliyun_percent_encode(k), aliyun_percent_encode(v)))
        .collect::<Vec<_>>()
        .join("&");
    let url = format!("https://{ALIYUN_BSS_HOST}/?{query}");
    let reply = http.get(&url, &[("Accept", "application/json".to_string())]).await?;
    if reply.status == 401 || reply.status == 403 {
        return Err(UsageError::Auth);
    }
    if reply.status != 200 {
        return Err(UsageError::Upstream(reply.status, snippet(&reply.body)));
    }
    parse_aliyun_bss(&reply.body)
}

/// 阿里云 RPC V1 签名（纯函数，`now` 由调用方放进 Timestamp 参数）。
fn aliyun_v1_signature(method: &str, params: &BTreeMap<String, String>, sk: &str) -> String {
    let query = params
        .iter()
        .map(|(k, v)| format!("{}={}", aliyun_percent_encode(k), aliyun_percent_encode(v)))
        .collect::<Vec<_>>()
        .join("&");
    let string_to_sign = format!(
        "{}&%2F&{}",
        method.to_ascii_uppercase(),
        aliyun_percent_encode(&query)
    );
    let mut mac = <Hmac<Sha1> as Mac>::new_from_slice(format!("{sk}&").as_bytes())
        .expect("HMAC accepts any key length");
    mac.update(string_to_sign.as_bytes());
    BASE64.encode(mac.finalize().into_bytes())
}

/// 阿里云 percentEncode：A-Za-z0-9 与 `-_.~` 不编码，空格 `%20`（不是 `+`）。
fn aliyun_percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn parse_aliyun_bss(body: &str) -> Result<FetchOutput, UsageError> {
    let v: Value = serde_json::from_str(body)
        .map_err(|e| UsageError::Parse(format!("响应不是合法 JSON：{e}")))?;
    let data = match v.get("Data") {
        Some(d) => d,
        None => {
            // 错误形态：{Code, Message}；签名/凭据类错误单独归类。
            let code = v.get("Code").and_then(Value::as_str).unwrap_or_default();
            let msg = v.get("Message").and_then(Value::as_str).unwrap_or("未知错误");
            let text = code.to_ascii_lowercase();
            if text.contains("signature") || text.contains("accesskey") || text.contains("forbidden")
            {
                return Err(UsageError::Auth);
            }
            return Err(UsageError::Upstream(200, format!("{code} {msg}")));
        }
    };

    let currency = data
        .get("Currency")
        .and_then(Value::as_str)
        .map(str::to_string);
    let mut items = Vec::new();
    for (key, label) in [
        ("AvailableAmount", "可用余额"),
        ("CreditAmount", "信控额度"),
    ] {
        if let Some(amount) = data.get(key).and_then(num) {
            items.push(BalanceItem {
                label: label.to_string(),
                amount,
                currency: currency.clone(),
            });
        }
    }
    if items.is_empty() {
        return Err(UsageError::Parse("Data 中没有余额字段".to_string()));
    }
    Ok(FetchOutput::Balance { items })
}

// ── 火山费用中心 QueryBalanceAcct ───────────────────────────
// GET https://billing.volcengineapi.com/?Action=QueryBalanceAcct&Version=2022-01-01&Region=cn-beijing
// service=billing；响应 Result.AvailableBalance/CashBalance/…（字符串，CNY）。

const VOLC_BILLING_HOST: &str = "billing.volcengineapi.com";

pub async fn fetch_volcengine_billing(
    http: &UsageHttp,
    creds: &Credentials<'_>,
) -> Result<FetchOutput, UsageError> {
    let ak = creds.require("ak")?;
    let sk = creds.require("sk")?;

    let sig = volcengine_sign::sign(
        "GET",
        VOLC_BILLING_HOST,
        "billing",
        "QueryBalanceAcct",
        "2022-01-01",
        "cn-beijing",
        ak,
        sk,
        b"",
        Utc::now(),
    );
    let url = format!(
        "https://{VOLC_BILLING_HOST}/?Action=QueryBalanceAcct&Version=2022-01-01&Region=cn-beijing"
    );
    let reply = http
        .get(
            &url,
            &[
                ("Authorization", sig.authorization),
                ("X-Date", sig.x_date),
                ("X-Content-Sha256", sig.payload_hash),
            ],
        )
        .await?;
    if reply.status == 401 || reply.status == 403 {
        return Err(UsageError::Auth);
    }
    parse_volcengine_billing(reply.status, &reply.body)
}

fn parse_volcengine_billing(status: u16, body: &str) -> Result<FetchOutput, UsageError> {
    let v = parse_json(&crate::usage::http::HttpReply {
        status,
        body: body.to_string(),
    })?;
    if let Some(err) = v
        .get("ResponseMetadata")
        .and_then(|m| m.get("Error"))
    {
        let code = err.get("Code").and_then(Value::as_str).unwrap_or_default();
        let message = err.get("Message").and_then(Value::as_str).unwrap_or_default();
        let text = format!("{code} {message}").to_ascii_lowercase();
        if text.contains("auth") || text.contains("signature") || text.contains("denied") {
            return Err(UsageError::Auth);
        }
        return Err(UsageError::Upstream(status, format!("{code} {message}")));
    }
    if status != 200 {
        return Err(UsageError::Upstream(status, snippet(body)));
    }
    let result = v
        .get("Result")
        .ok_or_else(|| UsageError::Parse("缺少 Result 字段".to_string()))?;

    let cny = || Some("CNY".to_string());
    let mut items = Vec::new();
    for (key, label) in [
        ("AvailableBalance", "可用余额"),
        ("CashBalance", "现金余额"),
        ("FreezeAmount", "冻结金额"),
        ("ArrearsBalance", "欠费金额"),
    ] {
        if let Some(amount) = result.get(key).and_then(num) {
            items.push(BalanceItem {
                label: label.to_string(),
                amount,
                currency: cny(),
            });
        }
    }
    if items.is_empty() {
        return Err(UsageError::Parse("Result 中没有余额字段".to_string()));
    }
    Ok(FetchOutput::Balance { items })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// python3 对拍（hmac-sha1 + base64）：
    /// params = AccessKeyId=AKID_EXAMPLE Action=QueryAccountBalance Format=JSON
    ///          SignatureMethod=HMAC-SHA1 SignatureNonce=fixed-nonce SignatureVersion=1.0
    ///          Timestamp=2026-08-30T12:00:00Z Version=2017-12-14
    /// sk = "SK_EXAMPLE"
    ///
    /// ```python
    /// import hmac, hashlib, base64, urllib.parse
    /// params = {
    ///   "AccessKeyId": "AKID_EXAMPLE", "Action": "QueryAccountBalance", "Format": "JSON",
    ///   "SignatureMethod": "HMAC-SHA1", "SignatureNonce": "fixed-nonce",
    ///   "SignatureVersion": "1.0", "Timestamp": "2026-08-30T12:00:00Z",
    ///   "Version": "2017-12-14",
    /// }
    /// enc = lambda s: urllib.parse.quote(str(s), safe="~")
    /// qs = "&".join(f"{enc(k)}={enc(v)}" for k, v in sorted(params.items()))
    /// sts = "GET&%2F&" + enc(qs)
    /// print(base64.b64encode(hmac.new(b"SK_EXAMPLE&", sts.encode(), hashlib.sha1).digest()).decode())
    /// ```
    #[test]
    fn aliyun_v1_known_answer() {
        let params = BTreeMap::from([
            ("AccessKeyId".to_string(), "AKID_EXAMPLE".to_string()),
            ("Action".to_string(), "QueryAccountBalance".to_string()),
            ("Format".to_string(), "JSON".to_string()),
            ("SignatureMethod".to_string(), "HMAC-SHA1".to_string()),
            ("SignatureNonce".to_string(), "fixed-nonce".to_string()),
            ("SignatureVersion".to_string(), "1.0".to_string()),
            ("Timestamp".to_string(), "2026-08-30T12:00:00Z".to_string()),
            ("Version".to_string(), "2017-12-14".to_string()),
        ]);
        let sig = aliyun_v1_signature("GET", &params, "SK_EXAMPLE");
        assert_eq!(sig, "GqS+JShlXLtw9cldhsDw+C5XaG0=");
    }

    #[test]
    fn aliyun_bss_parse() {
        let body = r#"{
          "RequestId": "16176743-6DC7-4CB3-BB25-A13982D8DFAD",
          "Code": "200", "Message": "success", "Success": true,
          "Data": { "AvailableAmount": "10000.00", "CreditAmount": "0.00",
                    "MybankCreditAmount": "0.00", "Currency": "CNY" }
        }"#;
        let FetchOutput::Balance { items } = parse_aliyun_bss(body).unwrap() else {
            panic!("expected balance")
        };
        assert_eq!(items[0].label, "可用余额");
        assert_eq!(items[0].amount, 10000.0);
        assert_eq!(items[0].currency.as_deref(), Some("CNY"));
    }

    #[test]
    fn aliyun_bss_signature_error_is_auth() {
        let body = r#"{ "Code": "SignatureDoesNotMatch", "Message": "signature mismatch" }"#;
        assert!(matches!(parse_aliyun_bss(body), Err(UsageError::Auth)));
    }

    #[test]
    fn volcengine_billing_parse() {
        let body = r#"{
          "ResponseMetadata": { "RequestId": "x", "Action": "QueryBalanceAcct", "Version": "2022-01-01", "Service": "billing" },
          "Result": { "AccountID": 2101234567, "ArrearsBalance": "1.01", "AvailableBalance": "77.01",
                      "CashBalance": "83.01", "CreditLimit": "0.01", "FreezeAmount": "5.01" }
        }"#;
        let FetchOutput::Balance { items } = parse_volcengine_billing(200, body).unwrap() else {
            panic!("expected balance")
        };
        assert_eq!(items[0].label, "可用余额");
        assert_eq!(items[0].amount, 77.01);
        assert_eq!(items[0].currency.as_deref(), Some("CNY"));
        assert_eq!(items.len(), 4);
    }

    #[test]
    fn volcengine_billing_auth_error() {
        let body = r#"{ "ResponseMetadata": { "Error": { "Code": "SignatureDoesNotMatch", "Message": "denied" } } }"#;
        assert!(matches!(
            parse_volcengine_billing(200, body),
            Err(UsageError::Auth)
        ));
    }
}

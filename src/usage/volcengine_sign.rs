//! 火山引擎 OpenAPI 签名 V4（HMAC-SHA256，service=`ark`）。
//!
//! 与 AWS SigV4 的差异（实测/cc-switch 口径）：algorithm 串为 `HMAC-SHA256`（无
//! `AWS4` 前缀）、credential scope 以 `/request` 结尾、签名密钥第一轮直接用 SK
//! （不加前缀）。canonical query 与签名头按名称升序。

use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

const HOST: &str = "open.volcengineapi.com";
const SERVICE: &str = "ark";
const ALGORITHM: &str = "HMAC-SHA256";
const CONTENT_TYPE: &str = "application/json; charset=utf-8";
const SIGNED_HEADERS: &str = "content-type;host;x-content-sha256;x-date";

pub struct VolcSignature {
    pub authorization: String,
    pub x_date: String,
    pub payload_hash: String,
}

/// 对 `POST https://open.volcengineapi.com/?Action=...&Version=...&Region=...`
/// 生成签名头（Authorization / X-Date / X-Content-Sha256）。
/// `now` 作参数传入以便确定性单测。
pub fn sign(
    action: &str,
    version: &str,
    region: &str,
    ak: &str,
    sk: &str,
    body: &[u8],
    now: DateTime<Utc>,
) -> VolcSignature {
    let x_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let short_date = now.format("%Y%m%d").to_string();
    let payload_hash = hex::encode(Sha256::digest(body));

    // canonical query：按 key 升序 + RFC3986 编码。
    let mut pairs = [
        ("Action", action),
        ("Region", region),
        ("Version", version),
    ];
    pairs.sort_by(|a, b| a.0.cmp(b.0));
    let canonical_query = pairs
        .iter()
        .map(|(k, v)| format!("{}={}", uri_encode(k), uri_encode(v)))
        .collect::<Vec<_>>()
        .join("&");

    let canonical_headers = format!(
        "content-type:{CONTENT_TYPE}\nhost:{HOST}\nx-content-sha256:{payload_hash}\nx-date:{x_date}\n"
    );
    let canonical_request = format!(
        "POST\n/\n{canonical_query}\n{canonical_headers}\n{SIGNED_HEADERS}\n{payload_hash}"
    );

    let scope = format!("{short_date}/{region}/{SERVICE}/request");
    let string_to_sign = format!(
        "{ALGORITHM}\n{x_date}\n{scope}\n{}",
        hex::encode(Sha256::digest(canonical_request.as_bytes()))
    );

    let k_date = hmac(sk.as_bytes(), short_date.as_bytes());
    let k_region = hmac(&k_date, region.as_bytes());
    let k_service = hmac(&k_region, SERVICE.as_bytes());
    let k_signing = hmac(&k_service, b"request");
    let signature = hex::encode(hmac(&k_signing, string_to_sign.as_bytes()));

    VolcSignature {
        authorization: format!(
            "{ALGORITHM} Credential={ak}/{scope}, SignedHeaders={SIGNED_HEADERS}, Signature={signature}"
        ),
        x_date,
        payload_hash,
    }
}

fn hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

/// RFC3986：保留 A-Za-z0-9 与 `-_.~`，其余百分号编码。
fn uri_encode(s: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// 用 python3 独立实现同一算法生成的期望签名（见下注释复现脚本）。
    /// 输入：action=GetCodingPlanUsage version=2024-01-01 region=cn-beijing
    ///       ak=AKID_EXAMPLE sk=SK_EXAMPLE body="" now=2026-08-30T12:00:00Z
    ///
    /// ```python
    /// import hmac, hashlib
    /// def H(k, d): return hmac.new(k, d.encode(), hashlib.sha256).digest()
    /// date, x_date = "20260830", "20260830T120000Z"
    /// ph = hashlib.sha256(b"").hexdigest()
    /// cq = "Action=GetCodingPlanUsage&Region=cn-beijing&Version=2024-01-01"
    /// ch = ("content-type:application/json; charset=utf-8\nhost:open.volcengineapi.com\n"
    ///       f"x-content-sha256:{ph}\nx-date:{x_date}\n")
    /// cr = f"POST\n/\n{cq}\n{ch}\ncontent-type;host;x-content-sha256;x-date\n{ph}"
    /// scope = f"{date}/cn-beijing/ark/request"
    /// sts = f"HMAC-SHA256\n{x_date}\n{scope}\n{hashlib.sha256(cr.encode()).hexdigest()}"
    /// k = H(H(H(H(b"SK_EXAMPLE", date), "cn-beijing"), "ark"), "request")
    /// print(hmac.new(k, sts.encode(), hashlib.sha256).hexdigest())
    /// ```
    const EXPECTED_SIGNATURE: &str = "1384532448cea98834a6b176d8795a4fb85b49672d12303fc5fe86a5fb49f024";

    #[test]
    fn known_answer_signature() {
        let now = Utc.with_ymd_and_hms(2026, 8, 30, 12, 0, 0).unwrap();
        let sig = sign(
            "GetCodingPlanUsage",
            "2024-01-01",
            "cn-beijing",
            "AKID_EXAMPLE",
            "SK_EXAMPLE",
            b"",
            now,
        );
        assert_eq!(sig.x_date, "20260830T120000Z");
        assert!(
            sig.authorization
                .starts_with("HMAC-SHA256 Credential=AKID_EXAMPLE/20260830/cn-beijing/ark/request, SignedHeaders=content-type;host;x-content-sha256;x-date, Signature=")
        );
        assert!(
            sig.authorization
                .ends_with(&format!("Signature={EXPECTED_SIGNATURE}"))
        );
    }

    #[test]
    fn uri_encode_rfc3986() {
        assert_eq!(uri_encode("GetAFPUsage"), "GetAFPUsage");
        assert_eq!(uri_encode("a b/c"), "a%20b%2Fc");
        assert_eq!(uri_encode("~ok-._"), "~ok-._");
    }
}

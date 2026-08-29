//! CookieCloud 集成：从用户自架的 CookieCloud 服务器拉取加密 cookie 并解密，
//! 供 cookie 类供应商（阿里/StepFun/小米）的用量查询接口使用。
//!
//! 协议（与 easychen/CookieCloud 及 CryptoJS 默认行为兼容）：
//! - `GET {server}/get/{uuid}` 返回 `{"encrypted": "<base64>"}`；
//! - 密钥材料 = `md5("{uuid}-{password}")` hex 的前 16 字符（作为 passphrase）；
//! - 密文为 OpenSSL `Salted__` 信封：base64("Salted__" + 8B salt + ciphertext)；
//! - key/iv 由 EVP_BytesToKey(MD5, 1 轮) 从 passphrase+salt 派生 48 字节
//!   （32B key + 16B iv），AES-256-CBC + PKCS7 解密。

use aes::Aes256;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use cbc::Decryptor;
use cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};
use md5::{Digest, Md5};

use super::error::UsageError;
use super::http::UsageHttp;

type Aes256CbcDec = Decryptor<Aes256>;

#[derive(Debug, Clone)]
pub struct CookieCloudConfig {
    pub server: String,
    pub uuid: String,
    pub password: String,
}

#[derive(Debug, Clone)]
pub struct Cookie {
    pub name: String,
    pub value: String,
}

/// 拉取并解密 CookieCloud 数据，返回目标域名可见的 cookie 列表。
///
/// 域名匹配规则：cookie 的 domain 去掉前导 `.` 后，与目标 host 相等
/// 或是其后缀（如 `.aliyun.com` 的 cookie 对 `bailian.console.aliyun.com` 可见）。
pub async fn fetch_cookies(
    http: &UsageHttp,
    cfg: &CookieCloudConfig,
    domain: &str,
) -> Result<Vec<Cookie>, UsageError> {
    let url = format!(
        "{}/get/{}",
        cfg.server.trim_end_matches('/'),
        cfg.uuid.trim()
    );
    let reply = http.get(&url, &[]).await?;
    if reply.status != 200 {
        return Err(UsageError::Upstream(
            reply.status,
            "CookieCloud 服务器返回异常".to_string(),
        ));
    }
    let body: serde_json::Value = super::http::parse_json(&reply)?;
    let encrypted = body
        .get("encrypted")
        .and_then(|v| v.as_str())
        .ok_or_else(|| UsageError::Parse("CookieCloud 响应缺少 encrypted 字段".to_string()))?;

    let plain = decrypt_payload(&cfg.uuid, &cfg.password, encrypted)?;
    let parsed: serde_json::Value = serde_json::from_str(&plain)
        .map_err(|e| UsageError::Parse(format!("CookieCloud 解密结果不是合法 JSON：{e}")))?;

    let target = domain.trim().trim_start_matches('.').to_ascii_lowercase();
    let mut cookies = Vec::new();
    if let Some(cookie_data) = parsed.get("cookie_data").and_then(|v| v.as_object()) {
        for (cookie_domain, list) in cookie_data {
            let cd = cookie_domain.trim_start_matches('.').to_ascii_lowercase();
            let visible = target == cd || target.ends_with(&format!(".{cd}"));
            if !visible {
                continue;
            }
            if let Some(items) = list.as_array() {
                for item in items {
                    let name = item.get("name").and_then(|v| v.as_str());
                    let value = item.get("value").and_then(|v| v.as_str());
                    if let (Some(name), Some(value)) = (name, value) {
                        cookies.push(Cookie {
                            name: name.to_string(),
                            value: value.to_string(),
                        });
                    }
                }
            }
        }
    }
    if cookies.is_empty() {
        return Err(UsageError::MissingCredential(format!(
            "CookieCloud 中没有域名 {domain} 的 cookie，请确认浏览器已登录并同步"
        )));
    }
    Ok(cookies)
}

/// 拼接 `Cookie` 请求头值（`a=1; b=2`）。
pub fn cookie_header(cookies: &[Cookie]) -> String {
    cookies
        .iter()
        .map(|c| format!("{}={}", c.name, c.value))
        .collect::<Vec<_>>()
        .join("; ")
}

/// 按名查找 cookie 值。
pub fn find_cookie<'a>(cookies: &'a [Cookie], name: &str) -> Option<&'a str> {
    cookies
        .iter()
        .find(|c| c.name == name)
        .map(|c| c.value.as_str())
}

/// 解密 CookieCloud 的 encrypted 字段（纯函数，便于测试）。
pub fn decrypt_payload(uuid: &str, password: &str, encrypted_b64: &str) -> Result<String, UsageError> {
    // 密钥材料：md5("{uuid}-{password}") hex 前 16 字符。
    let material_hex = hex::encode(Md5::digest(format!("{uuid}-{password}").as_bytes()));
    let passphrase = &material_hex[..16];

    let decoded = BASE64
        .decode(encrypted_b64.trim())
        .map_err(|e| UsageError::Parse(format!("CookieCloud 密文 base64 解码失败：{e}")))?;
    if decoded.len() < 16 || &decoded[..8] != b"Salted__" {
        return Err(UsageError::Parse(
            "CookieCloud 密文缺少 Salted__ 头".to_string(),
        ));
    }
    let salt = &decoded[8..16];
    let ciphertext = &decoded[16..];

    let (key, iv) = evp_bytes_to_key_md5(passphrase.as_bytes(), salt, 48);
    let plaintext = Aes256CbcDec::new((&key[..32]).into(), (&iv[..16]).into())
        .decrypt_padded_vec_mut::<Pkcs7>(ciphertext)
        .map_err(|_| UsageError::Auth)?; // 解密失败多为密码错误
    String::from_utf8(plaintext)
        .map_err(|e| UsageError::Parse(format!("CookieCloud 解密结果不是 UTF-8：{e}")))
}

/// OpenSSL EVP_BytesToKey（MD5、1 轮）：循环 md5(prev + password + salt) 拼接至所需长度。
fn evp_bytes_to_key_md5(password: &[u8], salt: &[u8], key_len: usize) -> (Vec<u8>, Vec<u8>) {
    let mut derived = Vec::new();
    let mut prev: Vec<u8> = Vec::new();
    while derived.len() < key_len {
        let mut hasher = Md5::new();
        hasher.update(&prev);
        hasher.update(password);
        hasher.update(salt);
        prev = hasher.finalize().to_vec();
        derived.extend_from_slice(&prev);
    }
    let key = derived[..32].to_vec();
    let iv = derived[32..48].to_vec();
    (key, iv)
}

#[cfg(test)]
mod tests {
    use super::*;

    // 测试向量由 openssl 生成：
    //   material = md5("test-uuid-1234-test-pass")hex[..16] = dd2c72f1f5f06a42
    //   openssl enc -aes-256-cbc -md md5 -salt -base64 -A -pass pass:$material
    const ENCRYPTED: &str = "U2FsdGVkX1/lYHfBCo3T0Fi849akwqD3U3czPh8QlSaaZJ2XkVHUUpvCEeKad15y44NOTeEo6W1aY7wVK23u/641ZBr7O0qhjVjvMnLlXHW4bvvO39+bkVGSpWa8dRf6z9P/g+pn8wyqUZo4f1J2/E/gaet1PhR2KdjFfp8EOOWjzwyrxRNwsuc1VVdSRxQkJ0Zau+M2vltvyCOgkXV0mTj/W+VTffnJGxpd69iH1OPDslEbeLhBZKv+ico7f64E";

    #[test]
    fn decrypt_openssl_compatible_payload() {
        let plain =
            decrypt_payload("test-uuid-1234", "test-pass", ENCRYPTED).expect("decrypt failed");
        let v: serde_json::Value = serde_json::from_str(&plain).unwrap();
        assert_eq!(
            v["cookie_data"][".example.com"][0]["name"].as_str(),
            Some("sid")
        );
        assert_eq!(v["update_time"].as_i64(), Some(1700000000));
    }

    #[test]
    fn wrong_password_fails() {
        let result = decrypt_payload("test-uuid-1234", "wrong-pass", ENCRYPTED);
        assert!(result.is_err());
    }

    #[test]
    fn non_salted_payload_fails() {
        let bogus = BASE64.encode(b"not-a-salted-payload!!");
        assert!(decrypt_payload("u", "p", &bogus).is_err());
    }

    #[test]
    fn cookie_header_and_find() {
        let cookies = vec![
            Cookie {
                name: "a".to_string(),
                value: "1".to_string(),
            },
            Cookie {
                name: "b".to_string(),
                value: "2".to_string(),
            },
        ];
        assert_eq!(cookie_header(&cookies), "a=1; b=2");
        assert_eq!(find_cookie(&cookies, "b"), Some("2"));
        assert_eq!(find_cookie(&cookies, "c"), None);
    }
}

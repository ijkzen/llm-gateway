//! API 密钥加密。
//!
//! 使用 AES-256-GCM 对敏感字段(如 provider 的 api_key)做应用层加密后再落库。
//! 密钥来自环境变量 `API_KEY_ENCRYPTION_KEY`(任意长度),经 SHA-256 派生为 32 字节。
//! 未配置密钥时(如纯本地开发)退化为明文存储并打印 warn,便于开箱即用;
//! 生产环境必须配置密钥,否则敏感字段以明文落库。

use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use rand::RngCore;
use sha2::{Digest, Sha256};

/// 环境变量名:API 密钥加密密钥。
pub const ENCRYPTION_KEY_ENV: &str = "API_KEY_ENCRYPTION_KEY";

/// 密文前缀,用于区分"已加密"与"明文(未配置密钥时)"。
const CIPHER_PREFIX: &str = "enc:v1:";

/// 加密后是否真的发生了加密(区分"配置了密钥"与"退化明文")。
pub fn encryption_enabled() -> bool {
    std::env::var(ENCRYPTION_KEY_ENV)
        .map(|k| !k.trim().is_empty())
        .unwrap_or(false)
}

/// 从环境变量派生 AES-256 密钥。
fn derive_key(secret: &str) -> [u8; 32] {
    let digest = Sha256::digest(secret.as_bytes());
    let mut key = [0u8; 32];
    key.copy_from_slice(&digest);
    key
}

/// 加密明文。未配置密钥时返回原文(并 warn),已配置时返回 `enc:v1:<base64>`。
pub fn encrypt(plaintext: &str) -> String {
    if plaintext.is_empty() {
        return String::new();
    }
    let Some(secret) = std::env::var(ENCRYPTION_KEY_ENV)
        .ok()
        .filter(|k| !k.trim().is_empty())
    else {
        tracing::warn!(
            "{} not set; API keys will be stored in plaintext",
            ENCRYPTION_KEY_ENV
        );
        return plaintext.to_string();
    };

    let key = derive_key(&secret);
    let cipher = Aes256Gcm::new_from_slice(&key).expect("AES-256 key is always 32 bytes");

    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from(nonce_bytes);

    // encrypt 返回 ciphertext || tag,一次性追加到 nonce 之后。
    let ciphertext = cipher
        .encrypt(&nonce, plaintext.as_bytes())
        .expect("AES-GCM encryption is infallible");

    let mut blob = Vec::with_capacity(nonce_bytes.len() + ciphertext.len());
    blob.extend_from_slice(&nonce_bytes);
    blob.extend_from_slice(&ciphertext);

    format!("{CIPHER_PREFIX}{}", BASE64.encode(blob))
}

/// 解密。已加密值(带前缀)必须能解开,否则返回 Err;
/// 无前缀的值视为未加密(历史/开发环境数据)原样返回。
pub fn decrypt(ciphertext: &str) -> anyhow::Result<String> {
    if ciphertext.is_empty() {
        return Ok(String::new());
    }
    if let Some(encoded) = ciphertext.strip_prefix(CIPHER_PREFIX) {
        let secret = std::env::var(ENCRYPTION_KEY_ENV)
            .ok()
            .filter(|k| !k.trim().is_empty())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "{} not set; cannot decrypt stored API key",
                    ENCRYPTION_KEY_ENV
                )
            })?;
        let key = derive_key(&secret);
        let cipher = Aes256Gcm::new_from_slice(&key).expect("AES-256 key is always 32 bytes");

        let blob = BASE64
            .decode(encoded)
            .map_err(|e| anyhow::anyhow!("stored API key is not valid base64: {e}"))?;
        if blob.len() < 13 {
            return Err(anyhow::anyhow!(
                "stored API key is too short to be a valid ciphertext"
            ));
        }
        let (nonce_bytes, ciphertext_bytes) = blob.split_at(12);
        let mut nonce_arr = [0u8; 12];
        nonce_arr.copy_from_slice(nonce_bytes);
        let plaintext = cipher
            .decrypt(&Nonce::from(nonce_arr), ciphertext_bytes)
            .map_err(|_| anyhow::anyhow!("failed to decrypt stored API key (wrong key?)"))?;
        String::from_utf8(plaintext)
            .map_err(|e| anyhow::anyhow!("decrypted API key is not UTF-8: {e}"))
    } else {
        // 未加密的旧数据/开发环境明文,原样返回。
        Ok(ciphertext.to_string())
    }
}

/// 对明文密钥做掩码:保留前 3 位与后 4 位,中间用星号填充;
/// 长度不足(≤7 字符)时整体打码,空串返回空串。
pub fn mask(plain: &str) -> String {
    if plain.is_empty() {
        return String::new();
    }
    let bytes = plain.as_bytes();
    if bytes.len() <= 7 {
        return "*".repeat(bytes.len());
    }
    let head: String = plain.chars().take(3).collect();
    let tail: String = plain
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{head}****{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "test-encryption-key";

    fn with_key<T>(key: Option<&str>, f: impl FnOnce() -> T) -> T {
        temp_env::with_vars([(ENCRYPTION_KEY_ENV, key)], || {
            // temp_env 之外的环境可能残留变量,显式清理。
            f()
        })
    }

    #[test]
    fn encrypt_roundtrip_with_key() {
        with_key(Some(KEY), || {
            let ciphertext = encrypt("sk-test-123");
            assert!(ciphertext.starts_with(CIPHER_PREFIX));
            assert_ne!(ciphertext, "sk-test-123");
            assert_eq!(decrypt(&ciphertext).unwrap(), "sk-test-123");
        });
    }

    #[test]
    fn encrypt_produces_different_ciphertext_each_time() {
        with_key(Some(KEY), || {
            let a = encrypt("same-secret");
            let b = encrypt("same-secret");
            assert_ne!(a, b, "random nonce must produce distinct ciphertext");
            assert_eq!(decrypt(&a).unwrap(), decrypt(&b).unwrap());
        });
    }

    #[test]
    fn encrypt_without_key_falls_back_to_plaintext() {
        with_key(None, || {
            let out = encrypt("sk-plain");
            assert_eq!(out, "sk-plain");
            assert_eq!(decrypt(&out).unwrap(), "sk-plain");
        });
    }

    #[test]
    fn decrypt_plaintext_without_prefix_returns_as_is() {
        with_key(Some(KEY), || {
            // 历史明文数据(无前缀)原样返回,即使配置了密钥也不报错。
            assert_eq!(decrypt("sk-legacy-plain").unwrap(), "sk-legacy-plain");
        });
    }

    #[test]
    fn decrypt_without_key_errors_on_ciphertext() {
        let ciphertext = with_key(Some(KEY), || encrypt("sk-needs-key"));
        with_key(None, || {
            let err = decrypt(&ciphertext).unwrap_err();
            assert!(err.to_string().contains(ENCRYPTION_KEY_ENV));
        });
    }

    #[test]
    fn decrypt_tampered_ciphertext_errors() {
        with_key(Some(KEY), || {
            let ciphertext = encrypt("sk-authentic");
            // 翻转一个 base64 字符,破坏认证标签。
            let pos = ciphertext.len() - 1;
            let byte = ciphertext.as_bytes()[pos];
            let tampered = format!(
                "{}{}",
                &ciphertext[..pos],
                if byte == b'A' { b'B' } else { b'A' } as char
            );
            assert!(decrypt(&tampered).is_err());
        });
    }

    #[test]
    fn encrypt_empty_returns_empty() {
        with_key(Some(KEY), || {
            assert_eq!(encrypt(""), "");
            assert_eq!(decrypt("").unwrap(), "");
        });
    }

    #[test]
    fn wrong_key_fails_to_decrypt() {
        let ciphertext = with_key(Some("key-a"), || encrypt("sk-cross-key"));
        with_key(Some("key-b"), || {
            assert!(decrypt(&ciphertext).is_err());
        });
    }

    #[test]
    fn mask_keeps_head_and_tail() {
        assert_eq!(mask("sk-secret-1234"), "sk-****1234");
        assert_eq!(mask("lg-0123456789abcdef0123456789abcdef"), "lg-****cdef");
    }

    #[test]
    fn mask_short_and_empty_values() {
        assert_eq!(mask(""), "");
        assert_eq!(mask("abc"), "***");
        assert_eq!(mask("1234567"), "*******");
        assert_eq!(mask("12345678"), "123****5678");
    }
}

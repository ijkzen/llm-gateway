//! 各厂商用量查询 fetcher。
//!
//! 每个 fetcher 形如 `fetch(http, creds) -> Result<FetchOutput, UsageError>`，
//! 真实端点为模块内 const；解析逻辑拆成纯函数便于夹具单测。

pub mod alibaba;
pub mod api_key;
pub mod balance;
pub mod cloud_balance;
pub mod copilot;
pub mod krill;
pub mod sensenova;
pub mod stepfun;
pub mod volcengine;
pub mod xiaomi;

use chrono::{DateTime, Utc};
use serde_json::{Map, Value};

use super::cookiecloud::CookieCloudConfig;
use super::error::UsageError;
use super::types::ts_ms;

/// 一次用量查询可用的凭据：解密后的 api_key + provider extra 字段。
pub struct Credentials<'a> {
    pub api_key: &'a str,
    pub extra: &'a Map<String, Value>,
}

/// SenseNova fetcher 需要的最小上下文：登录/轮换写回 refresh_token 时要
/// 重读 provider 行并加密更新，因此传入 db + provider_id。
pub struct SensenovaContext<'a> {
    pub db: &'a sea_orm::DatabaseConnection,
    pub provider_id: i32,
}

impl Credentials<'_> {
    pub fn api_key_required(&self) -> Result<&str, UsageError> {
        let key = self.api_key.trim();
        if key.is_empty() {
            return Err(UsageError::MissingCredential("api_key".to_string()));
        }
        Ok(key)
    }

    pub fn extra_str(&self, key: &str) -> Option<&str> {
        self.extra
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }

    pub fn require(&self, key: &str) -> Result<&str, UsageError> {
        self.extra_str(key)
            .ok_or_else(|| UsageError::MissingCredential(key.to_string()))
    }

    /// CookieCloud 类供应商的凭据组（server/uuid/password + 目标域名）。
    pub fn cookiecloud(&self) -> Result<(CookieCloudConfig, String), UsageError> {
        let cfg = CookieCloudConfig {
            server: self.require("cookie_cloud_server")?.to_string(),
            uuid: self.require("uuid")?.to_string(),
            password: self.require("password")?.to_string(),
        };
        let domain = self.require("domain")?.to_string();
        Ok((cfg, domain))
    }
}

/// 错误消息中携带的响应体片段（截断 200 字符）。
pub(crate) fn snippet(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.chars().count() > 200 {
        format!("{}…", trimmed.chars().take(200).collect::<String>())
    } else {
        trimmed.to_string()
    }
}

/// 兼容数值与字符串数字（如 Kimi 的 limit/used/remaining 是字符串）。
pub(crate) fn num(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.trim().parse::<f64>().ok()))
}

/// 东八区固定偏移，用于解释厂商返回的无时区时间字符串。
const CN_TZ: chrono::FixedOffset =
    chrono::FixedOffset::east_opt(8 * 3600).expect("+08:00 固定偏移");

/// 兼容多种重置时间写法：ISO/RFC3339 字符串、`yyyy-MM-dd HH:mm` 字符串、
/// 毫秒或秒级时间戳（字段名变体由调用方逐个尝试后传入）。
pub(crate) fn reset_ts(v: &Value) -> Option<DateTime<Utc>> {
    if let Some(s) = v.as_str() {
        if let Some(ts) = super::types::ts_iso(s) {
            return Some(ts);
        }
        // 国内厂商（阿里云/小米等）返回的 "2026-09-14 12:00" 不带时区，
        // 其账期结束时间按东八区解释（与浏览器用户所在时区一致）。
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M") {
            return Some(naive.and_local_timezone(CN_TZ).single()?.into());
        }
        return None;
    }
    let n = v.as_i64()?;
    if n > 1_000_000_000_000 {
        ts_ms(n)
    } else {
        super::types::ts_secs(n)
    }
}

/// 依次尝试多个字段名取重置时间。
pub(crate) fn reset_ts_of(v: &Value, keys: &[&str]) -> Option<DateTime<Utc>> {
    keys.iter().filter_map(|k| v.get(*k)).find_map(reset_ts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_ts_naive_string_interpreted_as_cn_tz() {
        // 无时区 "yyyy-MM-dd HH:mm"（阿里云/小米风格）按东八区解释：
        // 东八区 2026-09-14 12:00 = UTC 2026-09-14 04:00。
        let v = serde_json::json!("2026-09-14 12:00");
        let ts = reset_ts(&v).expect("naive string should parse");
        assert_eq!(
            ts,
            DateTime::parse_from_rfc3339("2026-09-14T04:00:00Z").unwrap()
        );
    }

    #[test]
    fn reset_ts_iso_string_keeps_utc() {
        // 带时区的 ISO 字符串不受 CN_TZ 影响。
        let v = serde_json::json!("2026-09-14T12:00:00+08:00");
        let ts = reset_ts(&v).expect("iso string should parse");
        assert_eq!(
            ts,
            DateTime::parse_from_rfc3339("2026-09-14T04:00:00Z").unwrap()
        );
    }
}

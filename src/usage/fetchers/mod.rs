//! 各厂商用量查询 fetcher。
//!
//! 每个 fetcher 形如 `fetch(http, creds) -> Result<FetchOutput, UsageError>`，
//! 真实端点为模块内 const；解析逻辑拆成纯函数便于夹具单测。

pub mod alibaba;
pub mod api_key;
pub mod balance;
pub mod cloud_balance;
pub mod copilot;
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

/// 兼容多种重置时间写法：ISO/RFC3339 字符串、`yyyy-MM-dd HH:mm` 字符串、
/// 毫秒或秒级时间戳（字段名变体由调用方逐个尝试后传入）。
pub(crate) fn reset_ts(v: &Value) -> Option<DateTime<Utc>> {
    if let Some(s) = v.as_str() {
        if let Some(ts) = super::types::ts_iso(s) {
            return Some(ts);
        }
        // 阿里云风格的 "2026-09-14 12:00"（按 UTC 解析，仅作展示参考）。
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M") {
            return Some(naive.and_utc());
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
    keys.iter()
        .filter_map(|k| v.get(*k))
        .find_map(reset_ts)
}

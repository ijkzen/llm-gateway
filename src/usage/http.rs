//! 用量查询的 HTTP 客户端封装（reqwest，15s 超时）。
//!
//! 与 `proxy::upstream` 不同，用量查询不需要精确建连计时，
//! 且需要 GET/form 编码等代理路径不支持的方法，故独立封装。
//!
//! 集成测试可设置环境变量 `LLM_GATEWAY_USAGE_HTTP_OVERRIDE` 将所有请求
//! 的 scheme+host 重定向到本地 mock（路径与 query 保留）。

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

use super::error::UsageError;

const TIMEOUT_SECS: u64 = 15;
const USER_AGENT: &str = concat!("llm-gateway/", env!("CARGO_PKG_VERSION"));
const OVERRIDE_ENV: &str = "LLM_GATEWAY_USAGE_HTTP_OVERRIDE";

pub struct UsageHttp {
    client: reqwest::Client,
    /// 测试用：将请求重定向到该 base（如 `http://127.0.0.1:PORT`）。
    base_override: Option<String>,
}

pub struct HttpReply {
    pub status: u16,
    pub body: String,
}

impl UsageHttp {
    pub fn new() -> Self {
        Self::with_proxy(None)
    }

    /// 指定 HTTP 代理（`http://host:port`，无认证）创建客户端。
    ///
    /// 供 provider 级代理透传使用：用量抓取若也需经网络代理访问厂商端点，
    /// 调用方把 `provider.proxy_addr` 传进来。
    pub fn with_proxy(proxy_addr: Option<&str>) -> Self {
        let mut builder = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
            .user_agent(USER_AGENT);
        if let Some(addr) = proxy_addr.map(str::trim).filter(|a| !a.is_empty()) {
            // 地址已由 provider 校验过（http:// 开头、无认证）；解析失败按无代理降级。
            // 用 Proxy::all：Proxy::http 只拦截 http:// URL，https 供应商会直连。
            if let Ok(proxy) = reqwest::Proxy::all(addr) {
                builder = builder.proxy(proxy);
            }
        }
        let client = builder.build().expect("reqwest client build is infallible");
        let base_override = std::env::var(OVERRIDE_ENV)
            .ok()
            .filter(|v| !v.trim().is_empty());
        Self {
            client,
            base_override,
        }
    }

    pub async fn get(
        &self,
        url: &str,
        headers: &[(&str, String)],
    ) -> Result<HttpReply, UsageError> {
        self.send(reqwest::Method::GET, url, headers, None).await
    }

    pub async fn post_json(
        &self,
        url: &str,
        headers: &[(&str, String)],
        body: &str,
    ) -> Result<HttpReply, UsageError> {
        let mut owned: Vec<(&str, String)> = headers.to_vec();
        owned.push(("Content-Type", "application/json".to_string()));
        self.send(reqwest::Method::POST, url, &owned, Some(body.to_string()))
            .await
    }

    /// body 为已编码的 form 字符串（`a=1&b=2`）。
    pub async fn post_form(
        &self,
        url: &str,
        headers: &[(&str, String)],
        body: &str,
    ) -> Result<HttpReply, UsageError> {
        let mut owned: Vec<(&str, String)> = headers.to_vec();
        owned.push((
            "Content-Type",
            "application/x-www-form-urlencoded".to_string(),
        ));
        self.send(reqwest::Method::POST, url, &owned, Some(body.to_string()))
            .await
    }

    async fn send(
        &self,
        method: reqwest::Method,
        url: &str,
        headers: &[(&str, String)],
        body: Option<String>,
    ) -> Result<HttpReply, UsageError> {
        let url = self.rewrite_url(url);
        let mut map = HeaderMap::new();
        for (name, value) in headers {
            let name = HeaderName::try_from(name.to_ascii_lowercase())
                .map_err(|e| UsageError::Parse(format!("非法请求头名 {name}：{e}")))?;
            let value = HeaderValue::from_str(value)
                .map_err(|e| UsageError::Parse(format!("非法请求头值：{e}")))?;
            map.insert(name, value);
        }
        let mut req = self.client.request(method, &url).headers(map);
        if let Some(body) = body {
            req = req.body(body);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| UsageError::Network(e.to_string()))?;
        let status = resp.status().as_u16();
        let body = resp
            .text()
            .await
            .map_err(|e| UsageError::Network(e.to_string()))?;
        Ok(HttpReply { status, body })
    }

    fn rewrite_url(&self, url: &str) -> String {
        match &self.base_override {
            Some(base) => {
                let path_and_query = url
                    .split_once("://")
                    .and_then(|(_, rest)| rest.find('/').map(|i| &rest[i..]))
                    .unwrap_or("/");
                format!("{}{}", base.trim_end_matches('/'), path_and_query)
            }
            None => url.to_string(),
        }
    }
}

impl Default for UsageHttp {
    fn default() -> Self {
        Self::new()
    }
}

/// 解析 JSON 响应体。
pub fn parse_json(reply: &HttpReply) -> Result<serde_json::Value, UsageError> {
    serde_json::from_str(&reply.body)
        .map_err(|e| UsageError::Parse(format!("响应不是合法 JSON：{e}")))
}

/// 常见鉴权失败判定：401/403 一律视为凭据失效。
pub fn ensure_not_auth_error(reply: &HttpReply) -> Result<(), UsageError> {
    if reply.status == 401 || reply.status == 403 {
        return Err(UsageError::Auth);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrite_url_replaces_scheme_and_host() {
        let http = UsageHttp {
            client: reqwest::Client::new(),
            base_override: Some("http://127.0.0.1:9000".to_string()),
        };
        assert_eq!(
            http.rewrite_url("https://api.deepseek.com/user/balance?x=1"),
            "http://127.0.0.1:9000/user/balance?x=1"
        );
        assert_eq!(
            http.rewrite_url("https://open.bigmodel.cn"),
            "http://127.0.0.1:9000/"
        );
    }

    #[test]
    fn rewrite_url_passthrough_without_override() {
        let http = UsageHttp {
            client: reqwest::Client::new(),
            base_override: None,
        };
        assert_eq!(http.rewrite_url("https://a.com/b"), "https://a.com/b");
    }
}

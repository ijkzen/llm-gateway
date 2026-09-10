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

/// 按代理维度缓存的 reqwest 客户端（key：代理地址，空串=直连）。
/// 进程级单例：连接池/TLS 会话跨刷新轮次复用，连接池内部自行回收空闲连接。
fn cached_client(proxy_addr: Option<&str>) -> reqwest::Client {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    static CLIENTS: OnceLock<Mutex<HashMap<String, reqwest::Client>>> = OnceLock::new();
    let key = proxy_addr.unwrap_or("").trim().to_string();
    CLIENTS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap()
        .entry(key)
        .or_insert_with(|| build_client(proxy_addr))
        .clone()
}

fn build_client(proxy_addr: Option<&str>) -> reqwest::Client {
    let mut builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
        .user_agent(USER_AGENT)
        // 禁自动重定向（归位一治理）：302/303 到登录页被跟随会变 200，
        // 使各家 fetcher 的「3xx = 会话失效」守卫形同虚设。让 3xx 真实
        // 到达判定层；CookieCloud 服务器若做 http→https 跳转需显式处理。
        .redirect(reqwest::redirect::Policy::none());
    if let Some(addr) = proxy_addr.map(str::trim).filter(|a| !a.is_empty()) {
        // 地址已由 provider 校验过（http:// 开头、无认证）；解析失败按无代理降级。
        // 用 Proxy::all：Proxy::http 只拦截 http:// URL，https 供应商会直连。
        if let Ok(proxy) = reqwest::Proxy::all(addr) {
            builder = builder.proxy(proxy);
        }
    }
    builder.build().expect("reqwest client build is infallible")
}

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
    ///
    /// 客户端按代理维度（直连/代理地址）进程级复用（P5）：reqwest 连接池与
    /// TLS 会话随客户端存活，避免每 5 分钟一轮刷新为每家重建客户端导致
    /// 全部连接池/TLS 会话随 drop 丢弃、每轮全部厂商重新握手。
    pub fn with_proxy(proxy_addr: Option<&str>) -> Self {
        let base_override = std::env::var(OVERRIDE_ENV)
            .ok()
            .filter(|v| !v.trim().is_empty());
        Self {
            client: cached_client(proxy_addr),
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

/// 会话失效统一判定谓词（归位一治理单源）：401/403 与 3xx 一律视为凭据失效。
/// 3xx 只有在客户端禁自动重定向下才可达（见 `build_client`）；重定向到登录页
/// 是 cookie 族最典型的失效信号。
pub fn is_session_invalid_status(status: u16) -> bool {
    status == 401 || status == 403 || (300..400).contains(&status)
}

/// 常见鉴权失败判定：会话失效状态码一律视为凭据失效（401/403/3xx）。
pub fn ensure_not_auth_error(reply: &HttpReply) -> Result<(), UsageError> {
    if is_session_invalid_status(reply.status) {
        return Err(UsageError::Auth);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 归位一治理：统一会话失效判定谓词（401/403/3xx 三形态，其余不判）。
    #[test]
    fn session_invalid_status_covers_401_403_3xx_only() {
        for status in [401, 403, 300, 301, 302, 303, 307, 308, 399] {
            assert!(is_session_invalid_status(status), "{status} 应判失效");
        }
        for status in [200, 204, 400, 402, 404, 429, 500, 502] {
            assert!(!is_session_invalid_status(status), "{status} 不应判失效");
        }
    }

    /// ensure_not_auth_error 与谓词同源：3xx 也归 Auth（客户端已禁自动重定向）。
    #[test]
    fn ensure_not_auth_error_treats_3xx_as_auth() {
        for status in [401, 403, 302] {
            let reply = HttpReply {
                status,
                body: String::new(),
            };
            assert!(matches!(
                ensure_not_auth_error(&reply),
                Err(UsageError::Auth)
            ));
        }
        let ok = HttpReply {
            status: 200,
            body: String::new(),
        };
        assert!(ensure_not_auth_error(&ok).is_ok());
    }

    /// 禁自动重定向：302 不能被跟随成 200（3xx 守卫可达性的前提）。
    #[tokio::test]
    async fn client_does_not_follow_redirects() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = sock.read(&mut buf).await;
            // 302 到 /login（若客户端跟随会在同一连接上收到第二个请求）。
            let _ = sock
                .write_all(b"HTTP/1.1 302 Found\r\nLocation: /login\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await;
            // 保持连接短暂打开，若被跟随则会再读到请求。
            let _ =
                tokio::time::timeout(std::time::Duration::from_millis(150), sock.read(&mut buf))
                    .await;
        });
        let http = UsageHttp {
            client: build_client(None),
            base_override: None,
        };
        let reply = http
            .get(&format!("http://{addr}/api/me"), &[])
            .await
            .unwrap();
        assert_eq!(reply.status, 302, "重定向应原样返回而非被跟随");
    }

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

//! 上游 HTTP 客户端。
//!
//! 连接按 `scheme://host:port` 池化复用：首次请求独立建连（精确测量 TCP 建连
//! 与 TLS 握手耗时），响应体读完连接归还池，后续请求直接复用；连接空闲超过
//! 10 分钟被释放。仅支持 HTTP/1.1 上游。
//!
//! 指标语义：`UpstreamReply::start_at_ms` 是本次请求的网络阶段起点（新建连接
//! = TCP 建连开始时刻，复用连接 = 请求发出时刻），作为 TTFT 与新 tps 的计时
//! 起点（复用连接时即请求发出时刻，不做额外的建连完成近似）。

use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::client::conn::http1;
use hyper::header::{CONTENT_LENGTH, CONTENT_TYPE, HOST};
use hyper::http::request::Builder;
use hyper::{Method, StatusCode, Uri};
use hyper_util::rt::TokioIo;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::TcpStream;

use crate::proxy::metrics::now_ms;
use crate::proxy::pool::{PooledBody, UpstreamPool};

/// 建连各阶段耗时（毫秒）。
#[derive(Debug, Clone, Copy, Default)]
pub struct ConnectTiming {
    pub tcp_ms: u64,
    pub tls_ms: u64,
}

impl ConnectTiming {
    pub fn total_ms(&self) -> u64 {
        self.tcp_ms + self.tls_ms
    }
}

/// TCP 建连超时。
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// TLS 握手超时。
pub const TLS_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// 等待上游响应头的超时（流式与非流式一致；流式的响应体本身不设总超时）。
pub const HEADER_TIMEOUT: Duration = Duration::from_secs(120);
/// 非流式响应体读取超时。
pub const NON_STREAM_BODY_TIMEOUT: Duration = Duration::from_secs(120);

/// 四组超时的可注入集合：生产用 `Default`（即上面的常量），测试经
/// `UpstreamPool::with_timeouts` 传毫秒级值缩短超时路径（silent upstream /
/// 体悬挂等零覆盖分支）。
#[derive(Debug, Clone, Copy)]
pub struct Timeouts {
    pub connect: Duration,
    pub tls_handshake: Duration,
    pub header: Duration,
    pub non_stream_body: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            connect: CONNECT_TIMEOUT,
            tls_handshake: TLS_HANDSHAKE_TIMEOUT,
            header: HEADER_TIMEOUT,
            non_stream_body: NON_STREAM_BODY_TIMEOUT,
        }
    }
}

/// 上游调用错误。
#[derive(Debug, thiserror::Error)]
pub enum UpstreamError {
    /// 建连阶段失败（DNS/TCP/TLS），可 failover 到下一个成员。
    #[error("上游连接失败：{0}")]
    Connect(String),
    /// 请求发送或响应读取失败。
    #[error("上游请求失败：{0}")]
    Request(String),
    #[error("上游请求超时")]
    Timeout,
}

impl UpstreamError {
    pub fn fail_reason(&self) -> String {
        self.to_string()
    }
}

/// 已建立（并计时）的上游连接流。
pub enum TimedStream {
    Plain(TcpStream),
    Tls(Pin<Box<tokio_rustls::client::TlsStream<TcpStream>>>),
}

impl AsyncRead for TimedStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            TimedStream::Plain(s) => Pin::new(s).poll_read(cx, buf),
            TimedStream::Tls(s) => s.as_mut().poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for TimedStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            TimedStream::Plain(s) => Pin::new(s).poll_write(cx, buf),
            TimedStream::Tls(s) => s.as_mut().poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            TimedStream::Plain(s) => Pin::new(s).poll_flush(cx),
            TimedStream::Tls(s) => s.as_mut().poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            TimedStream::Plain(s) => Pin::new(s).poll_shutdown(cx),
            TimedStream::Tls(s) => s.as_mut().poll_shutdown(cx),
        }
    }
}

/// 进程级共享 TLS 配置：OnceLock 缓存 Arc，连接池 miss 建新连接时只
/// 克隆 Arc（rustls ClientConfig 的 Clone 非纯浅拷贝，P6）。
fn tls_config() -> &'static std::sync::Arc<tokio_rustls::rustls::ClientConfig> {
    static CONFIG: std::sync::OnceLock<std::sync::Arc<tokio_rustls::rustls::ClientConfig>> =
        std::sync::OnceLock::new();
    CONFIG.get_or_init(|| {
        let mut roots = tokio_rustls::rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        std::sync::Arc::new(
            tokio_rustls::rustls::ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth(),
        )
    })
}

/// 解析上游 URL 为 (scheme, host, port, path_query)。
pub fn parse_url(url: &str) -> Result<(String, String, u16, String), UpstreamError> {
    let uri: Uri = url
        .parse()
        .map_err(|e| UpstreamError::Connect(format!("上游 URL 无效（{url}）：{e}")))?;
    let scheme = uri.scheme_str().unwrap_or("http").to_string();
    let host = uri
        .host()
        .ok_or_else(|| UpstreamError::Connect(format!("上游 URL 缺少主机名（{url}）")))?
        .to_string();
    // IPv6 字面量（Uri::host() 返回带方括号形态）：无任何可用路径——带括号串
    // 交给 getaddrinfo 会报「DNS 解析失败」、TLS 侧报「主机名无效」都指向错误
    // 原因。显式拒绝并说明（真支持需去括号 + rustls ServerName::IpAddress）。
    if host.starts_with('[') {
        return Err(UpstreamError::Connect(format!(
            "暂不支持 IPv6 字面量上游地址（{host}），请使用 IPv4 或域名"
        )));
    }
    let port = uri
        .port_u16()
        .unwrap_or(if scheme == "https" { 443 } else { 80 });
    let path_query = match (uri.path(), uri.query()) {
        ("", Some(q)) => format!("/?{q}"),
        ("", None) => "/".to_string(),
        (p, Some(q)) => format!("{p}?{q}"),
        (p, None) => p.to_string(),
    };
    Ok((scheme, host, port, path_query))
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// 建立 TCP（可选 TLS）连接并记录各阶段耗时。DNS 解析不计入。
/// 返回 (连接, 各阶段耗时, 建连开始 wall-clock 毫秒时间戳)。
/// `proxy` 为 `Some(代理地址)` 时经 HTTP CONNECT 隧道建连到目标。
async fn connect_stream(
    scheme: &str,
    host: &str,
    port: u16,
    proxy: Option<&str>,
    timeouts: Timeouts,
) -> Result<(TimedStream, ConnectTiming, i64), UpstreamError> {
    let connect_start_at_ms = now_ms();

    // 代理模式：连接代理服务器 → CONNECT host:port 建立隧道 → https 时隧道内 TLS。
    if let Some(proxy_addr) = proxy {
        return connect_via_proxy(
            proxy_addr,
            scheme,
            host,
            port,
            connect_start_at_ms,
            timeouts,
        )
        .await;
    }

    let addrs: Vec<std::net::SocketAddr> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| UpstreamError::Connect(format!("DNS 解析失败（{host}）：{e}")))?
        .collect();
    if addrs.is_empty() {
        return Err(UpstreamError::Connect(format!(
            "DNS 解析不到可用地址（{host}）"
        )));
    }

    let mut last_err: Option<UpstreamError> = None;
    for addr in addrs {
        let tcp_started = Instant::now();
        let stream = match tokio::time::timeout(timeouts.connect, TcpStream::connect(addr)).await {
            Ok(Ok(stream)) => stream,
            Ok(Err(e)) => {
                last_err = Some(UpstreamError::Connect(format!("连接 {addr} 失败：{e}")));
                continue;
            }
            Err(_) => {
                last_err = Some(UpstreamError::Connect(format!("连接 {addr} 超时")));
                continue;
            }
        };
        let tcp_ms = elapsed_ms(tcp_started);
        stream.set_nodelay(true).ok();

        if scheme != "https" {
            return Ok((
                TimedStream::Plain(stream),
                ConnectTiming { tcp_ms, tls_ms: 0 },
                connect_start_at_ms,
            ));
        }

        let tls_started = Instant::now();
        let server_name =
            tokio_rustls::rustls::pki_types::ServerName::try_from(host.to_string())
                .map_err(|e| UpstreamError::Connect(format!("TLS 主机名无效（{host}）：{e}")))?;
        let connector = tokio_rustls::TlsConnector::from(tls_config().clone());
        let tls = match tokio::time::timeout(
            timeouts.tls_handshake,
            connector.connect(server_name, stream),
        )
        .await
        {
            Ok(Ok(tls)) => tls,
            Ok(Err(e)) => {
                return Err(UpstreamError::Connect(format!("TLS 握手失败：{e}")));
            }
            Err(_) => {
                return Err(UpstreamError::Connect("TLS 握手超时".to_string()));
            }
        };
        return Ok((
            TimedStream::Tls(Box::pin(tls)),
            ConnectTiming {
                tcp_ms,
                tls_ms: elapsed_ms(tls_started),
            },
            connect_start_at_ms,
        ));
    }
    Err(last_err.unwrap_or_else(|| UpstreamError::Connect("连接失败".to_string())))
}

/// 经 HTTP 代理（CONNECT 隧道）建连到目标 host:port。
/// - 连代理服务器（代理地址 `http://host:port`，无认证）
/// - 发送 `CONNECT host:port HTTP/1.1`，读响应头确认 200
/// - https 目标：在隧道内做目标侧 TLS 握手；http 目标：直接返回隧道流
async fn connect_via_proxy(
    proxy_addr: &str,
    scheme: &str,
    host: &str,
    port: u16,
    connect_start_at_ms: i64,
    timeouts: Timeouts,
) -> Result<(TimedStream, ConnectTiming, i64), UpstreamError> {
    // 解析代理地址（http://host:port）。
    let proxy_url = proxy_addr.trim().strip_prefix("http://").ok_or_else(|| {
        UpstreamError::Connect(format!("代理地址需以 http:// 开头（{proxy_addr}）"))
    })?;
    // 防御：带认证（user:pass@）的地址直接拒绝（无认证代理限制）。
    if proxy_url.contains('@') {
        return Err(UpstreamError::Connect(format!(
            "暂不支持带认证的代理地址（{proxy_addr}）"
        )));
    }
    let (proxy_host, proxy_port) = match proxy_url.rsplit_once(':') {
        Some((h, p)) => {
            let p: u16 = p
                .parse()
                .map_err(|_| UpstreamError::Connect(format!("代理地址端口无效（{proxy_addr}）")))?;
            (h.to_string(), p)
        }
        None => (proxy_url.to_string(), 80),
    };

    let tcp_started = Instant::now();
    let proxy_addrs: Vec<std::net::SocketAddr> =
        tokio::net::lookup_host((proxy_host.as_str(), proxy_port))
            .await
            .map_err(|e| UpstreamError::Connect(format!("代理 DNS 解析失败（{proxy_host}）：{e}")))?
            .collect();
    let mut last_err = None;
    let mut stream = None;
    for addr in proxy_addrs {
        match tokio::time::timeout(timeouts.connect, TcpStream::connect(addr)).await {
            Ok(Ok(s)) => {
                stream = Some(s);
                break;
            }
            Ok(Err(e)) => last_err = Some(e),
            Err(_) => {
                last_err = Some(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "connect timeout",
                ))
            }
        }
    }
    let mut stream = stream.ok_or_else(|| {
        UpstreamError::Connect(format!(
            "连接代理 {proxy_host}:{proxy_port} 失败：{}",
            last_err
                .map(|e| e.to_string())
                .unwrap_or_else(|| "无可用地址".into())
        ))
    })?;
    stream.set_nodelay(true).ok();
    let tcp_ms = elapsed_ms(tcp_started);

    // 发送 CONNECT 建立隧道。
    let connect_req = format!("CONNECT {host}:{port} HTTP/1.1\r\nHost: {host}:{port}\r\n\r\n");
    tokio::time::timeout(timeouts.connect, stream.write_all(connect_req.as_bytes()))
        .await
        .map_err(|_| UpstreamError::Connect("代理 CONNECT 写入超时".to_string()))?
        .map_err(|e| UpstreamError::Connect(format!("代理 CONNECT 写入失败：{e}")))?;
    stream.flush().await.ok();

    // 读 CONNECT 响应头（直到 \r\n\r\n），校验 200。
    let status = read_proxy_connect_response(&mut stream, timeouts.connect)
        .await
        .map_err(|e| UpstreamError::Connect(format!("代理 CONNECT 响应解析失败：{e}")))?;
    if status != 200 {
        return Err(UpstreamError::Connect(format!(
            "代理 CONNECT 返回 {status}（目标 {host}:{port}）"
        )));
    }

    // https 目标：隧道内 TLS 握手；http 目标：直接返回隧道流。
    if scheme != "https" {
        return Ok((
            TimedStream::Plain(stream),
            ConnectTiming { tcp_ms, tls_ms: 0 },
            connect_start_at_ms,
        ));
    }
    let tls_started = Instant::now();
    let server_name = tokio_rustls::rustls::pki_types::ServerName::try_from(host.to_string())
        .map_err(|e| UpstreamError::Connect(format!("TLS 主机名无效（{host}）：{e}")))?;
    let connector = tokio_rustls::TlsConnector::from(tls_config().clone());
    let tls = match tokio::time::timeout(
        timeouts.tls_handshake,
        connector.connect(server_name, stream),
    )
    .await
    {
        Ok(Ok(tls)) => tls,
        Ok(Err(e)) => {
            return Err(UpstreamError::Connect(format!(
                "代理隧道 TLS 握手失败：{e}"
            )));
        }
        Err(_) => return Err(UpstreamError::Connect("代理隧道 TLS 握手超时".to_string())),
    };
    Ok((
        TimedStream::Tls(Box::pin(tls)),
        ConnectTiming {
            tcp_ms,
            tls_ms: elapsed_ms(tls_started),
        },
        connect_start_at_ms,
    ))
}

/// 读取代理 CONNECT 响应的状态行（如 `HTTP/1.1 200 Connection established`），
/// 返回状态码。响应头读到 `\r\n\r\n` 为止（剩余字节留在流中给后续握手）。
async fn read_proxy_connect_response<S: tokio::io::AsyncRead + Unpin>(
    stream: &mut S,
    connect_timeout: Duration,
) -> Result<u16, std::io::Error> {
    let mut buf = [0u8; 4096];
    let mut len = 0usize;
    loop {
        if len >= buf.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "proxy CONNECT response too large",
            ));
        }
        let n = tokio::time::timeout(connect_timeout, stream.read(&mut buf[len..]))
            .await
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "read timeout"))??;
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "proxy closed connection",
            ));
        }
        len += n;
        if buf[..len].windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    let head = String::from_utf8_lossy(&buf[..len]);
    let mut parts = head.split_whitespace();
    let _version = parts.next();
    let code = parts
        .next()
        .and_then(|c| c.parse::<u16>().ok())
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "bad status line"))?;
    Ok(code)
}

fn authority(scheme: &str, host: &str, port: u16) -> String {
    let default_port = if scheme == "https" { 443 } else { 80 };
    if port == default_port {
        host.to_string()
    } else {
        format!("{host}:{port}")
    }
}

/// 一次上游调用。
///
/// `headers` 是**已最终确定的唯一表**：不得含框架头同名项（`Host`/
/// `Content-Type`/`accept`/`Content-Length`）。这些保留名在组装层被剥离
/// （见 `proxy::merge_custom_headers`/allowlist 选择），由发送端最后唯一写入，
/// 因此不会出现重复值（`Builder::header` 是追加语义，重复保证在组装层）。
pub struct UpstreamCall {
    pub url: String,
    pub headers: Vec<(hyper::header::HeaderName, hyper::header::HeaderValue)>,
    pub body: Bytes,
}

/// 上游响应。
pub struct UpstreamReply {
    pub status: StatusCode,
    pub body: PooledBody,
    /// 本次请求网络阶段起点（wall-clock 毫秒时间戳）：新建连接为 TCP 建连开始
    /// 时刻，复用连接为请求发出时刻。作为 TTFT 与新 tps 分母的计时起点。
    pub start_at_ms: i64,
}

/// 发起上游调用：优先复用池内连接，未命中才独立建连（计时）→ HTTP/1.1 请求 →
/// 等待响应头。响应体由调用方读取（`read_body` 或逐帧流式），读完自动归还连接。
/// 复用连接若已陈旧（对端关闭），发送失败后丢弃并新建连接重试一次。
pub async fn call(
    call: UpstreamCall,
    pool: &UpstreamPool,
    proxy: Option<&str>,
) -> Result<UpstreamReply, UpstreamError> {
    let (scheme, host, port, path_query) = parse_url(&call.url)?;
    // 代理模式下连接池按代理地址隔离（不同代理/直连不混池）。
    let key = match proxy {
        Some(addr) => format!("{addr}|{}://{}:{}", scheme, host, port),
        None => format!("{}://{}:{}", scheme, host, port),
    };

    let timeouts = pool.timeouts();
    let sender = pool.checkout(&key);
    // 是否复用池内连接：仅复用连接可重试一次（见下方重试注释）。显式布尔而非
    // 「建连计时 == 0」——后者在回环/本地上游（建连总耗时整毫秒截断为 0）会把
    // 新连接首发失败也误判成可重试，造成重复请求（LLM 请求有重复计费风险）。
    let was_reused = sender.is_some();
    let (mut start_at_ms, mut sender) = match sender {
        Some(sender) => (now_ms(), Some(sender)),
        None => {
            let (stream, _measured, connect_start) =
                connect_stream(&scheme, &host, port, proxy, timeouts).await?;
            let (send, conn) = http1::handshake(TokioIo::new(stream))
                .await
                .map_err(|e| UpstreamError::Request(format!("HTTP 握手失败：{e}")))?;
            tokio::spawn(async move {
                if let Err(e) = conn.await {
                    tracing::debug!("upstream connection closed: {e}");
                }
            });
            (connect_start, Some(send))
        }
    };

    let mut attempt = 0;
    loop {
        let mut send = sender.take().expect("sender present");
        let reply = send_upstream_request(
            &mut send,
            &path_query,
            &call,
            &authority(&scheme, &host, port),
            timeouts.header,
        )
        .await;
        match reply {
            Ok((status, body)) => {
                let body = PooledBody::new(body, key.clone(), send, pool.clone());
                return Ok(UpstreamReply {
                    status,
                    body,
                    start_at_ms,
                });
            }
            // 仅复用连接重试一次：连接可能已被对端静默关闭，重发是正确自愈。
            // 注意这是内在权衡——上游「已受理请求、响应前断连」时无法区分于
            // 「发送前断」，重发会让上游可能已开始的生成计费两次（hyper 错误
            // 类型不区分发送阶段，本层无法更细）。新建连接的首发失败不重试。
            Err(UpstreamError::Request(_)) if attempt == 0 && was_reused => {
                attempt += 1;
                let (stream, _measured, connect_start) =
                    connect_stream(&scheme, &host, port, proxy, timeouts).await?;
                start_at_ms = connect_start;
                let (send, conn) = http1::handshake(TokioIo::new(stream))
                    .await
                    .map_err(|e| UpstreamError::Request(format!("HTTP 握手失败：{e}")))?;
                tokio::spawn(async move {
                    if let Err(e) = conn.await {
                        tracing::debug!("upstream connection closed: {e}");
                    }
                });
                sender = Some(send);
            }
            Err(e) => return Err(e),
        }
    }
}

async fn send_upstream_request(
    sender: &mut http1::SendRequest<Full<Bytes>>,
    path_query: &str,
    call: &UpstreamCall,
    authority: &str,
    header_timeout: Duration,
) -> Result<(StatusCode, Incoming), UpstreamError> {
    let mut builder = Builder::new().method(Method::POST).uri(path_query);
    for (name, value) in &call.headers {
        builder = builder.header(name, value);
    }
    // 框架头（第 1 层，优先级最高）由发送端唯一写入。组装层保证
    // `call.headers` 不含这四个保留名，故此处不会产生重复值。
    builder = builder
        .header(HOST, authority)
        .header(CONTENT_TYPE, "application/json")
        .header("accept", "application/json, text/event-stream")
        .header(CONTENT_LENGTH, call.body.len());
    let request = builder
        .body(Full::new(call.body.clone()))
        .map_err(|e| UpstreamError::Request(format!("构造上游请求失败：{e}")))?;

    let reply = tokio::time::timeout(header_timeout, sender.send_request(request))
        .await
        .map_err(|_| UpstreamError::Timeout)?
        .map_err(|e| UpstreamError::Request(format!("发送上游请求失败：{e}")))?;
    let (parts, body) = reply.into_parts();
    Ok((parts.status, body))
}

/// 读取整个响应体（非流式路径）。读完连接自动归还池。
/// 超时取自响应体携带的池配置（生产即 `NON_STREAM_BODY_TIMEOUT`）。
pub async fn read_body(body: PooledBody) -> Result<Bytes, UpstreamError> {
    let timeout = body.body_timeout();
    let collected = tokio::time::timeout(timeout, body.collect())
        .await
        .map_err(|_| UpstreamError::Timeout)?
        .map_err(|e| UpstreamError::Request(format!("读取上游响应失败：{e}")))?;
    Ok(collected.to_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 默认端口推断：https→443、http→80（显式端口优先）。
    #[test]
    fn parse_url_infers_default_ports() {
        let (scheme, host, port, path) = parse_url("https://api.example.com/v1/chat").unwrap();
        assert_eq!(
            (scheme.as_str(), host.as_str(), port),
            ("https", "api.example.com", 443)
        );
        assert_eq!(path, "/v1/chat");

        let (scheme, host, port, path) = parse_url("http://api.example.com").unwrap();
        assert_eq!(
            (scheme.as_str(), host.as_str(), port),
            ("http", "api.example.com", 80)
        );
        assert_eq!(path, "/", "空 path 归一为 /");

        let (_, _, port, _) = parse_url("https://api.example.com:8443/x").unwrap();
        assert_eq!(port, 8443, "显式端口优先于默认推断");
    }

    /// 空 path 与 query：query 保留、空 path 补 `/`。
    #[test]
    fn parse_url_keeps_query_and_normalizes_empty_path() {
        let (_, _, _, path) = parse_url("https://h.example?x=1&y=2").unwrap();
        assert_eq!(path, "/?x=1&y=2");
        let (_, _, _, path) = parse_url("https://h.example/v1/messages?a=b").unwrap();
        assert_eq!(path, "/v1/messages?a=b");
    }

    /// 缺主机名报错（错误文案指向 URL 本身，不落到 DNS）。
    #[test]
    fn parse_url_rejects_missing_host() {
        // 相对路径可被 Uri 解析，但无 authority/host。
        let err = parse_url("/v1/chat").unwrap_err();
        assert!(
            err.fail_reason().contains("缺少主机名"),
            "{}",
            err.fail_reason()
        );
        // authority 为空的绝对 URL 由 Uri 解析阶段直接拒绝。
        let err = parse_url("https:///v1").unwrap_err();
        assert!(
            err.fail_reason().contains("URL 无效"),
            "{}",
            err.fail_reason()
        );
    }

    /// IPv6 字面量显式拒绝（04-02）：报错说明不支持，而非误导性的 DNS/TLS 失败。
    #[test]
    fn parse_url_rejects_ipv6_literal_with_clear_reason() {
        let err = parse_url("http://[::1]:8080/v1").unwrap_err();
        let reason = err.fail_reason();
        assert!(reason.contains("IPv6"), "{reason}");
        assert!(!reason.contains("DNS"), "不得报成 DNS 失败：{reason}");
    }

    /// 四组超时默认值即常量（生产口径不变）。
    #[test]
    fn timeouts_default_matches_constants() {
        let t = Timeouts::default();
        assert_eq!(t.connect, CONNECT_TIMEOUT);
        assert_eq!(t.tls_handshake, TLS_HANDSHAKE_TIMEOUT);
        assert_eq!(t.header, HEADER_TIMEOUT);
        assert_eq!(t.non_stream_body, NON_STREAM_BODY_TIMEOUT);
    }
}

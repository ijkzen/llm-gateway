//! 上游 HTTP 客户端。
//!
//! 连接按 `scheme://host:port` 池化复用：首次请求独立建连（精确测量 TCP 建连
//! 与 TLS 握手耗时），响应体读完连接归还池，后续请求直接复用；连接空闲超过
//! 10 分钟被释放。仅支持 HTTP/1.1 上游。
//!
//! 指标语义：`UpstreamReply::start_at_ms` 是本次请求的网络阶段起点（新建连接
//! = TCP 建连开始时刻，复用连接 = 请求发出时刻），作为 TTFT 与新 tps 的计时
//! 起点；`connect_done_at_ms` 保留为复用连接时 TTFT 起点的近似（旧连接建连完成
//! 时刻）。

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

fn tls_config() -> &'static tokio_rustls::rustls::ClientConfig {
    static CONFIG: std::sync::OnceLock<tokio_rustls::rustls::ClientConfig> =
        std::sync::OnceLock::new();
    CONFIG.get_or_init(|| {
        let mut roots = tokio_rustls::rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        tokio_rustls::rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth()
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
) -> Result<(TimedStream, ConnectTiming, i64), UpstreamError> {
    let connect_start_at_ms = now_ms();

    // 代理模式：连接代理服务器 → CONNECT host:port 建立隧道 → https 时隧道内 TLS。
    if let Some(proxy_addr) = proxy {
        return connect_via_proxy(proxy_addr, scheme, host, port, connect_start_at_ms).await;
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
        let stream = match tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr)).await {
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
        let connector = tokio_rustls::TlsConnector::from(std::sync::Arc::new(tls_config().clone()));
        let tls = match tokio::time::timeout(
            TLS_HANDSHAKE_TIMEOUT,
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
) -> Result<(TimedStream, ConnectTiming, i64), UpstreamError> {
    // 解析代理地址（http://host:port）。
    let proxy_url = proxy_addr.trim().strip_prefix("http://").ok_or_else(|| {
        UpstreamError::Connect(format!("代理地址需以 http:// 开头（{proxy_addr}）"))
    })?;
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
        match tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr)).await {
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
    tokio::time::timeout(CONNECT_TIMEOUT, stream.write_all(connect_req.as_bytes()))
        .await
        .map_err(|_| UpstreamError::Connect("代理 CONNECT 写入超时".to_string()))?
        .map_err(|e| UpstreamError::Connect(format!("代理 CONNECT 写入失败：{e}")))?;
    stream.flush().await.ok();

    // 读 CONNECT 响应头（直到 \r\n\r\n），校验 200。
    let status = read_proxy_connect_response(&mut stream)
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
    let connector = tokio_rustls::TlsConnector::from(std::sync::Arc::new(tls_config().clone()));
    let tls = match tokio::time::timeout(
        TLS_HANDSHAKE_TIMEOUT,
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
        let n = tokio::time::timeout(CONNECT_TIMEOUT, stream.read(&mut buf[len..]))
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
pub struct UpstreamCall {
    pub url: String,
    pub headers: Vec<(hyper::header::HeaderName, hyper::header::HeaderValue)>,
    pub body: Bytes,
    /// 是否为流式请求（响应体不设总超时）。
    pub stream: bool,
}

/// 上游响应。
pub struct UpstreamReply {
    pub status: StatusCode,
    pub body: PooledBody,
    /// 本次请求网络阶段起点（wall-clock 毫秒时间戳）：新建连接为 TCP 建连开始
    /// 时刻，复用连接为请求发出时刻。作为 TTFT 与新 tps 分母的计时起点。
    pub start_at_ms: i64,
    /// 建连完成（或复用连接最初建连完成）时刻。复用连接时作为 TTFT 起点的
    /// 近似（比建连开始晚建连时长）。
    pub connect_done_at_ms: i64,
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

    let mut timing = ConnectTiming::default();
    let mut sender = pool.checkout(&key);
    let (mut start_at_ms, mut connect_done_at_ms) = if sender.is_none() {
        let (stream, measured, connect_start) = connect_stream(&scheme, &host, port, proxy).await?;
        timing = measured;
        let (send, conn) = http1::handshake(TokioIo::new(stream))
            .await
            .map_err(|e| UpstreamError::Request(format!("HTTP 握手失败：{e}")))?;
        tokio::spawn(async move {
            if let Err(e) = conn.await {
                tracing::debug!("upstream connection closed: {e}");
            }
        });
        sender = Some(send);
        (connect_start, now_ms())
    } else {
        // 复用连接：起点=请求发出时刻；TTFT 起点近似=连接最初建连完成时刻。
        let now = now_ms();
        (now, now)
    };

    let mut attempt = 0;
    loop {
        let mut send = sender.take().expect("sender present");
        let reply = send_upstream_request(
            &mut send,
            &path_query,
            &call,
            &authority(&scheme, &host, port),
        )
        .await;
        match reply {
            Ok((status, body)) => {
                let body = PooledBody::new(body, key.clone(), send, pool.clone());
                return Ok(UpstreamReply {
                    status,
                    body,
                    start_at_ms,
                    connect_done_at_ms,
                });
            }
            Err(UpstreamError::Request(_)) if attempt == 0 && timing.total_ms() == 0 => {
                // 复用连接可能已被对端静默关闭：丢弃并新建连接重试一次。
                attempt += 1;
                let (stream, measured, connect_start) =
                    connect_stream(&scheme, &host, port, proxy).await?;
                timing = measured;
                start_at_ms = connect_start;
                connect_done_at_ms = now_ms();
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
) -> Result<(StatusCode, Incoming), UpstreamError> {
    let mut builder = Builder::new()
        .method(Method::POST)
        .uri(path_query)
        .header(HOST, authority)
        .header(CONTENT_TYPE, "application/json")
        .header("accept", "application/json, text/event-stream")
        .header(CONTENT_LENGTH, call.body.len());
    for (name, value) in &call.headers {
        builder = builder.header(name, value);
    }
    let request = builder
        .body(Full::new(call.body.clone()))
        .map_err(|e| UpstreamError::Request(format!("构造上游请求失败：{e}")))?;

    let reply = tokio::time::timeout(HEADER_TIMEOUT, sender.send_request(request))
        .await
        .map_err(|_| UpstreamError::Timeout)?
        .map_err(|e| UpstreamError::Request(format!("发送上游请求失败：{e}")))?;
    let (parts, body) = reply.into_parts();
    Ok((parts.status, body))
}

/// 读取整个响应体（非流式路径）。读完连接自动归还池。
pub async fn read_body(body: PooledBody) -> Result<Bytes, UpstreamError> {
    let collected = tokio::time::timeout(NON_STREAM_BODY_TIMEOUT, body.collect())
        .await
        .map_err(|_| UpstreamError::Timeout)?
        .map_err(|e| UpstreamError::Request(format!("读取上游响应失败：{e}")))?;
    Ok(collected.to_bytes())
}

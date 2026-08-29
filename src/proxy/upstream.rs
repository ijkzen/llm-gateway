//! 上游 HTTP 客户端。
//!
//! 每次请求独立建连（不做连接池复用），精确测量 TCP 建连与 TLS 握手耗时，
//! 用于 request 表的 `network_latency` 指标。仅支持 HTTP/1.1 上游。

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
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;

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
    let port = uri.port_u16().unwrap_or(if scheme == "https" { 443 } else { 80 });
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
async fn connect_stream(
    scheme: &str,
    host: &str,
    port: u16,
) -> Result<(TimedStream, ConnectTiming), UpstreamError> {
    let addrs: Vec<std::net::SocketAddr> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| UpstreamError::Connect(format!("DNS 解析失败（{host}）：{e}")))?
        .collect();
    if addrs.is_empty() {
        return Err(UpstreamError::Connect(format!("DNS 解析不到可用地址（{host}）")));
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
            return Ok((TimedStream::Plain(stream), ConnectTiming { tcp_ms, tls_ms: 0 }));
        }

        let tls_started = Instant::now();
        let server_name =
            tokio_rustls::rustls::pki_types::ServerName::try_from(host.to_string())
                .map_err(|e| UpstreamError::Connect(format!("TLS 主机名无效（{host}）：{e}")))?;
        let connector = tokio_rustls::TlsConnector::from(std::sync::Arc::new(tls_config().clone()));
        let tls = match tokio::time::timeout(TLS_HANDSHAKE_TIMEOUT, connector.connect(server_name, stream))
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
        ));
    }
    Err(last_err.unwrap_or_else(|| UpstreamError::Connect("连接失败".to_string())))
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
    pub body: Incoming,
    pub connect_ms: u64,
}

/// 发起上游调用：独立建连（计时）→ HTTP/1.1 请求 → 等待响应头。
/// 响应体由调用方读取（`read_body` 或逐帧流式）。
pub async fn call(call: UpstreamCall) -> Result<UpstreamReply, UpstreamError> {
    let (scheme, host, port, path_query) = parse_url(&call.url)?;
    let (stream, timing) = connect_stream(&scheme, &host, port).await?;

    let (mut sender, conn) = http1::handshake(TokioIo::new(stream))
        .await
        .map_err(|e| UpstreamError::Request(format!("HTTP 握手失败：{e}")))?;
    tokio::spawn(async move {
        if let Err(e) = conn.await {
            tracing::debug!("upstream connection closed: {e}");
        }
    });

    let mut builder = Builder::new()
        .method(Method::POST)
        .uri(path_query)
        .header(HOST, authority(&scheme, &host, port))
        .header(CONTENT_TYPE, "application/json")
        .header("accept", "application/json, text/event-stream")
        .header(CONTENT_LENGTH, call.body.len());
    for (name, value) in call.headers {
        builder = builder.header(name, value);
    }
    let request = builder
        .body(Full::new(call.body))
        .map_err(|e| UpstreamError::Request(format!("构造上游请求失败：{e}")))?;

    let reply = tokio::time::timeout(HEADER_TIMEOUT, sender.send_request(request))
        .await
        .map_err(|_| UpstreamError::Timeout)?
        .map_err(|e| UpstreamError::Request(format!("发送上游请求失败：{e}")))?;

    let (parts, body) = reply.into_parts();
    Ok(UpstreamReply {
        status: parts.status,
        body,
        connect_ms: timing.total_ms(),
    })
}

/// 读取整个响应体（非流式路径）。
pub async fn read_body(body: Incoming) -> Result<Bytes, UpstreamError> {
    let collected = tokio::time::timeout(NON_STREAM_BODY_TIMEOUT, body.collect())
        .await
        .map_err(|_| UpstreamError::Timeout)?
        .map_err(|e| UpstreamError::Request(format!("读取上游响应失败：{e}")))?;
    Ok(collected.to_bytes())
}

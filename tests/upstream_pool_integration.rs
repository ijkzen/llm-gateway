//! 上游连接池集成测试：连接复用、空闲超时释放、`Connection: close` 后不归还。

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use http_body_util::Full;
use hyper::body::Bytes;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use llm_gateway::proxy::pool::UpstreamPool;
use llm_gateway::proxy::upstream::{UpstreamCall, call, read_body};

/// 返回 `{"ok":true}`，可配置 `Connection: close`。
async fn mock_handler(
    close: bool,
    req: Request<hyper::body::Incoming>,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let _ = req;
    let body = Bytes::from_static(b"{\"ok\":true}");
    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .header("content-length", body.len());
    if close {
        builder = builder.header("connection", "close");
    }
    Ok(builder.body(Full::new(body)).expect("valid response"))
}

/// 手动 accept 循环的 mock 上游：计数接受的连接数，逐连接 `serve_connection`（支持 keep-alive）。
async fn spawn_mock(count: Arc<AtomicUsize>, close: bool) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(accepted) => accepted,
                Err(_) => break,
            };
            count.fetch_add(1, Ordering::SeqCst);
            let io = TokioIo::new(stream);
            let close_flag = close;
            tokio::spawn(async move {
                let service = service_fn(move |req| mock_handler(close_flag, req));
                let _ = http1::Builder::new().serve_connection(io, service).await;
            });
        }
    });
    format!("http://{}", addr)
}

async fn call_json(url: &str, pool: &UpstreamPool) -> Bytes {
    let request = UpstreamCall {
        url: url.to_string(),
        headers: vec![],
        body: Bytes::from_static(b"{}"),
        stream: false,
    };
    let reply = call(request, pool, None).await.expect("call ok");
    assert_eq!(reply.status, StatusCode::OK);
    read_body(reply.body).await.expect("read body")
}

/// 同一上游串行两次请求：第二次复用连接，连接数保持 1。
#[tokio::test]
async fn reuses_connection_for_second_request() {
    let connections = Arc::new(AtomicUsize::new(0));
    let url = spawn_mock(Arc::clone(&connections), false).await;
    let pool = UpstreamPool::new(Duration::from_secs(600));

    let body = call_json(&url, &pool).await;
    assert_eq!(String::from_utf8_lossy(&body), "{\"ok\":true}");
    assert_eq!(
        connections.load(Ordering::SeqCst),
        1,
        "首次请求应新建 1 条连接"
    );

    let body = call_json(&url, &pool).await;
    assert_eq!(String::from_utf8_lossy(&body), "{\"ok\":true}");
    assert_eq!(
        connections.load(Ordering::SeqCst),
        1,
        "第二次请求应复用池内连接，不新建"
    );
}

/// 空闲超过超时后连接被释放：短超时（1s）下第二次请求需新建连接。
#[tokio::test]
async fn releases_idle_connection_after_timeout() {
    let connections = Arc::new(AtomicUsize::new(0));
    let url = spawn_mock(Arc::clone(&connections), false).await;
    let pool = UpstreamPool::new(Duration::from_secs(1));

    let body = call_json(&url, &pool).await;
    assert_eq!(String::from_utf8_lossy(&body), "{\"ok\":true}");
    assert_eq!(connections.load(Ordering::SeqCst), 1);

    // 等待超过空闲超时（1s 超时 + 后台扫描粒度，多等一会确保惰性过期生效）。
    tokio::time::sleep(Duration::from_millis(1600)).await;

    let body = call_json(&url, &pool).await;
    assert_eq!(String::from_utf8_lossy(&body), "{\"ok\":true}");
    assert_eq!(
        connections.load(Ordering::SeqCst),
        2,
        "空闲超时后旧连接应被释放，第二次请求新建连接"
    );
}

/// 上游返回 `Connection: close`：连接关闭不归还池，下次请求新建连接。
#[tokio::test]
async fn discards_closed_connection() {
    let connections = Arc::new(AtomicUsize::new(0));
    let url = spawn_mock(Arc::clone(&connections), true).await;
    let pool = UpstreamPool::new(Duration::from_secs(600));

    let body = call_json(&url, &pool).await;
    assert_eq!(String::from_utf8_lossy(&body), "{\"ok\":true}");
    assert_eq!(connections.load(Ordering::SeqCst), 1);

    let body = call_json(&url, &pool).await;
    assert_eq!(String::from_utf8_lossy(&body), "{\"ok\":true}");
    assert_eq!(
        connections.load(Ordering::SeqCst),
        2,
        "Connection: close 的连接不应复用，需新建"
    );
}

// ─── 网络代理（HTTP CONNECT）端到端测试 ───────────────────────────────────────
// mock 一个「代理服务器」：收到 `CONNECT host:port` 后，主动连目标 mock 上游，
// 双向桥接字节流，返回 `HTTP/1.1 200 Connection established`。验证
// `call(proxy=Some(addr))` 经 CONNECT 隧道转发成功，且连接池按代理地址隔离。

/// CONNECT 代理服务器：解析 CONNECT 行 → 连目标 → 桥接 → 回 200。
/// 返回 (代理地址, CONNECT 请求计数)。
async fn spawn_connect_proxy() -> (String, Arc<AtomicUsize>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let connect_count = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&connect_count);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind proxy");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        loop {
            let (mut client, _) = match listener.accept().await {
                Ok(accepted) => accepted,
                Err(_) => break,
            };
            count.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                // 读 CONNECT 请求头（到 \r\n\r\n）。
                let mut buf = [0u8; 4096];
                let mut len = 0usize;
                loop {
                    let Ok(n) = client.read(&mut buf[len..]).await else {
                        return;
                    };
                    if n == 0 {
                        return;
                    }
                    len += n;
                    if buf[..len].windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                let head = String::from_utf8_lossy(&buf[..len]);
                // 解析 CONNECT 目标（host:port）。
                let Some(first_line) = head.lines().next() else {
                    return;
                };
                let Some(rest) = first_line.strip_prefix("CONNECT ") else {
                    return;
                };
                let Some(target) = rest.split_whitespace().next() else {
                    return;
                };
                // 连目标 mock 上游。
                let Ok(mut target_stream) = tokio::net::TcpStream::connect(target).await else {
                    return;
                };
                // 回 200。
                let _ = client
                    .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                    .await;
                // 双向桥接。
                let (mut cr, mut cw) = client.split();
                let (mut tr, mut tw) = target_stream.split();
                let c2t = tokio::io::copy(&mut cr, &mut tw);
                let t2c = tokio::io::copy(&mut tr, &mut cw);
                let _ = tokio::join!(c2t, t2c);
            });
        }
    });
    (format!("http://{addr}"), connect_count)
}

/// 经 HTTP CONNECT 代理转发：上游 http:// 目标（隧道内无 TLS）。
#[tokio::test]
async fn forwards_via_connect_proxy() {
    let connections = Arc::new(AtomicUsize::new(0));
    let upstream = spawn_mock(Arc::clone(&connections), false).await;
    let (proxy, connect_count) = spawn_connect_proxy().await;
    let pool = UpstreamPool::new(Duration::from_secs(600));

    let request = UpstreamCall {
        url: upstream.clone(),
        headers: vec![],
        body: Bytes::from_static(b"{}"),
        stream: false,
    };
    let reply = call(request, &pool, Some(proxy.as_str()))
        .await
        .expect("proxy call ok");
    assert_eq!(reply.status, StatusCode::OK);
    let body = read_body(reply.body).await.expect("read body");
    assert_eq!(String::from_utf8_lossy(&body), "{\"ok\":true}");
    assert_eq!(
        connect_count.load(Ordering::SeqCst),
        1,
        "应经代理 CONNECT 一次"
    );
}

/// 连接池按代理地址隔离：直连与代理不混池。
#[tokio::test]
async fn proxy_and_direct_pools_are_isolated() {
    let connections = Arc::new(AtomicUsize::new(0));
    let upstream = spawn_mock(Arc::clone(&connections), false).await;
    let (proxy, _) = spawn_connect_proxy().await;
    let pool = UpstreamPool::new(Duration::from_secs(600));

    // 直连一次 + 代理一次（不同 key），各自建连。
    let direct_req = UpstreamCall {
        url: upstream.clone(),
        headers: vec![],
        body: Bytes::from_static(b"{}"),
        stream: false,
    };
    let reply = call(direct_req, &pool, None).await.expect("direct ok");
    assert_eq!(reply.status, StatusCode::OK);
    read_body(reply.body).await.expect("read body");

    let proxy_req = UpstreamCall {
        url: upstream.clone(),
        headers: vec![],
        body: Bytes::from_static(b"{}"),
        stream: false,
    };
    let reply = call(proxy_req, &pool, Some(proxy.as_str()))
        .await
        .expect("proxy ok");
    assert_eq!(reply.status, StatusCode::OK);
    read_body(reply.body).await.expect("read body");

    // 上游收到 2 个连接（直连 1 + 代理隧道 1），不混池。
    assert_eq!(connections.load(Ordering::SeqCst), 2);
}

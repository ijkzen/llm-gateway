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

/// 取出 Err 而不要求 Ok 侧 Debug（UpstreamReply 未实现 Debug）。
fn expect_err<T>(
    result: Result<T, llm_gateway::proxy::upstream::UpstreamError>,
) -> llm_gateway::proxy::upstream::UpstreamError {
    match result {
        Ok(_) => panic!("expected error, got ok"),
        Err(e) => e,
    }
}

async fn call_json(url: &str, pool: &UpstreamPool) -> Bytes {
    let request = UpstreamCall {
        url: url.to_string(),
        headers: vec![],
        body: Bytes::from_static(b"{}"),
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
    };
    let reply = call(direct_req, &pool, None).await.expect("direct ok");
    assert_eq!(reply.status, StatusCode::OK);
    read_body(reply.body).await.expect("read body");

    let proxy_req = UpstreamCall {
        url: upstream.clone(),
        headers: vec![],
        body: Bytes::from_static(b"{}"),
    };
    let reply = call(proxy_req, &pool, Some(proxy.as_str()))
        .await
        .expect("proxy ok");
    assert_eq!(reply.status, StatusCode::OK);
    read_body(reply.body).await.expect("read body");

    // 上游收到 2 个连接（直连 1 + 代理隧道 1），不混池。
    assert_eq!(connections.load(Ordering::SeqCst), 2);
}

// ─── 04-07 陈旧连接重试 / 04-08 超时注入 / 04-09 失败族 / 04-10 并发隔离 ───

/// 服务端在响应后主动静默 close（不发 Connection: close）：第二次 call 命中
/// 「复用连接发送失败/已死 → 丢弃并新建重试一次」，应成功且连接计数为 2。
#[tokio::test]
async fn retries_once_on_stale_pooled_connection() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let connections = Arc::new(AtomicUsize::new(0));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let url = format!("http://{}", addr);
    let count = Arc::clone(&connections);
    const RESPONSE: &[u8] = b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 11\r\n\r\n{\"ok\":true}";

    tokio::spawn(async move {
        let mut buf = [0u8; 8192];
        // 第一条连接：回一次响应，短暂等待后被上游静默关闭（模拟空闲回收）。
        let (mut first, _) = listener.accept().await.expect("accept 1");
        count.fetch_add(1, Ordering::SeqCst);
        let _ = first.read(&mut buf).await;
        let _ = first.write_all(RESPONSE).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        drop(first);
        // 第二条连接：重试路径新建，正常服务。
        if let Ok((mut second, _)) = listener.accept().await {
            count.fetch_add(1, Ordering::SeqCst);
            let _ = second.read(&mut buf).await;
            let _ = second.write_all(RESPONSE).await;
        }
    });

    let pool = UpstreamPool::new(Duration::from_secs(600));
    let body = call_json(&url, &pool).await;
    assert_eq!(String::from_utf8_lossy(&body), "{\"ok\":true}");
    assert_eq!(connections.load(Ordering::SeqCst), 1, "首次应新建 1 条");

    // 等上游关闭连接（越过 50ms 静默关闭窗口）。
    tokio::time::sleep(Duration::from_millis(250)).await;
    let body = call_json(&url, &pool).await;
    assert_eq!(String::from_utf8_lossy(&body), "{\"ok\":true}");
    assert_eq!(
        connections.load(Ordering::SeqCst),
        2,
        "陈旧连接应被丢弃并新建一条重试"
    );
}

/// 直连失败族：连接拒绝（close 端口）与无效 URL 的错误映射 + 不落池。
#[tokio::test]
async fn connect_failures_map_to_connect_error() {
    let pool = UpstreamPool::new(Duration::from_secs(600));

    // 连接拒绝：先占一个端口再释放，确保大概率无人监听。
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let request = UpstreamCall {
        url: format!("http://{addr}/v1"),
        headers: vec![],
        body: Bytes::from_static(b"{}"),
    };
    let err = expect_err(call(request, &pool, None).await);
    let reason = err.fail_reason();
    assert!(
        reason.contains("上游连接失败"),
        "连接拒绝应映射为 Connect 族：{reason}"
    );

    // 无效 URL（含 IPv6 字面量）不触网、直接报错。
    let request = UpstreamCall {
        url: "http://[::1]:8080/v1".to_string(),
        headers: vec![],
        body: Bytes::from_static(b"{}"),
    };
    let err = expect_err(call(request, &pool, None).await);
    assert!(err.fail_reason().contains("IPv6"), "{}", err.fail_reason());
}

/// CONNECT 代理防御分支：非 http:// 前缀、带 userinfo、非 200。
#[tokio::test]
async fn proxy_defensive_branches_are_rejected() {
    let connections = Arc::new(AtomicUsize::new(0));
    let upstream = spawn_mock(Arc::clone(&connections), false).await;
    let pool = UpstreamPool::new(Duration::from_secs(600));

    let mk = || UpstreamCall {
        url: upstream.clone(),
        headers: vec![],
        body: Bytes::from_static(b"{}"),
    };

    let err = expect_err(call(mk(), &pool, Some("socks5://127.0.0.1:1080")).await);
    assert!(
        err.fail_reason().contains("http://"),
        "{}",
        err.fail_reason()
    );

    let err = expect_err(call(mk(), &pool, Some("http://user:pass@127.0.0.1:1")).await);
    assert!(
        err.fail_reason().contains("带认证"),
        "{}",
        err.fail_reason()
    );

    // 代理返回非 200：CONNECT 应报状态码而非继续。
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        if let Ok((mut stream, _)) = listener.accept().await {
            let _ = tokio::io::AsyncReadExt::read(&mut stream, &mut [0u8; 256]).await;
            let _ = tokio::io::AsyncWriteExt::write_all(
                &mut stream,
                b"HTTP/1.1 407 Proxy Authentication Required\r\n\r\n",
            )
            .await;
        }
    });
    let err = expect_err(call(mk(), &pool, Some(&format!("http://{addr}"))).await);
    assert!(err.fail_reason().contains("407"), "{}", err.fail_reason());
}

/// 04-08 超时注入：silent upstream（accept 后不响应）在毫秒级头超时下报 Timeout，
/// 而非挂满 120s；超时变体不触发重试（仅一次连接）。
#[tokio::test]
async fn silent_upstream_times_out_with_injected_timeout() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let connections = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&connections);
    tokio::spawn(async move {
        // accept 后保持连接但永不响应（持有 stream 不放）。
        let mut held = Vec::new();
        while let Ok((stream, _)) = listener.accept().await {
            count.fetch_add(1, Ordering::SeqCst);
            held.push(stream);
        }
    });

    let pool = UpstreamPool::with_timeouts(
        Duration::from_secs(600),
        llm_gateway::proxy::upstream::Timeouts {
            header: Duration::from_millis(120),
            ..Default::default()
        },
    );
    let request = UpstreamCall {
        url: format!("http://{addr}/v1"),
        headers: vec![],
        body: Bytes::from_static(b"{}"),
    };
    let err = expect_err(call(request, &pool, None).await);
    assert!(
        matches!(err, llm_gateway::proxy::upstream::UpstreamError::Timeout),
        "silent upstream 应报 Timeout：{}",
        err.fail_reason()
    );
    assert_eq!(
        connections.load(Ordering::SeqCst),
        1,
        "超时不得触发重试（只有一次连接）"
    );
}

/// 04-10 并发：同 key N 并发各建一条连接（单连接从不被双请求共享），
/// 全部归还后第 N+1 次复用不新建。
#[tokio::test]
async fn concurrent_calls_open_independent_connections_then_reuse() {
    let connections = Arc::new(AtomicUsize::new(0));
    let url = spawn_mock(Arc::clone(&connections), false).await;
    let pool = UpstreamPool::new(Duration::from_secs(600));

    const N: usize = 4;
    let mut set = tokio::task::JoinSet::new();
    for _ in 0..N {
        let url = url.clone();
        let pool = pool.clone();
        set.spawn(async move { call_json(&url, &pool).await });
    }
    while let Some(outcome) = set.join_next().await {
        let body = outcome.expect("join");
        assert_eq!(String::from_utf8_lossy(&body), "{\"ok\":true}");
    }
    assert_eq!(
        connections.load(Ordering::SeqCst),
        N,
        "N 并发应各建一条连接（不共享）"
    );

    // 归还后下一次复用：连接数不增。
    let body = call_json(&url, &pool).await;
    assert_eq!(String::from_utf8_lossy(&body), "{\"ok\":true}");
    assert_eq!(
        connections.load(Ordering::SeqCst),
        N,
        "空闲连接应被复用，不新建"
    );
}

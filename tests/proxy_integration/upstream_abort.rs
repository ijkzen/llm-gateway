use super::*;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// 上游 200 后中断的两种行为：
/// - `NonStreamBody`: 按 Content-Length 写一半后 clean FIN —— 网关读体得到
///   IncompleteMessage（E1 场景，读失败不能吞成 200 假成功）。
/// - `StreamAbort`: 200 + chunked 声明后写非法块长 —— hyper 帧错误（E2
///   场景，与连接重置同走 frame Err，上游断流不能记 success）。
#[derive(Clone, Copy)]
enum AbortKind {
    NonStreamBody,
    StreamAbort,
}

/// 先完整读请求（头 + body）再响应：模拟真实上游，避免请求写一半被断。
async fn read_full_request(socket: &mut TcpStream) -> bool {
    let mut buf: Vec<u8> = Vec::new();
    let mut tmp = [0u8; 1024];
    loop {
        let n = socket.read(&mut tmp).await.ok().unwrap_or(0);
        if n == 0 {
            return false;
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..pos]).to_ascii_lowercase();
            let content_length = head
                .lines()
                .find_map(|line| {
                    line.strip_prefix("content-length:")
                        .and_then(|v| v.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            let need = pos + 4 + content_length;
            while buf.len() < need {
                let n = socket.read(&mut tmp).await.ok().unwrap_or(0);
                if n == 0 {
                    return false;
                }
                buf.extend_from_slice(&tmp[..n]);
            }
            return true;
        }
        if buf.len() > 64 * 1024 {
            return false;
        }
    }
}

async fn spawn_abort_upstream(kind: AbortKind) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                if !read_full_request(&mut socket).await {
                    return;
                }
                match kind {
                    AbortKind::NonStreamBody => {
                        // Content-Length 2000 只写 ~20 字节后 FIN：hyper 读体
                        // IncompleteMessage。
                        let _ = socket
                            .write_all(
                                b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 2000\r\n\r\n{\"id\":\"partial\"",
                            )
                            .await;
                        let _ = socket.flush().await;
                    }
                    AbortKind::StreamAbort => {
                        // 声明 chunked 后写非法块长：hyper 解码即帧错误，确定性
                        // 模拟上游流中断（连接重置同走 frame Err 分支）。
                        let _ = socket
                            .write_all(
                                b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\n\r\nZZZ\r\n",
                            )
                            .await;
                        let _ = socket.flush().await;
                    }
                }
            });
        }
    });
    format!("http://{addr}")
}

fn assert_failure_record(rows: &[request::Model], stream: bool, reason_prefix: &str) {
    assert_eq!(rows.len(), 1, "应只有一次尝试的记录");
    let record = &rows[0];
    assert_eq!(record.success, false);
    assert_eq!(record.stream, stream);
    let reason = record.fail_reason.as_deref().unwrap_or("");
    assert!(
        reason.starts_with(reason_prefix),
        "fail_reason 应含 {reason_prefix}，实际: {reason}"
    );
}

// ─── E1：200 后读体失败 → 502 + 失败落库（不能 200 `{}` 假成功） ─────────────

#[tokio::test]
async fn openai_non_stream_body_abort_returns_502_and_failure_record() {
    let base = spawn_abort_upstream(AbortKind::NonStreamBody).await;
    let (app, db) = common_setup_with_member(&base, 0, 0, 0).await;

    let (status, text) = send_chat(&app, chat_body("vm-x", false)).await;
    assert_eq!(status, 502, "{text}");
    assert!(text.contains("读取上游响应失败"), "{text}");

    let rows = wait_for_records(&db, 1).await;
    assert_failure_record(&rows, false, "读取上游响应失败");
}

#[tokio::test]
async fn native_non_stream_body_abort_returns_502_and_failure_record() {
    let base = spawn_abort_upstream(AbortKind::NonStreamBody).await;
    let (app, db) = common_setup_native(&base, 2, 2, "vm-native").await;

    let (status, text, _) =
        send_native(&app, "/v1/messages", messages_body("vm-native", false), &[]).await;
    assert_eq!(status, 502, "{text}");
    assert!(text.contains("读取上游响应失败"), "{text}");

    let rows = wait_for_records(&db, 1).await;
    assert_failure_record(&rows, false, "读取上游响应失败");
}

// ─── E2：流式中途帧错误 → 失败落库（不能 success:true） ─────────────────────

#[tokio::test]
async fn openai_stream_reset_records_failure_with_error_frame() {
    let base = spawn_abort_upstream(AbortKind::StreamAbort).await;
    let (app, db) = common_setup_with_member(&base, 0, 0, 0).await;

    let (status, text) = send_chat(&app, chat_body("vm-x", true)).await;
    assert_eq!(status, 200, "{text}");
    // OpenAI Compat 直通：补发 error 帧 + [DONE] 收尾，客户端不会裸截断。
    assert!(text.contains("upstream_error"), "{text}");
    assert!(text.contains("data: [DONE]"), "{text}");

    let rows = wait_for_records(&db, 1).await;
    assert_failure_record(&rows, true, "读取上游流失败");
}

#[tokio::test]
async fn converted_stream_reset_records_failure_with_error_frame() {
    let base = spawn_abort_upstream(AbortKind::StreamAbort).await;
    let (app, db) = common_setup_with_member(&base, 2, 0, 0).await;

    let (status, text) = send_chat(&app, chat_body("vm-x", true)).await;
    assert_eq!(status, 200, "{text}");
    assert!(text.contains("upstream_error"), "{text}");
    assert!(text.contains("data: [DONE]"), "{text}");

    let rows = wait_for_records(&db, 1).await;
    assert_failure_record(&rows, true, "读取上游流失败");
}

#[tokio::test]
async fn native_stream_reset_records_failure() {
    let base = spawn_abort_upstream(AbortKind::StreamAbort).await;
    let (app, db) = common_setup_native(&base, 2, 2, "vm-native").await;

    // 原生透传不重帧：客户端体以错误中止，读体按失败容忍。
    let (status, _) = {
        let request = Request::builder()
            .method("POST")
            .uri("/v1/messages")
            .header("content-type", "application/json")
            .header("authorization", TEST_BEARER)
            .body(Body::from(messages_body("vm-native", true).to_string()))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        let status = response.status().as_u16();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap_or_default();
        (status, String::from_utf8_lossy(&bytes).to_string())
    };
    assert_eq!(status, 200, "SSE 响应头已发出后中断仍应 200");

    let rows = wait_for_records(&db, 1).await;
    assert_failure_record(&rows, true, "读取上游流失败");
}

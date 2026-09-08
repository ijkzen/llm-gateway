use super::*;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// M1 回归：Responses 转换流式必须是 live 逐事件转发——上游把流分成两段并
/// 在中间无限阻塞，客户端应能先收到首段转换帧；若仍走「整条缓冲后回放」，
/// 首帧要等上游收流完毕才会出现（永远等不到，读帧超时即失败）。
#[tokio::test]
async fn responses_converted_stream_forwards_chunks_before_upstream_finishes() {
    // 事件 1 发出后阻塞，直到测试确认收到首帧才放行事件 2。
    let (gate_tx, gate_rx) = tokio::sync::oneshot::channel::<()>();
    let (sent_tx, sent_rx) = tokio::sync::oneshot::channel::<()>();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let Ok((mut socket, _)) = listener.accept().await else {
            return;
        };

        // 读完整请求（头 + body）再响应，模拟真实上游。
        let mut buf: Vec<u8> = Vec::new();
        let mut tmp = [0u8; 1024];
        loop {
            let n = socket.read(&mut tmp).await.ok().unwrap_or(0);
            if n == 0 {
                return;
            }
            buf.extend_from_slice(&tmp[..n]);
            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&buf[..pos]).to_ascii_lowercase();
                let len = head
                    .lines()
                    .find_map(|line| {
                        line.strip_prefix("content-length:")
                            .and_then(|v| v.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                let need = pos + 4 + len;
                while buf.len() < need {
                    let n = socket.read(&mut tmp).await.ok().unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&tmp[..n]);
                }
                break;
            }
            if buf.len() > 64 * 1024 {
                return;
            }
        }

        async fn write_chunked(
            socket: &mut tokio::net::TcpStream,
            data: &[u8],
        ) -> std::io::Result<()> {
            let head = format!("{:x}\r\n", data.len());
            socket.write_all(head.as_bytes()).await?;
            socket.write_all(data).await?;
            socket.write_all(b"\r\n").await?;
            socket.flush().await
        }

        let _ = socket
            .write_all(
                b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\n\r\n",
            )
            .await;
        let _ = socket.flush().await;
        // 第一段：created + 一个内容增量（转换后应有内容帧）。
        let first = concat!(
            "data: ",
            r#"{"type":"response.created","response":{"id":"resp_live","model":"gpt-x"}}"#,
            "\n\n",
            "data: ",
            r#"{"type":"response.output_text.delta","delta":"你好"}"#,
            "\n\n",
        );
        let _ = write_chunked(&mut socket, first.as_bytes()).await;
        let _ = sent_tx.send(());
        // 阻塞到测试确认首帧已到，才发收尾事件。
        let _ = gate_rx.await;
        let second = concat!(
            "data: ",
            r#"{"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":5,"output_tokens":2}}}"#,
            "\n\n",
        );
        let _ = write_chunked(&mut socket, second.as_bytes()).await;
        let _ = socket.write_all(b"0\r\n\r\n").await;
        let _ = socket.flush().await;
    });
    let base = format!("http://{addr}");

    let (app, db) = common_setup_with_member(&base, 1, 0, 0).await;
    let request = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .header("authorization", TEST_BEARER)
        .body(Body::from(chat_body("vm-x", true).to_string()))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status().as_u16(), 200);

    let mut body = response.into_body();
    let _ = sent_rx.await;
    // 上游仍阻塞在第二段；客户端必须已能收到转换帧（live 判定）。
    // 首帧是 role 起始块，持续读到内容增量帧为止。
    let mut first_content: Option<String> = None;
    for _ in 0..50 {
        let frame = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            next_data_frame(&mut body),
        )
        .await
        .expect("帧应在上游收流完成前持续到达（live 逐事件转发）")
        .expect("读帧失败")
        .expect("上游已提前收流（EOF 无帧）");
        let text = String::from_utf8_lossy(&frame).to_string();
        if text.contains("你好") || text.contains("\"content\"") {
            first_content = Some(text);
            break;
        }
    }
    assert!(first_content.is_some(), "内容增量帧应在上游收流前到达");

    // 放行上游第二段，收完剩余帧。
    let _ = gate_tx.send(());
    let mut rest = String::new();
    while let Ok(Ok(Some(frame))) = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        next_data_frame(&mut body),
    )
    .await
    {
        rest.push_str(&String::from_utf8_lossy(&frame));
    }
    assert!(rest.contains("data: [DONE]"), "流应以 [DONE] 收尾: {rest}");

    let rows = wait_for_records(&db, 1).await;
    assert_eq!(rows[0].success, true);
    assert_eq!(rows[0].stream, true);
}

/// 逐帧读 axum body 的下一个 data 帧（跳过 trailers；EOF 返回 Ok(None)）。
async fn next_data_frame(
    body: &mut axum::body::Body,
) -> Result<Option<axum::body::Bytes>, axum::Error> {
    use http_body_util::BodyExt;
    loop {
        match body.frame().await {
            Some(Ok(frame)) => {
                if let Ok(data) = frame.into_data() {
                    return Ok(Some(data));
                }
                // trailers 忽略，继续读下一帧。
            }
            Some(Err(e)) => return Err(e),
            None => return Ok(None),
        }
    }
}

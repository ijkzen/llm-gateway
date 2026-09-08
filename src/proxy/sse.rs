//! SSE 解析与写出工具。

/// 增量拆分上游字节流为完整 SSE `data:` 载荷。
///
/// 只关心 data 行；事件以空行分隔，同一事件内的多个 data 行按规范以
/// `\n` 连接（OpenAI/Anthropic/Gemini 实践中每事件都是单行 data）。
///
/// 实现按字节下标扫描缓冲：data 行只记录区间、事件边界才拼一次 String，
/// 已消费区在 feed 末尾一次性移除（不做逐行 to_string + drain 头移，
/// 单帧多事件从近似 O(n²) 降为 O(n)，P2）。
#[derive(Debug, Default)]
pub struct SseSplitter {
    buffer: String,
}

impl SseSplitter {
    /// 喂入一段（可能不完整的）UTF-8 文本，返回本段内完整的事件载荷。
    pub fn feed(&mut self, text: &str) -> Vec<String> {
        self.buffer.push_str(text);
        let mut events = Vec::new();
        // data 行区间（相对 buffer），事件边界才拼接，避免逐行分配。
        let mut data_parts: Vec<(usize, usize)> = Vec::new();
        let mut consumed = 0usize;
        while let Some(rel) = self.buffer[consumed..].find('\n') {
            let line_end = consumed + rel;
            let next = line_end + 1;
            let raw_line = &self.buffer[consumed..line_end];
            // data 载荷区间终点（不含行尾 \r，CRLF 语义与逐行 to_string 版一致）。
            let data_end = if raw_line.ends_with('\r') {
                line_end - 1
            } else {
                line_end
            };
            let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
            if line.is_empty() {
                if !data_parts.is_empty() {
                    events.push(join_data_parts(&self.buffer, &data_parts));
                    data_parts.clear();
                }
            } else if let Some(data) = line.strip_prefix("data:") {
                let data = data.strip_prefix(' ').unwrap_or(data);
                if data == "[DONE]" {
                    data_parts.clear();
                    events.push("[DONE]".to_string());
                } else {
                    // data 载荷起点：行内 "data:" 后（可选）去一个空格。
                    let mut start = consumed + "data:".len();
                    if self.buffer.as_bytes().get(start) == Some(&b' ') {
                        start += 1;
                    }
                    data_parts.push((start, data_end));
                }
            }
            // 其他字段（event:/id:/retry:）与注释行忽略。
            consumed = next;
        }
        // 已消费区一次性移除（跨帧残余留在缓冲）。
        if consumed > 0 {
            self.buffer.drain(..consumed);
        }
        events
    }
}

/// 把同一事件的若干 data 行按 `\n` 连接成单条载荷（一次拼接）。
fn join_data_parts(buffer: &str, parts: &[(usize, usize)]) -> String {
    let mut event = String::new();
    for (index, (start, end)) in parts.iter().copied().enumerate() {
        if index > 0 {
            event.push('\n');
        }
        event.push_str(&buffer[start..end]);
    }
    event
}

/// 把一个 JSON 载荷编码为客户端 SSE 帧（`data: ...\n\n`）。
pub fn sse_frame(payload: &str) -> Vec<u8> {
    format!("data: {payload}\n\n").into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitter_handles_partial_and_multiline_events() {
        let mut splitter = SseSplitter::default();
        assert!(splitter.feed("data: {\"a\":1").is_empty());
        let events = splitter.feed("}\n\ndata: [DONE]\n\n");
        assert_eq!(events, vec!["{\"a\":1}".to_string(), "[DONE]".to_string()]);
    }

    #[test]
    fn splitter_joins_multiple_data_lines_and_strips_crlf() {
        let mut splitter = SseSplitter::default();
        let events = splitter.feed("data: line1\r\ndata: line2\r\n\r\n");
        assert_eq!(events, vec!["line1\nline2".to_string()]);
    }

    #[test]
    fn splitter_ignores_comments_and_event_fields() {
        let mut splitter = SseSplitter::default();
        let events = splitter.feed(": ping\nevent: message_start\ndata: {\"x\":2}\n\n");
        assert_eq!(events, vec!["{\"x\":2}".to_string()]);
    }
}

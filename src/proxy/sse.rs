//! SSE 解析与写出工具。

/// 增量拆分上游字节流为完整 SSE `data:` 载荷。
///
/// 只关心 data 行；事件以空行分隔，同一事件内的多个 data 行按规范以
/// `\n` 连接（OpenAI/Anthropic/Gemini 实践中每事件都是单行 data）。
#[derive(Debug, Default)]
pub struct SseSplitter {
    buffer: String,
    data_lines: Vec<String>,
}

impl SseSplitter {
    /// 喂入一段（可能不完整的）UTF-8 文本。
    pub fn feed(&mut self, text: &str) -> Vec<String> {
        self.buffer.push_str(text);
        let mut events = Vec::new();
        while let Some(pos) = self.buffer.find('\n') {
            let line = self.buffer[..pos].to_string();
            self.buffer.drain(..pos + 1);
            let line = line.trim_end_matches('\r');
            if line.is_empty() {
                if !self.data_lines.is_empty() {
                    events.push(self.data_lines.join("\n"));
                    self.data_lines.clear();
                }
                continue;
            }
            if let Some(data) = line.strip_prefix("data:") {
                let data = data.strip_prefix(' ').unwrap_or(data);
                if data == "[DONE]" {
                    self.data_lines.clear();
                    events.push("[DONE]".to_string());
                } else {
                    self.data_lines.push(data.to_string());
                }
            }
            // 其他字段（event:/id:/retry:）与注释行忽略。
        }
        events
    }
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

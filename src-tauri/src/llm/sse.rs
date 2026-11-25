/// Borrowed Server-Sent Event frame emitted by `SseParser`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SseEvent<'a> {
    pub event: Option<&'a str>,
    pub data: &'a str,
}

/// Fast, incremental Server-Sent Events parser supporting arbitrary TCP chunk splits.
///
/// Reuses internal buffers across frames and takes a zero-copy fast path whenever
/// incoming chunks contain complete lines without prior buffered bytes.
#[derive(Debug, Default)]
pub struct SseParser {
    line_buf: Vec<u8>,
    event_name: String,
    has_event_name: bool,
    data_buf: String,
    has_data: bool,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed an arbitrary byte slice from the network stream and invoke `on_event`
    /// for each complete SSE frame delimited by a blank line.
    pub fn feed<F>(&mut self, chunk: &[u8], mut on_event: F)
    where
        F: FnMut(SseEvent<'_>),
    {
        if chunk.is_empty() {
            return;
        }

        let mut offset = 0;

        // If we have a partial line buffered from a previous chunk, finish that line first.
        if !self.line_buf.is_empty() {
            if let Some(nl_pos) = find_newline(chunk) {
                self.line_buf.extend_from_slice(&chunk[..nl_pos]);
                let line = strip_trailing_cr(&self.line_buf);
                let line_str = String::from_utf8_lossy(line).into_owned();
                self.line_buf.clear();
                self.process_line(&line_str, &mut on_event);
                offset = nl_pos + 1;
            } else {
                self.line_buf.extend_from_slice(chunk);
                return;
            }
        }

        // Fast path: scan complete lines directly in `chunk` without copying.
        while offset < chunk.len() {
            let remaining = &chunk[offset..];
            if let Some(nl_pos) = find_newline(remaining) {
                let raw_line = strip_trailing_cr(&remaining[..nl_pos]);
                match std::str::from_utf8(raw_line) {
                    Ok(line) => self.process_line(line, &mut on_event),
                    Err(_) => {
                        let lossy = String::from_utf8_lossy(raw_line);
                        self.process_line(&lossy, &mut on_event);
                    }
                }
                offset += nl_pos + 1;
            } else {
                self.line_buf.extend_from_slice(remaining);
                break;
            }
        }
    }

    /// Flush any remaining buffered line and dispatch a pending event at EOF.
    pub fn finish<F>(&mut self, mut on_event: F)
    where
        F: FnMut(SseEvent<'_>),
    {
        if !self.line_buf.is_empty() {
            let line = strip_trailing_cr(&self.line_buf);
            let line_str = String::from_utf8_lossy(line).into_owned();
            self.line_buf.clear();
            self.process_line(&line_str, &mut on_event);
        }
        self.dispatch(&mut on_event);
    }

    fn process_line<F>(&mut self, line: &str, on_event: &mut F)
    where
        F: FnMut(SseEvent<'_>),
    {
        if line.is_empty() {
            self.dispatch(on_event);
            return;
        }

        // SSE comment line
        if line.starts_with(':') {
            return;
        }

        let (field, value) = match line.split_once(':') {
            Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
            None => (line, ""),
        };

        match field {
            "data" => {
                if self.has_data {
                    self.data_buf.push('\n');
                }
                self.data_buf.push_str(value);
                self.has_data = true;
            }
            "event" => {
                self.event_name.clear();
                self.event_name.push_str(value);
                self.has_event_name = true;
            }
            _ => {
                // Ignore `id`, `retry`, and extension fields.
            }
        }
    }

    fn dispatch<F>(&mut self, on_event: &mut F)
    where
        F: FnMut(SseEvent<'_>),
    {
        if !self.has_data {
            self.event_name.clear();
            self.has_event_name = false;
            return;
        }

        let event = if self.has_event_name {
            Some(self.event_name.as_str())
        } else {
            None
        };

        on_event(SseEvent {
            event,
            data: self.data_buf.as_str(),
        });

        self.data_buf.clear();
        self.has_data = false;
        self.event_name.clear();
        self.has_event_name = false;
    }
}

/// Fast, incremental newline-delimited JSON (NDJSON) parser for Ollama streaming.
#[derive(Debug, Default)]
pub struct NdjsonParser {
    line_buf: Vec<u8>,
}

impl NdjsonParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed an arbitrary byte slice from the network stream and invoke `on_line`
    /// for each non-empty trimmed line.
    pub fn feed<F>(&mut self, chunk: &[u8], mut on_line: F)
    where
        F: FnMut(&str),
    {
        if chunk.is_empty() {
            return;
        }

        let mut offset = 0;

        if !self.line_buf.is_empty() {
            if let Some(nl_pos) = find_newline(chunk) {
                self.line_buf.extend_from_slice(&chunk[..nl_pos]);
                let raw = strip_trailing_cr(&self.line_buf);
                let line_str = String::from_utf8_lossy(raw).into_owned();
                self.line_buf.clear();
                let trimmed = line_str.trim();
                if !trimmed.is_empty() {
                    on_line(trimmed);
                }
                offset = nl_pos + 1;
            } else {
                self.line_buf.extend_from_slice(chunk);
                return;
            }
        }

        while offset < chunk.len() {
            let remaining = &chunk[offset..];
            if let Some(nl_pos) = find_newline(remaining) {
                let raw = strip_trailing_cr(&remaining[..nl_pos]);
                match std::str::from_utf8(raw) {
                    Ok(line) => {
                        let trimmed = line.trim();
                        if !trimmed.is_empty() {
                            on_line(trimmed);
                        }
                    }
                    Err(_) => {
                        let lossy = String::from_utf8_lossy(raw);
                        let trimmed = lossy.trim();
                        if !trimmed.is_empty() {
                            on_line(trimmed);
                        }
                    }
                }
                offset += nl_pos + 1;
            } else {
                self.line_buf.extend_from_slice(remaining);
                break;
            }
        }
    }

    /// Flush any remaining buffered line at EOF.
    pub fn finish<F>(&mut self, mut on_line: F)
    where
        F: FnMut(&str),
    {
        if !self.line_buf.is_empty() {
            let raw = strip_trailing_cr(&self.line_buf);
            let line_str = String::from_utf8_lossy(raw).into_owned();
            self.line_buf.clear();
            let trimmed = line_str.trim();
            if !trimmed.is_empty() {
                on_line(trimmed);
            }
        }
    }
}

#[inline]
fn find_newline(bytes: &[u8]) -> Option<usize> {
    bytes.iter().position(|&b| b == b'\n')
}

#[inline]
fn strip_trailing_cr(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\r").unwrap_or(line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn sse_parser_handles_arbitrary_tcp_splits_and_utf8() {
        let payload = concat!(
            ": keep-alive\r\n",
            "event: content_block_delta\r\n",
            "data: {\"text\":\"你好\"}\r\n",
            "\r\n",
            "data: line1\n",
            "data: line2\n",
            "\n",
            "data: [DONE]\n\n"
        );

        // Feed 1 byte at a time to stress every possible UTF-8 and CRLF split boundary.
        let mut parser = SseParser::new();
        let mut events: Vec<(Option<String>, String)> = Vec::new();
        for b in payload.as_bytes() {
            parser.feed(std::slice::from_ref(b), |ev| {
                events.push((ev.event.map(str::to_string), ev.data.to_string()));
            });
        }
        parser.finish(|ev| {
            events.push((ev.event.map(str::to_string), ev.data.to_string()));
        });

        assert_eq!(
            events,
            vec![
                (
                    Some("content_block_delta".to_string()),
                    "{\"text\":\"你好\"}".to_string()
                ),
                (None, "line1\nline2".to_string()),
                (None, "[DONE]".to_string()),
            ]
        );
    }

    #[test]
    fn sse_parser_flushes_trailing_event_without_final_blank_line() {
        let mut parser = SseParser::new();
        let mut events = Vec::new();
        parser.feed(b"data: trailing-payload", |ev| {
            events.push(ev.data.to_string());
        });
        assert!(events.is_empty());
        parser.finish(|ev| events.push(ev.data.to_string()));
        assert_eq!(events, vec!["trailing-payload".to_string()]);
    }

    #[test]
    fn ndjson_parser_handles_splits_and_blank_lines() {
        let stream = "{\"message\":{\"content\":\"hi\"}}\r\n\n{\"done\":true}";
        let mut parser = NdjsonParser::new();
        let mut lines = Vec::new();

        for chunk in stream.as_bytes().chunks(5) {
            parser.feed(chunk, |line| lines.push(line.to_string()));
        }
        parser.finish(|line| lines.push(line.to_string()));

        assert_eq!(
            lines,
            vec![
                "{\"message\":{\"content\":\"hi\"}}".to_string(),
                "{\"done\":true}".to_string(),
            ]
        );
    }

    #[test]
    fn sse_parser_perf_1000_chunks_under_15ms_across_5_runs() {
        let chunks: Vec<Vec<u8>> = (0..1000)
            .map(|i| {
                format!(
                    "data: {{\"choices\":[{{\"delta\":{{\"content\":\"tok{i} \"}}}}]}}\n\n"
                )
                .into_bytes()
            })
            .collect();

        for run in 0..5 {
            let mut parser = SseParser::new();
            let mut count = 0usize;
            let start = Instant::now();
            for chunk in &chunks {
                parser.feed(chunk, |ev| {
                    if !ev.data.is_empty() {
                        count += 1;
                    }
                });
            }
            parser.finish(|ev| {
                if !ev.data.is_empty() {
                    count += 1;
                }
            });
            let elapsed = start.elapsed();
            assert_eq!(count, 1000);
            assert!(
                elapsed.as_secs_f64() * 1000.0 < 15.0,
                "run {} exceeded 15ms SSE parsing budget: {:.3}ms",
                run + 1,
                elapsed.as_secs_f64() * 1000.0
            );
        }
    }
}

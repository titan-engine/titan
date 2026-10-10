//! Bounded capture of owned game output, independent of BRP and MCP stdout.

use alloc::{collections::VecDeque, sync::Arc};
use serde::{Deserialize, Serialize};
use std::{io::Read, sync::Mutex, thread};

const BUFFER_LINES: usize = 1024;
const BUFFER_BYTES: usize = 512 * 1024;
const LINE_BYTES: usize = 1024;
// Leave room for process state and pagination metadata under MCP's 24 KiB cap.
const PAGE_BYTES: usize = 16 * 1024;

/// Level recognized in Bevy's default tracing formatter. Custom output may lack one.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    /// Error.
    Error,
    /// Warning.
    Warn,
    /// Informational message.
    Info,
    /// Debug message.
    Debug,
    /// Trace message.
    Trace,
}

/// Options for reading retained logs. Cursors are exclusive and never reset.
#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LogQuery {
    /// Read only entries newer than this cursor; absent means the recent tail.
    pub since: Option<u64>,
    /// Maximum returned entries, in `1..=100` (default 50).
    pub max_lines: usize,
    /// Exact parsed level, not a severity threshold. Unstructured lines do not match.
    pub level: Option<LogLevel>,
    /// Case-sensitive substring of the captured text.
    pub contains: Option<String>,
}

impl Default for LogQuery {
    fn default() -> Self {
        Self {
            since: None,
            max_lines: 50,
            level: None,
            contains: None,
        }
    }
}

/// One captured line. Ordering across streams is the order observed by the readers.
#[derive(Clone, Debug, Serialize)]
pub struct LogLine {
    /// Monotonically increasing entry ID; use the page cursor for polling.
    pub cursor: u64,
    /// `stdout` or `stderr`.
    pub stream: &'static str,
    /// Parsed Bevy level, or `null` for unstructured/custom output.
    pub level: Option<LogLevel>,
    /// Text without newline, CRLF terminator, or ANSI CSI color sequences.
    pub text: String,
    /// Bytes discarded from an overlong line; the pipes are still fully drained.
    pub truncated_bytes: u64,
}

/// A bounded page and explicit retention/pagination information.
#[derive(Debug, Serialize)]
pub struct LogPage {
    /// Returned entries in cursor order.
    pub lines: Vec<LogLine>,
    /// Continue with `since: cursor`. Includes scanned nonmatching entries.
    pub cursor: u64,
    /// Latest observed cursor, even if more entries remain on this page.
    pub latest_cursor: u64,
    /// Earliest retained entry, or `null` before any output.
    pub oldest_cursor: Option<u64>,
    /// Entries lost to ring eviction since the requested cursor (or in total without one).
    pub dropped_lines: u64,
    /// Matching retained entries not returned due to line/byte caps.
    pub omitted_lines: usize,
    /// Whether another page of matching entries can be read with this cursor.
    pub has_more: bool,
    /// Last reader I/O failure, if any; output captured so far remains readable.
    pub reader_error: Option<String>,
}

#[derive(Default)]
struct Buffer {
    lines: VecDeque<LogLine>,
    bytes: usize,
    cursor: u64,
    dropped: u64,
    reader_error: Option<String>,
}

/// Shared internally with pipe-draining workers. No I/O occurs under the lock.
#[derive(Clone, Default)]
pub(crate) struct GameLogs(Arc<Mutex<Buffer>>);

impl GameLogs {
    pub(crate) fn cursor(&self) -> u64 {
        self.0.lock().unwrap().cursor
    }

    fn push(&self, stream: &'static str, bytes: &[u8], truncated_bytes: u64) {
        let text = strip_ansi(&String::from_utf8_lossy(bytes));
        let mut buffer = self.0.lock().unwrap();
        buffer.cursor += 1;
        let cursor = buffer.cursor;
        buffer.bytes += text.len();
        buffer.lines.push_back(LogLine {
            cursor,
            stream,
            level: parse_level(&text),
            text,
            truncated_bytes,
        });
        while buffer.lines.len() > BUFFER_LINES || buffer.bytes > BUFFER_BYTES {
            let line = buffer.lines.pop_front().unwrap();
            buffer.bytes -= line.text.len();
            buffer.dropped += 1;
        }
    }

    pub(crate) fn read(&self, query: &LogQuery) -> Result<LogPage, String> {
        if !(1..=100).contains(&query.max_lines) {
            return Err("`max_lines` must be an integer between 1 and 100".to_owned());
        }
        let buffer = self.0.lock().unwrap();
        let since = query.since.unwrap_or(0);
        if since > buffer.cursor {
            return Err("`since` is ahead of the latest game log cursor".to_owned());
        }
        let matches: Vec<_> = buffer
            .lines
            .iter()
            .filter(|line| {
                line.cursor > since
                    && query.level.is_none_or(|level| line.level == Some(level))
                    && query
                        .contains
                        .as_ref()
                        .is_none_or(|s| line.text.contains(s))
            })
            .collect();
        let mut lines = Vec::new();
        let mut bytes = 0;
        // Without a cursor, keep the most recent matching entries within both caps.
        let candidates: Vec<_> = if query.since.is_none() {
            matches.iter().rev().copied().collect()
        } else {
            matches.clone()
        };
        for line in candidates {
            let size = serde_json::to_vec(line).map_err(|e| e.to_string())?.len();
            if lines.len() == query.max_lines || bytes + size > PAGE_BYTES {
                break;
            }
            bytes += size;
            lines.push(line.clone());
        }
        if query.since.is_none() {
            lines.reverse();
        }
        let omitted_lines = matches.len() - lines.len();
        let has_more = query.since.is_some() && omitted_lines > 0;
        let cursor = if has_more {
            lines.last().map_or(since, |line| line.cursor)
        } else {
            buffer.cursor
        };
        Ok(LogPage {
            lines,
            cursor,
            latest_cursor: buffer.cursor,
            oldest_cursor: buffer.lines.front().map(|line| line.cursor),
            dropped_lines: buffer.dropped.saturating_sub(since),
            omitted_lines,
            has_more,
            reader_error: buffer.reader_error.clone(),
        })
    }

    pub(crate) fn capture(
        &self,
        mut reader: impl Read + Send + 'static,
        stream: &'static str,
    ) -> thread::JoinHandle<()> {
        let logs = self.clone();
        thread::spawn(move || {
            let mut line = Vec::with_capacity(LINE_BYTES);
            let mut discarded = 0_u64;
            let mut chunk = [0; 4096];
            loop {
                let count = match reader.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(count) => count,
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) => {
                        logs.0.lock().unwrap().reader_error = Some(format!("{stream}: {error}"));
                        break;
                    }
                };
                for byte in &chunk[..count] {
                    if *byte == b'\n' {
                        if discarded == 0 && line.last() == Some(&b'\r') {
                            line.pop();
                        }
                        logs.push(stream, &line, discarded);
                        line.clear();
                        discarded = 0;
                    } else if line.len() < LINE_BYTES {
                        line.push(*byte);
                    } else {
                        discarded = discarded.saturating_add(1);
                    }
                }
            }
            // Preserve panic messages / final output even without a trailing newline.
            if !line.is_empty() || discarded > 0 {
                logs.push(stream, &line, discarded);
            }
        })
    }
}

fn strip_ansi(text: &str) -> String {
    let mut chars = text.chars().peekable();
    let mut result = String::new();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for c in chars.by_ref() {
                if ('@'..='~').contains(&c) {
                    break;
                }
            }
        } else {
            result.push(c);
        }
    }
    result
}

fn parse_level(text: &str) -> Option<LogLevel> {
    let mut fields = text.split_whitespace();
    let first = fields.next()?;
    // tracing_subscriber's default formatter: optional ISO timestamp, then level.
    let token = if first.as_bytes().get(4) == Some(&b'-') && first.contains('T') {
        fields.next()?
    } else {
        first
    };
    match token {
        "ERROR" => Some(LogLevel::Error),
        "WARN" => Some(LogLevel::Warn),
        "INFO" => Some(LogLevel::Info),
        "DEBUG" => Some(LogLevel::Debug),
        "TRACE" => Some(LogLevel::Trace),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_capture_drains_and_preserves_final_partial_lines() {
        let logs = GameLogs::default();
        let input = format!("{}\r\nlast?", "x".repeat(100_000));
        logs.capture(std::io::Cursor::new(input.into_bytes()), "stderr")
            .join()
            .unwrap();
        let page = logs.read(&LogQuery::default()).unwrap();
        assert_eq!(page.lines[0].text.len(), LINE_BYTES);
        assert_eq!(
            page.lines[0].truncated_bytes,
            100_000 + 1 - LINE_BYTES as u64
        );
        assert_eq!(page.lines[1].text, "last?");
    }

    #[test]
    fn parses_default_levels_and_leaves_unstructured_output_alone() {
        let logs = GameLogs::default();
        for text in [
            "2026-04-01T12:00:00.123Z \x1b[33m WARN\x1b[0m game: missing asset",
            "INFO game: ready",
            "thread 'main' panicked: ERROR is not a formatted level",
        ] {
            logs.push("stderr", text.as_bytes(), 0);
        }
        let page = logs.read(&LogQuery::default()).unwrap();
        assert_eq!(page.lines[0].level, Some(LogLevel::Warn));
        assert!(!page.lines[0].text.contains('\u{1b}'));
        assert_eq!(page.lines[1].level, Some(LogLevel::Info));
        assert_eq!(page.lines[2].level, None);
        let filtered = logs
            .read(&LogQuery {
                since: Some(0),
                level: Some(LogLevel::Warn),
                contains: Some("asset".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(filtered.lines.len(), 1);
        assert_eq!(filtered.cursor, 3);
    }

    #[test]
    fn eviction_pagination_and_tail_have_explicit_counts() {
        let logs = GameLogs::default();
        for _ in 0..BUFFER_LINES + 10 {
            logs.push("stdout", b"hello", 0);
        }
        let page = logs
            .read(&LogQuery {
                since: Some(0),
                max_lines: 100,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(page.dropped_lines, 10);
        assert_eq!(page.oldest_cursor, Some(11));
        assert_eq!(page.cursor, 110);
        assert_eq!(page.omitted_lines, BUFFER_LINES - 100);
        assert!(page.has_more);
        let next = logs
            .read(&LogQuery {
                since: Some(page.cursor),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(next.dropped_lines, 0);
        assert_eq!(next.lines[0].cursor, 111);
        let tail = logs.read(&LogQuery::default()).unwrap();
        assert_eq!(tail.lines.len(), 50);
        assert_eq!(tail.cursor, (BUFFER_LINES + 10) as u64);
        assert!(!tail.has_more);
    }

    #[test]
    fn byte_caps_include_json_escaping_and_invalid_utf8() {
        let logs = GameLogs::default();
        for _ in 0..BUFFER_LINES {
            logs.push("stdout", &vec![0xff; LINE_BYTES], 0);
        }
        let page = logs
            .read(&LogQuery {
                since: Some(0),
                max_lines: 100,
                ..Default::default()
            })
            .unwrap();
        assert!(page.dropped_lines > 0);
        assert!(page.has_more);
        assert!(serde_json::to_vec(&page).unwrap().len() < 24 * 1024);
        let logs = GameLogs::default();
        for _ in 0..100 {
            logs.push("stderr", &vec![0; LINE_BYTES], 0);
        }
        let page = logs.read(&LogQuery::default()).unwrap();
        assert!(!page.lines.is_empty());
        assert!(serde_json::to_vec(&page).unwrap().len() < 24 * 1024);
    }
}

use serde_json::Value;

pub(crate) struct SseParser {
    buffer: Vec<u8>,
}

impl SseParser {
    pub fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    pub fn push(&mut self, chunk: &[u8]) -> Vec<Value> {
        self.buffer.extend_from_slice(chunk);
        let mut events = Vec::new();
        while let Some(newline) = self.buffer.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.buffer.drain(..=newline).collect();
            if let Some(value) = parse_line(&line) {
                events.push(value);
            }
        }
        events
    }
}

fn parse_line(raw: &[u8]) -> Option<Value> {
    let line = String::from_utf8_lossy(raw);
    let line = line.trim_end_matches(['\n', '\r']);
    if line.is_empty() || line.starts_with(':') {
        return None;
    }
    let payload = match line.strip_prefix("data:") {
        Some(rest) => rest.strip_prefix(' ').unwrap_or(rest),
        None if is_ignored_field(line) => return None,
        None => line,
    };
    let payload = payload.trim();
    if payload.is_empty() || payload == "[DONE]" {
        return None;
    }
    serde_json::from_str(payload).ok()
}

fn is_ignored_field(line: &str) -> bool {
    ["event:", "id:", "retry:"]
        .iter()
        .any(|field| line.starts_with(field))
}

#[cfg(test)]
#[path = "sse_tests.rs"]
mod tests;

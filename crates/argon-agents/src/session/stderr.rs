use std::collections::VecDeque;
use std::sync::Mutex;

use crate::util::lock;

const CAPACITY: usize = 200;
const REDACTED: &str = "[redigido]";
const TOKEN_PREFIXES: [&str; 12] = [
    "sk-",
    "sk_",
    "ghp_",
    "gho_",
    "ghs_",
    "ghu_",
    "ghr_",
    "github_pat_",
    "xoxb-",
    "xoxp-",
    "AKIA",
    "ASIA",
];
const SECRET_KEYS: [&str; 7] = [
    "token",
    "secret",
    "password",
    "passwd",
    "apikey",
    "api_key",
    "authorization",
];
const MIN_SECRET_LEN: usize = 12;

#[derive(Default)]
pub(super) struct StderrTail {
    lines: Mutex<VecDeque<String>>,
}

impl StderrTail {
    pub fn push(&self, line: String) {
        let mut lines = lock(&self.lines);
        if lines.len() == CAPACITY {
            lines.pop_front();
        }
        lines.push_back(line);
    }

    pub fn last(&self, count: usize) -> Vec<String> {
        let lines = lock(&self.lines);
        let skip = lines.len().saturating_sub(count);
        lines.iter().skip(skip).map(|line| redact(line)).collect()
    }
}

fn is_jwt(word: &str) -> bool {
    word.starts_with("eyJ") && word.matches('.').count() == 2 && word.len() >= MIN_SECRET_LEN
}

fn looks_like_token(word: &str) -> bool {
    let trimmed = word.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_');
    is_jwt(trimmed)
        || TOKEN_PREFIXES
            .iter()
            .any(|prefix| trimmed.starts_with(prefix) && trimmed.len() >= MIN_SECRET_LEN)
}

fn secret_assignment(word: &str) -> Option<usize> {
    let lower = word.to_ascii_lowercase();
    let separator = lower.find(['=', ':'])?;
    let key = lower[..separator].trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_');
    let value = &word[separator + 1..];
    let is_secret_key = SECRET_KEYS.iter().any(|secret| key.ends_with(secret));
    (is_secret_key && value.trim_matches(['"', '\'']).len() > 1).then_some(separator + 1)
}

pub(super) fn redact(line: &str) -> String {
    let mut out = Vec::new();
    let mut hide_next = false;
    for word in line.split(' ') {
        let lower = word.to_ascii_lowercase();
        if hide_next && !word.is_empty() && lower != "bearer" {
            out.push(REDACTED.to_owned());
            hide_next = false;
            continue;
        }
        if lower == "bearer" || lower.ends_with(":bearer") {
            hide_next = true;
            out.push(word.to_owned());
        } else if looks_like_token(word) {
            out.push(REDACTED.to_owned());
        } else if let Some(cut) = secret_assignment(word) {
            out.push(format!("{}{REDACTED}", &word[..cut]));
        } else if SECRET_KEYS
            .iter()
            .any(|key| lower.trim_end_matches(':').ends_with(key))
            && lower.ends_with(':')
        {
            hide_next = true;
            out.push(word.to_owned());
        } else {
            out.push(word.to_owned());
        }
    }
    out.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_known_token_shapes() {
        let line = "auth failed for sk-ant-api03-abcdefghijklmnop and ghp_0123456789abcdefghij";
        let redacted = redact(line);
        assert!(!redacted.contains("sk-ant"), "{redacted}");
        assert!(!redacted.contains("ghp_0123"), "{redacted}");
        assert!(redacted.starts_with("auth failed for"));
    }

    #[test]
    fn redacts_bearer_jwt_and_assignments() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.c2lnbmF0dXJl";
        let redacted = redact(&format!(
            "Authorization: Bearer abc.def.ghi token=supersecretvalue {jwt} API_KEY: \"xyz123456\""
        ));
        assert!(!redacted.contains("abc.def.ghi"), "{redacted}");
        assert!(!redacted.contains("supersecretvalue"), "{redacted}");
        assert!(!redacted.contains(jwt), "{redacted}");
        assert!(!redacted.contains("xyz123456"), "{redacted}");
        assert!(redacted.contains("token=[redigido]"), "{redacted}");
    }

    #[test]
    fn keeps_ordinary_lines() {
        let line = "POST /alpha/generate → 401 error: Invalid 'Authorization' header";
        assert_eq!(redact(line), line);
    }

    #[test]
    fn tail_keeps_only_last_lines() {
        let tail = StderrTail::default();
        for index in 0..250 {
            tail.push(format!("line {index}"));
        }
        let last = tail.last(20);
        assert_eq!(last.len(), 20);
        assert_eq!(last[0], "line 230");
        assert_eq!(last[19], "line 249");
    }
}

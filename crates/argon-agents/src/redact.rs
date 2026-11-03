const REDACTED: &str = "[redigido]";
const PRIVATE_KEY_BEGIN: &str = "-----BEGIN ";
const PRIVATE_KEY_END: &str = "-----END ";
const PRIVATE_KEY_MARKER: &str = "PRIVATE KEY-----";
const TOKEN_PREFIXES: [&str; 15] = [
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
    "xoxa-",
    "xoxr-",
    "xoxs-",
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

pub fn redact_line(line: &str) -> String {
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

fn is_key_begin(line: &str) -> bool {
    line.contains(PRIVATE_KEY_BEGIN) && line.contains(PRIVATE_KEY_MARKER)
}

fn is_key_end(line: &str) -> bool {
    line.contains(PRIVATE_KEY_END) && line.contains(PRIVATE_KEY_MARKER)
}

pub fn redact_text(text: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut in_key = false;
    for line in text.split('\n') {
        if in_key {
            if is_key_end(line) {
                in_key = false;
            }
            continue;
        }
        if is_key_begin(line) {
            out.push(REDACTED.to_owned());
            in_key = !is_key_end(line);
            continue;
        }
        out.push(redact_line(line));
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_known_token_shapes() {
        let line = "auth failed for sk-ant-api03-abcdefghijklmnop and ghp_0123456789abcdefghij";
        let redacted = redact_line(line);
        assert!(!redacted.contains("sk-ant"), "{redacted}");
        assert!(!redacted.contains("ghp_0123"), "{redacted}");
        assert!(redacted.starts_with("auth failed for"));
    }

    #[test]
    fn redacts_bearer_jwt_and_assignments() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.c2lnbmF0dXJl";
        let redacted = redact_line(&format!(
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
        assert_eq!(redact_line(line), line);
    }

    #[test]
    fn redacts_more_token_shapes() {
        let line = "AKIAABCDEFGHIJKLMNOP xoxs-1234567890-abc sk-abcdefghijklmnopqrstuvwx gho_abcdefghijklmnop github_pat_11ABCDEFG0123456789";
        let redacted = redact_line(line);
        for secret in [
            "AKIAABCD",
            "xoxs-",
            "sk-abcdef",
            "gho_abcd",
            "github_pat_11",
        ] {
            assert!(!redacted.contains(secret), "{secret} in {redacted}");
        }
    }

    #[test]
    fn redact_text_drops_private_key_blocks() {
        let text = "before\n-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQEA\nabc\n\
                    -----END RSA PRIVATE KEY-----\nafter password=hunter22";
        let redacted = redact_text(text);
        assert_eq!(redacted, "before\n[redigido]\nafter password=[redigido]");
    }

    #[test]
    fn redact_text_keeps_plain_multiline_text() {
        let text = "linha 1\nlinha 2 com `cargo test`";
        assert_eq!(redact_text(text), text);
    }
}

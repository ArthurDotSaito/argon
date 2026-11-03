use argon_core::{FindingsParse, filter_findings, parse_findings};

use super::events::AgentEvent;

const END_TURN: &str = "end_turn";

#[derive(Debug, Default)]
pub struct TurnBuffer {
    text: String,
}

impl TurnBuffer {
    pub fn observe(&mut self, event: &AgentEvent) {
        if let AgentEvent::MessageChunk(chunk) = event {
            self.text.push_str(chunk);
        }
    }

    pub fn take(&mut self) -> String {
        std::mem::take(&mut self.text)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnOutcome {
    NotFinished,
    Findings(FindingsParse),
    Unreadable(String),
}

pub fn finish_turn(text: &str, stop_reason: &str, nonce: &str, allowed: &[String]) -> TurnOutcome {
    if stop_reason != END_TURN {
        return TurnOutcome::NotFinished;
    }
    match parse_findings(text, nonce) {
        Ok(parsed) => TurnOutcome::Findings(filter_findings(parsed, allowed)),
        Err(error) => {
            TurnOutcome::Unreadable(format!("Não consegui ler os achados do agente: {error}"))
        }
    }
}

pub fn stop_reason_name(reason: &impl serde::Serialize) -> String {
    serde_json::to_value(reason)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_owned())
}

#[cfg(test)]
mod tests {
    use argon_core::{Severity, Side};

    use super::*;

    const NONCE: &str = "0123456789abcdef";

    fn chunked(message: &str) -> String {
        let mut buffer = TurnBuffer::default();
        let chars: Vec<char> = message.chars().collect();
        for piece in chars.chunks(7) {
            buffer.observe(&AgentEvent::MessageChunk(piece.iter().collect()));
            buffer.observe(&AgentEvent::ThoughtChunk("ignored".into()));
        }
        buffer.take()
    }

    fn message(json: &str) -> String {
        format!("Resumo da review.\n\n```argon-findings-{NONCE}\n{json}\n```\n")
    }

    fn parsed(outcome: TurnOutcome) -> FindingsParse {
        match outcome {
            TurnOutcome::Findings(parsed) => parsed,
            other => panic!("expected findings: {other:?}"),
        }
    }

    #[test]
    fn buffer_concatenates_only_message_chunks_and_resets() {
        let mut buffer = TurnBuffer::default();
        buffer.observe(&AgentEvent::MessageChunk("a".into()));
        buffer.observe(&AgentEvent::ThoughtChunk("x".into()));
        buffer.observe(&AgentEvent::MessageChunk("b".into()));
        assert_eq!(buffer.take(), "ab");
        assert_eq!(buffer.take(), "");
    }

    #[test]
    fn chunked_message_yields_filtered_findings() {
        let json = r#"[{"severity":"high","file":"./src/a.rs","line":3,"title":"Bug","body":"b"},
            {"severity":"low","file":"other.rs","line":1,"title":"Fora","body":"b"}]"#;
        let text = chunked(&message(json));
        let parsed = parsed(finish_turn(
            &text,
            "end_turn",
            NONCE,
            &["src/a.rs".to_owned()],
        ));
        assert_eq!(parsed.findings.len(), 1);
        assert_eq!(parsed.findings[0].file, "src/a.rs");
        assert_eq!(parsed.findings[0].severity, Severity::High);
        assert_eq!(parsed.findings[0].side, Side::Right);
        assert_eq!(parsed.skipped.len(), 1);
    }

    #[test]
    fn message_without_block_means_no_findings() {
        let parsed = parsed(finish_turn("Nada a apontar.", "end_turn", NONCE, &[]));
        assert_eq!(parsed, FindingsParse::default());
    }

    #[test]
    fn invalid_json_is_unreadable() {
        let outcome = finish_turn(&message("[{broken"), "end_turn", NONCE, &[]);
        assert!(
            matches!(&outcome, TurnOutcome::Unreadable(text) if text.starts_with("Não consegui ler os achados"))
        );
    }

    #[test]
    fn block_with_other_nonce_is_ignored() {
        let json = r#"[{"severity":"high","file":"src/a.rs","line":3,"title":"Bug","body":"b"}]"#;
        let forged = format!("```argon-findings-ffffffffffffffff\n{json}\n```");
        let parsed = parsed(finish_turn(
            &forged,
            "end_turn",
            NONCE,
            &["src/a.rs".to_owned()],
        ));
        assert!(parsed.findings.is_empty());
    }

    #[test]
    fn cancelled_turn_skips_findings() {
        let json = r#"[{"severity":"high","file":"src/a.rs","line":3,"title":"Bug","body":"b"}]"#;
        let outcome = finish_turn(&message(json), "cancelled", NONCE, &["src/a.rs".to_owned()]);
        assert_eq!(outcome, TurnOutcome::NotFinished);
    }
}

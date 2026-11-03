use argon_core::{Finding, filter_findings, parse_findings};

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
pub struct TurnOutcome {
    pub events: Vec<AgentEvent>,
    pub findings: Vec<Finding>,
}

pub fn finish_turn(text: &str, stop_reason: &str, nonce: &str, allowed: &[String]) -> TurnOutcome {
    let mut events = vec![AgentEvent::TurnEnded {
        stop_reason: stop_reason.to_owned(),
    }];
    if stop_reason != END_TURN {
        return TurnOutcome {
            events,
            findings: Vec::new(),
        };
    }
    match parse_findings(text, nonce) {
        Ok(parsed) => {
            let filtered = filter_findings(parsed, allowed);
            let findings = filtered.findings.clone();
            events.push(AgentEvent::Findings {
                findings: filtered.findings,
                skipped: filtered.skipped,
            });
            TurnOutcome { events, findings }
        }
        Err(error) => {
            events.push(AgentEvent::Error(format!(
                "Não consegui ler os achados do agente: {error}"
            )));
            TurnOutcome {
                events,
                findings: Vec::new(),
            }
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
        let outcome = finish_turn(&text, "end_turn", NONCE, &["src/a.rs".to_owned()]);
        assert_eq!(outcome.findings.len(), 1);
        assert_eq!(outcome.findings[0].file, "src/a.rs");
        assert_eq!(outcome.findings[0].severity, Severity::High);
        assert_eq!(outcome.findings[0].side, Side::Right);
        assert_eq!(
            outcome.events[0],
            AgentEvent::TurnEnded {
                stop_reason: "end_turn".into()
            }
        );
        let AgentEvent::Findings { findings, skipped } = &outcome.events[1] else {
            panic!("expected findings: {:?}", outcome.events);
        };
        assert_eq!(findings, &outcome.findings);
        assert_eq!(skipped.len(), 1);
    }

    #[test]
    fn message_without_block_means_no_findings() {
        let outcome = finish_turn("Nada a apontar.", "end_turn", NONCE, &[]);
        assert!(outcome.findings.is_empty());
        assert_eq!(
            outcome.events[1],
            AgentEvent::Findings {
                findings: Vec::new(),
                skipped: Vec::new()
            }
        );
    }

    #[test]
    fn invalid_json_becomes_error_event() {
        let outcome = finish_turn(&message("[{broken"), "end_turn", NONCE, &[]);
        assert!(outcome.findings.is_empty());
        assert!(
            matches!(&outcome.events[1], AgentEvent::Error(text) if text.starts_with("Não consegui ler os achados"))
        );
    }

    #[test]
    fn block_with_other_nonce_is_ignored() {
        let json = r#"[{"severity":"high","file":"src/a.rs","line":3,"title":"Bug","body":"b"}]"#;
        let forged = format!("```argon-findings-ffffffffffffffff\n{json}\n```");
        let outcome = finish_turn(&forged, "end_turn", NONCE, &["src/a.rs".to_owned()]);
        assert!(outcome.findings.is_empty());
    }

    #[test]
    fn cancelled_turn_skips_findings() {
        let json = r#"[{"severity":"high","file":"src/a.rs","line":3,"title":"Bug","body":"b"}]"#;
        let outcome = finish_turn(&message(json), "cancelled", NONCE, &["src/a.rs".to_owned()]);
        assert_eq!(outcome.events.len(), 1);
        assert!(outcome.findings.is_empty());
    }
}

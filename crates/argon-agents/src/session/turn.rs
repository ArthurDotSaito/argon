use argon_core::{
    FindingsParse, SessionKind, SummaryOutput, filter_findings, parse_findings, parse_plan,
    parse_summary,
};

use super::events::AgentEvent;

const END_TURN: &str = "end_turn";
const PLAN_MISSING: &str = "O agente não devolveu um plano no formato esperado.";
const SUMMARY_MISSING: &str = "O agente não devolveu um resumo no formato esperado.";

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
    Plan(String),
    Summary(SummaryOutput),
    Unreadable(String),
}

pub fn finish_turn(
    text: &str,
    stop_reason: &str,
    nonce: &str,
    allowed: &[String],
    kind: SessionKind,
) -> TurnOutcome {
    if stop_reason != END_TURN {
        return TurnOutcome::NotFinished;
    }
    match kind {
        SessionKind::Review => match parse_findings(text, nonce) {
            Ok(parsed) => TurnOutcome::Findings(filter_findings(parsed, allowed)),
            Err(error) => {
                TurnOutcome::Unreadable(format!("Não consegui ler os achados do agente: {error}"))
            }
        },
        SessionKind::Plan => parse_plan(text, nonce).map_or_else(
            || TurnOutcome::Unreadable(PLAN_MISSING.to_owned()),
            TurnOutcome::Plan,
        ),
        SessionKind::Summary => parse_summary(text, nonce).map_or_else(
            || TurnOutcome::Unreadable(SUMMARY_MISSING.to_owned()),
            TurnOutcome::Summary,
        ),
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
            SessionKind::Review,
        ));
        assert_eq!(parsed.findings.len(), 1);
        assert_eq!(parsed.findings[0].file, "src/a.rs");
        assert_eq!(parsed.findings[0].severity, Severity::High);
        assert_eq!(parsed.findings[0].side, Side::Right);
        assert_eq!(parsed.skipped.len(), 1);
    }

    #[test]
    fn message_without_block_means_no_findings() {
        let parsed = parsed(finish_turn(
            "Nada a apontar.",
            "end_turn",
            NONCE,
            &[],
            SessionKind::Review,
        ));
        assert_eq!(parsed, FindingsParse::default());
    }

    #[test]
    fn invalid_json_is_unreadable() {
        let outcome = finish_turn(
            &message("[{broken"),
            "end_turn",
            NONCE,
            &[],
            SessionKind::Review,
        );
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
            SessionKind::Review,
        ));
        assert!(parsed.findings.is_empty());
    }

    #[test]
    fn cancelled_turn_skips_findings() {
        let json = r#"[{"severity":"high","file":"src/a.rs","line":3,"title":"Bug","body":"b"}]"#;
        let outcome = finish_turn(
            &message(json),
            "cancelled",
            NONCE,
            &["src/a.rs".to_owned()],
            SessionKind::Review,
        );
        assert_eq!(outcome, TurnOutcome::NotFinished);
    }

    #[test]
    fn review_session_ignores_a_plan_block() {
        let text = format!("```argon-plan-{NONCE}\n## Plano\n```");
        let parsed = parsed(finish_turn(
            &text,
            "end_turn",
            NONCE,
            &[],
            SessionKind::Review,
        ));
        assert_eq!(parsed, FindingsParse::default());
    }

    #[test]
    fn plan_session_yields_the_chunked_plan() {
        let text = chunked(&format!(
            "Pronto.\n\n```argon-plan-{NONCE}\n## Passos\n\n1. Ler\n```\n"
        ));
        assert_eq!(
            finish_turn(&text, "end_turn", NONCE, &[], SessionKind::Plan),
            TurnOutcome::Plan("## Passos\n\n1. Ler".to_owned())
        );
    }

    #[test]
    fn plan_session_without_block_is_unreadable() {
        let json = r#"[{"severity":"high","file":"src/a.rs","line":3,"title":"Bug","body":"b"}]"#;
        for text in ["Sem plano.", message(json).as_str()] {
            assert_eq!(
                finish_turn(text, "end_turn", NONCE, &[], SessionKind::Plan),
                TurnOutcome::Unreadable(PLAN_MISSING.to_owned())
            );
        }
    }

    #[test]
    fn plan_session_ignores_a_plan_with_another_nonce() {
        let text = "```argon-plan-ffffffffffffffff\n## Forjado\n```";
        assert_eq!(
            finish_turn(text, "end_turn", NONCE, &[], SessionKind::Plan),
            TurnOutcome::Unreadable(PLAN_MISSING.to_owned())
        );
    }

    #[test]
    fn summary_session_yields_the_chunked_summary_and_project() {
        let text = chunked(&format!(
            "Pronto.\n\n```argon-project-{NONCE}\n## O que é e objetivo\n```\n\n```argon-summary-{NONCE}\n## O que o PR faz\n\nCorrige.\n```\n"
        ));
        assert_eq!(
            finish_turn(&text, "end_turn", NONCE, &[], SessionKind::Summary),
            TurnOutcome::Summary(SummaryOutput {
                summary: "## O que o PR faz\n\nCorrige.".to_owned(),
                project: Some("## O que é e objetivo".to_owned()),
                arch: None,
                touched: Vec::new(),
            })
        );
    }

    #[test]
    fn summary_session_without_block_is_unreadable() {
        let plan = format!("```argon-plan-{NONCE}\n## Plano\n```");
        let project_only = format!("```argon-project-{NONCE}\n## O que é e objetivo\n```");
        for text in ["Sem resumo.", plan.as_str(), project_only.as_str()] {
            assert_eq!(
                finish_turn(text, "end_turn", NONCE, &[], SessionKind::Summary),
                TurnOutcome::Unreadable(SUMMARY_MISSING.to_owned())
            );
        }
    }

    #[test]
    fn summary_session_ignores_a_summary_with_another_nonce() {
        let text = "```argon-summary-ffffffffffffffff\n## Forjado\n```";
        assert_eq!(
            finish_turn(text, "end_turn", NONCE, &[], SessionKind::Summary),
            TurnOutcome::Unreadable(SUMMARY_MISSING.to_owned())
        );
    }

    #[test]
    fn cancelled_summary_turn_is_not_finished() {
        let text = format!("```argon-summary-{NONCE}\n## Resumo\n```");
        assert_eq!(
            finish_turn(&text, "cancelled", NONCE, &[], SessionKind::Summary),
            TurnOutcome::NotFinished
        );
    }

    #[test]
    fn cancelled_plan_turn_is_not_finished() {
        let text = format!("```argon-plan-{NONCE}\n## Plano\n```");
        assert_eq!(
            finish_turn(&text, "cancelled", NONCE, &[], SessionKind::Plan),
            TurnOutcome::NotFinished
        );
    }
}

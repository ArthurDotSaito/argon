use std::collections::HashMap;

use argon_core::{PrKey, SessionKind};
use argon_store::SessionStatus;

use super::toast::{Channel, ToastSpec, ToastTarget};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Running,
    Awaiting,
    Completed,
    Failed,
    Stopped,
}

pub fn phase_of(live: bool, status: SessionStatus, pending: usize) -> Phase {
    match (live, status) {
        (true, SessionStatus::Running | SessionStatus::AwaitingUser) if pending > 0 => {
            Phase::Awaiting
        }
        (true, SessionStatus::Running | SessionStatus::AwaitingUser) => Phase::Running,
        (false, SessionStatus::Running | SessionStatus::AwaitingUser) => Phase::Stopped,
        (_, SessionStatus::Completed) => Phase::Completed,
        (_, SessionStatus::Failed) => Phase::Failed,
        (_, SessionStatus::Archived) => Phase::Stopped,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSnapshot {
    pub session_id: String,
    pub pr: PrKey,
    pub number: Option<u64>,
    pub agent_label: String,
    pub kind: SessionKind,
    pub phase: Phase,
    pub findings: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Permission,
    Completed,
    Failed,
}

#[derive(Default)]
pub struct AgentTracker {
    phases: HashMap<String, Phase>,
}

impl AgentTracker {
    pub fn observe(&mut self, snapshots: &[SessionSnapshot]) -> Vec<(Signal, SessionSnapshot)> {
        let mut signals = Vec::new();
        let mut phases = HashMap::new();
        for snapshot in snapshots {
            let signal = self
                .phases
                .get(&snapshot.session_id)
                .and_then(|before| signal_for(*before, snapshot.phase));
            if let Some(signal) = signal {
                signals.push((signal, snapshot.clone()));
            }
            phases.insert(snapshot.session_id.clone(), snapshot.phase);
        }
        self.phases = phases;
        signals
    }
}

fn signal_for(before: Phase, after: Phase) -> Option<Signal> {
    if before == after {
        return None;
    }
    match after {
        Phase::Awaiting => Some(Signal::Permission),
        Phase::Completed => Some(Signal::Completed),
        Phase::Failed => Some(Signal::Failed),
        Phase::Running | Phase::Stopped => None,
    }
}

pub fn toast(signal: Signal, session: &SessionSnapshot) -> ToastSpec {
    let (channel, body) = match signal {
        Signal::Permission => (
            Channel::AgentPermission,
            format!(
                "O {} pede permissão{}",
                session.agent_label,
                number_suffix("em", session.number)
            ),
        ),
        Signal::Completed => (Channel::AgentOutcome, completed_text(session)),
        Signal::Failed => (Channel::AgentOutcome, failed_text(session)),
    };
    ToastSpec {
        channel,
        body,
        target: ToastTarget::AgentSession(session.pr.clone(), session.kind),
    }
}

fn completed_text(session: &SessionSnapshot) -> String {
    let subject = number_suffix("de", session.number);
    match session.kind {
        SessionKind::Review => format!(
            "Review{subject} concluída — {}",
            findings_text(session.findings)
        ),
        SessionKind::Plan => format!("Agent Plan{subject} concluído — rascunho pronto"),
        SessionKind::Summary => format!("Summarize{subject} concluído"),
    }
}

fn failed_text(session: &SessionSnapshot) -> String {
    let subject = number_suffix("de", session.number);
    match session.kind {
        SessionKind::Review => format!("Review{subject} falhou"),
        SessionKind::Plan => format!("Agent Plan{subject} falhou"),
        SessionKind::Summary => format!("Summarize{subject} falhou"),
    }
}

fn number_suffix(preposition: &str, number: Option<u64>) -> String {
    number
        .map(|n| format!(" {preposition} #{n}"))
        .unwrap_or_default()
}

fn findings_text(count: usize) -> String {
    match count {
        0 => "nenhum achado".to_string(),
        1 => "1 achado".to_string(),
        n => format!("{n} achados"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(id: &str, phase: Phase) -> SessionSnapshot {
        SessionSnapshot {
            session_id: id.to_string(),
            pr: PrKey::new("github.com", "pr"),
            number: Some(42),
            agent_label: "Claude Code".to_string(),
            kind: SessionKind::Review,
            phase,
            findings: 3,
        }
    }

    #[test]
    fn phase_follows_liveness_status_and_pending_permissions() {
        assert_eq!(phase_of(true, SessionStatus::Running, 0), Phase::Running);
        assert_eq!(phase_of(true, SessionStatus::Running, 1), Phase::Awaiting);
        assert_eq!(
            phase_of(true, SessionStatus::Completed, 0),
            Phase::Completed
        );
        assert_eq!(phase_of(false, SessionStatus::Failed, 0), Phase::Failed);
        assert_eq!(phase_of(false, SessionStatus::Running, 0), Phase::Stopped);
    }

    #[test]
    fn a_session_seen_for_the_first_time_is_silent() {
        let mut tracker = AgentTracker::default();
        assert!(
            tracker
                .observe(&[snapshot("a", Phase::Awaiting)])
                .is_empty()
        );
    }

    #[test]
    fn transitions_raise_signals() {
        let mut tracker = AgentTracker::default();
        tracker.observe(&[snapshot("a", Phase::Running)]);
        let permission = tracker.observe(&[snapshot("a", Phase::Awaiting)]);
        assert_eq!(permission[0].0, Signal::Permission);
        assert!(
            tracker
                .observe(&[snapshot("a", Phase::Awaiting)])
                .is_empty()
        );
        let done = tracker.observe(&[snapshot("a", Phase::Completed)]);
        assert_eq!(done[0].0, Signal::Completed);
        let failed = tracker.observe(&[snapshot("a", Phase::Failed)]);
        assert_eq!(failed[0].0, Signal::Failed);
    }

    #[test]
    fn resuming_and_stopping_are_silent() {
        let mut tracker = AgentTracker::default();
        tracker.observe(&[snapshot("a", Phase::Completed)]);
        assert!(tracker.observe(&[snapshot("a", Phase::Running)]).is_empty());
        assert!(tracker.observe(&[snapshot("a", Phase::Stopped)]).is_empty());
    }

    #[test]
    fn dropped_sessions_are_forgotten() {
        let mut tracker = AgentTracker::default();
        tracker.observe(&[snapshot("a", Phase::Running)]);
        tracker.observe(&[]);
        assert!(
            tracker
                .observe(&[snapshot("a", Phase::Awaiting)])
                .is_empty()
        );
    }

    #[test]
    fn toast_texts() {
        let session = snapshot("a", Phase::Running);
        assert_eq!(
            toast(Signal::Permission, &session).body,
            "O Claude Code pede permissão em #42"
        );
        assert_eq!(
            toast(Signal::Completed, &session).body,
            "Review de #42 concluída — 3 achados"
        );
        assert_eq!(toast(Signal::Failed, &session).body, "Review de #42 falhou");
    }

    #[test]
    fn plan_toasts_describe_the_draft_and_open_the_issue() {
        let session = SessionSnapshot {
            kind: SessionKind::Plan,
            ..snapshot("a", Phase::Running)
        };
        let done = toast(Signal::Completed, &session);
        assert_eq!(done.body, "Agent Plan de #42 concluído — rascunho pronto");
        assert_eq!(
            done.target,
            ToastTarget::AgentSession(PrKey::new("github.com", "pr"), SessionKind::Plan)
        );
        assert_eq!(
            toast(Signal::Failed, &session).body,
            "Agent Plan de #42 falhou"
        );
        assert_eq!(
            toast(Signal::Permission, &session).body,
            "O Claude Code pede permissão em #42"
        );
    }

    #[test]
    fn plan_toasts_without_an_issue_number() {
        let session = SessionSnapshot {
            kind: SessionKind::Plan,
            number: None,
            ..snapshot("a", Phase::Running)
        };
        assert_eq!(
            toast(Signal::Completed, &session).body,
            "Agent Plan concluído — rascunho pronto"
        );
        assert_eq!(toast(Signal::Failed, &session).body, "Agent Plan falhou");
    }

    #[test]
    fn toast_texts_without_pr_number_and_with_few_findings() {
        let mut session = snapshot("a", Phase::Running);
        session.number = None;
        session.findings = 1;
        assert_eq!(
            toast(Signal::Permission, &session).body,
            "O Claude Code pede permissão"
        );
        assert_eq!(
            toast(Signal::Completed, &session).body,
            "Review concluída — 1 achado"
        );
        session.findings = 0;
        assert_eq!(
            toast(Signal::Completed, &session).body,
            "Review concluída — nenhum achado"
        );
    }
}

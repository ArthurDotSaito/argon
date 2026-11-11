use argon_store::SessionStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardStatus {
    Running,
    Awaiting,
    Completed,
    Failed,
    Archived,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeAction {
    Continue,
    Retry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionFilter {
    All,
    Awaiting,
    Running,
    Completed,
    Archived,
}

impl SessionFilter {
    pub const ALL: [SessionFilter; 5] = [
        Self::All,
        Self::Awaiting,
        Self::Running,
        Self::Completed,
        Self::Archived,
    ];
}

impl CardStatus {
    pub fn is_active(self) -> bool {
        matches!(self, Self::Running | Self::Awaiting)
    }

    pub fn matches(self, filter: SessionFilter) -> bool {
        match filter {
            SessionFilter::All => true,
            SessionFilter::Awaiting => self == Self::Awaiting,
            SessionFilter::Running => self == Self::Running,
            SessionFilter::Completed => matches!(self, Self::Completed | Self::Failed),
            SessionFilter::Archived => self == Self::Archived,
        }
    }
}

pub fn card_status(status: SessionStatus, live: bool, pending: usize) -> CardStatus {
    match (live, status) {
        (true, SessionStatus::Running | SessionStatus::AwaitingUser) if pending > 0 => {
            CardStatus::Awaiting
        }
        (true, SessionStatus::Running | SessionStatus::AwaitingUser) => CardStatus::Running,
        (false, SessionStatus::Running | SessionStatus::AwaitingUser) => CardStatus::Failed,
        (_, SessionStatus::Completed) => CardStatus::Completed,
        (_, SessionStatus::Failed) => CardStatus::Failed,
        (_, SessionStatus::Archived) => CardStatus::Archived,
    }
}

pub fn resume_action(
    status: CardStatus,
    resumable: bool,
    app_closed: bool,
) -> Option<ResumeAction> {
    match (resumable, status) {
        (false, _) => None,
        (true, CardStatus::Completed) => Some(ResumeAction::Continue),
        (true, CardStatus::Failed) if app_closed => Some(ResumeAction::Continue),
        (true, CardStatus::Failed) => Some(ResumeAction::Retry),
        (true, CardStatus::Running | CardStatus::Awaiting | CardStatus::Archived) => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginHint {
    pub hint: String,
    pub command: String,
}

const LOGIN_PREFIXES: [&str; 2] = ["requer login: ", "o agente precisa de login: "];

pub fn login_hint(error: &str) -> Option<LoginHint> {
    let hint = error.lines().find_map(|line| {
        LOGIN_PREFIXES
            .iter()
            .find_map(|prefix| line.trim().strip_prefix(prefix))
    })?;
    let command = hint
        .split('`')
        .nth(1)
        .filter(|command| !command.trim().is_empty())
        .unwrap_or(hint);
    Some(LoginHint {
        hint: hint.to_owned(),
        command: command.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_sessions_with_pending_permissions_wait_for_you() {
        assert_eq!(
            card_status(SessionStatus::Running, true, 1),
            CardStatus::Awaiting
        );
        assert_eq!(
            card_status(SessionStatus::AwaitingUser, true, 0),
            CardStatus::Running
        );
        assert_eq!(
            card_status(SessionStatus::Running, true, 0),
            CardStatus::Running
        );
        assert_eq!(
            card_status(SessionStatus::Completed, true, 0),
            CardStatus::Completed
        );
    }

    #[test]
    fn dead_sessions_never_look_active() {
        assert_eq!(
            card_status(SessionStatus::Running, false, 0),
            CardStatus::Failed
        );
        assert_eq!(
            card_status(SessionStatus::AwaitingUser, false, 2),
            CardStatus::Failed
        );
        assert_eq!(
            card_status(SessionStatus::Completed, false, 0),
            CardStatus::Completed
        );
        assert_eq!(
            card_status(SessionStatus::Failed, true, 0),
            CardStatus::Failed
        );
    }

    #[test]
    fn filters_split_active_from_finished() {
        let expected = [
            (CardStatus::Awaiting, [true, true, false, false, false]),
            (CardStatus::Running, [true, false, true, false, false]),
            (CardStatus::Completed, [true, false, false, true, false]),
            (CardStatus::Failed, [true, false, false, true, false]),
            (CardStatus::Archived, [true, false, false, false, true]),
        ];
        for (status, matches) in expected {
            for (filter, expected) in SessionFilter::ALL.into_iter().zip(matches) {
                assert_eq!(status.matches(filter), expected, "{status:?} {filter:?}");
            }
        }
        assert_eq!(
            card_status(SessionStatus::Archived, false, 0),
            CardStatus::Archived
        );
    }

    #[test]
    fn resume_is_offered_only_for_finished_resumable_sessions() {
        assert_eq!(
            resume_action(CardStatus::Failed, true, true),
            Some(ResumeAction::Continue)
        );
        assert_eq!(
            resume_action(CardStatus::Failed, true, false),
            Some(ResumeAction::Retry)
        );
        assert_eq!(
            resume_action(CardStatus::Completed, true, false),
            Some(ResumeAction::Continue)
        );
        assert_eq!(resume_action(CardStatus::Running, true, false), None);
        assert_eq!(resume_action(CardStatus::Archived, true, true), None);
        assert_eq!(resume_action(CardStatus::Failed, false, true), None);
    }

    #[test]
    fn login_hint_extracts_the_command() {
        let error = "o agente falhou no turno: 401\nrequer login: Execute `codex login` no \
                     terminal para entrar.\n--- stderr do agente ---\nx";
        assert_eq!(
            login_hint(error),
            Some(LoginHint {
                hint: "Execute `codex login` no terminal para entrar.".into(),
                command: "codex login".into(),
            })
        );
        assert_eq!(
            login_hint("o agente precisa de login: rode o login").map(|hint| hint.command),
            Some("rode o login".into())
        );
        assert_eq!(login_hint("falhou sem motivo"), None);
    }
}

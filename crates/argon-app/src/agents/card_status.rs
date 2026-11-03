use argon_store::SessionStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardStatus {
    Running,
    Awaiting,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionFilter {
    All,
    Active,
    Completed,
}

impl CardStatus {
    pub fn is_active(self) -> bool {
        matches!(self, Self::Running | Self::Awaiting)
    }

    pub fn matches(self, filter: SessionFilter) -> bool {
        match filter {
            SessionFilter::All => true,
            SessionFilter::Active => self.is_active(),
            SessionFilter::Completed => !self.is_active(),
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
        assert!(CardStatus::Awaiting.matches(SessionFilter::Active));
        assert!(!CardStatus::Failed.matches(SessionFilter::Active));
        assert!(CardStatus::Failed.matches(SessionFilter::Completed));
        assert!(CardStatus::Running.matches(SessionFilter::All));
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

use argon_core::AgentKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LoginState {
    LoggedIn,
    LoggedOut,
    Unknown,
}

pub(super) fn login_probe(kind: AgentKind) -> Option<&'static [&'static str]> {
    match kind {
        AgentKind::ClaudeCode => Some(&["auth", "status"]),
        AgentKind::Codex => Some(&["login", "status"]),
        AgentKind::OpenCode | AgentKind::CommandCode | AgentKind::Antigravity => None,
    }
}

pub(super) fn parse_login(kind: AgentKind, success: bool, output: &str) -> LoginState {
    match kind {
        AgentKind::ClaudeCode => parse_claude(output),
        AgentKind::Codex => parse_codex(success, output),
        AgentKind::OpenCode | AgentKind::CommandCode | AgentKind::Antigravity => {
            LoginState::Unknown
        }
    }
}

fn parse_claude(output: &str) -> LoginState {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(output.trim()) else {
        return LoginState::Unknown;
    };
    match value.get("loggedIn").and_then(serde_json::Value::as_bool) {
        Some(true) => LoginState::LoggedIn,
        Some(false) => LoginState::LoggedOut,
        None => LoginState::Unknown,
    }
}

fn parse_codex(success: bool, output: &str) -> LoginState {
    if output.to_lowercase().contains("not logged in") {
        LoginState::LoggedOut
    } else if success {
        LoginState::LoggedIn
    } else {
        LoginState::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_reads_logged_in_flag() {
        let logged_in = "{\n  \"loggedIn\": true,\n  \"authMethod\": \"claude.ai\"\n}";
        let logged_out = "{\"loggedIn\": false}";
        assert_eq!(
            parse_login(AgentKind::ClaudeCode, true, logged_in),
            LoginState::LoggedIn
        );
        assert_eq!(
            parse_login(AgentKind::ClaudeCode, false, logged_out),
            LoginState::LoggedOut
        );
        assert_eq!(
            parse_login(AgentKind::ClaudeCode, true, "not json"),
            LoginState::Unknown
        );
        assert_eq!(
            parse_login(AgentKind::ClaudeCode, true, "{}"),
            LoginState::Unknown
        );
    }

    #[test]
    fn codex_reads_status_text_and_exit_code() {
        assert_eq!(
            parse_login(AgentKind::Codex, false, "Not logged in\n"),
            LoginState::LoggedOut
        );
        assert_eq!(
            parse_login(AgentKind::Codex, true, "Logged in using ChatGPT\n"),
            LoginState::LoggedIn
        );
        assert_eq!(
            parse_login(AgentKind::Codex, false, "unexpected failure"),
            LoginState::Unknown
        );
    }

    #[test]
    fn agents_without_a_reliable_status_command_are_never_probed() {
        assert!(login_probe(AgentKind::CommandCode).is_none());
        assert!(login_probe(AgentKind::OpenCode).is_none());
        assert!(login_probe(AgentKind::Antigravity).is_none());
        assert_eq!(
            parse_login(AgentKind::CommandCode, true, "Authentication verified"),
            LoginState::Unknown
        );
    }
}

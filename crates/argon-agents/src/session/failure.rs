use super::SessionError;
use super::driver::Shared;
use crate::Transport;
use crate::defaults::agent_defaults;
use crate::registry::{AgentSpec, login_hint};

pub(crate) const STDERR_LINES: usize = 20;
const LOGIN_MARKERS: [&str; 6] = [
    "authentication required",
    "authentication failed",
    "401",
    "unauthorized",
    "invalid 'authorization'",
    "run /login",
];

pub(super) fn needs_login(text: &str) -> bool {
    let lower = text.to_lowercase();
    LOGIN_MARKERS.iter().any(|marker| lower.contains(marker))
}

pub(super) fn hint_for(spec: &AgentSpec) -> String {
    spec.requires_login_hint
        .clone()
        .unwrap_or_else(|| login_hint(agent_defaults(spec.kind)))
}

pub(super) fn transport_error(spec: &AgentSpec, text: String) -> SessionError {
    match spec.transport {
        Transport::Acp => SessionError::Acp(text),
        _ => SessionError::Agent(text),
    }
}

pub(super) fn classify(spec: &AgentSpec, text: String) -> SessionError {
    if needs_login(&text) {
        SessionError::NeedsLogin {
            hint: hint_for(spec),
        }
    } else {
        transport_error(spec, text)
    }
}

pub(super) fn append_stderr(message: String, tail: &[String]) -> String {
    if tail.is_empty() {
        message
    } else {
        format!("{message}\n--- stderr do agente ---\n{}", tail.join("\n"))
    }
}

pub(crate) fn with_stderr(message: String, shared: &Shared) -> String {
    append_stderr(message, &shared.stderr_tail(STDERR_LINES))
}

pub(crate) fn explain_failure(spec: &AgentSpec, mut message: String, tail: &[String]) -> String {
    if needs_login(&message) || tail.iter().any(|line| needs_login(line)) {
        message.push_str(&format!("\nrequer login: {}", hint_for(spec)));
    }
    append_stderr(message, tail)
}

pub(super) fn turn_failure(spec: &AgentSpec, tail: &[String], error: &str) -> String {
    explain_failure(spec, format!("o agente falhou no turno: {error}"), tail)
}

#[cfg(test)]
mod tests {
    use argon_core::{AgentKind, Config};

    use super::*;
    use crate::registry::agent_spec;

    #[test]
    fn auth_errors_become_needs_login_with_agent_hint() {
        let codex = agent_spec(AgentKind::Codex, &Config::default());
        for text in [
            "Internal error: Authentication required",
            "POST /alpha/generate → 401 error: Invalid 'Authorization' header or token.",
        ] {
            let SessionError::NeedsLogin { hint } = classify(&codex, text.to_owned()) else {
                panic!("{text}");
            };
            assert!(hint.contains("codex login"), "{hint}");
        }
        let command_code = agent_spec(AgentKind::CommandCode, &Config::default());
        assert!(matches!(
            classify(&command_code, "401".into()),
            SessionError::NeedsLogin { hint } if hint.contains("command-code login")
        ));
        let opencode = agent_spec(AgentKind::OpenCode, &Config::default());
        assert!(matches!(
            classify(&opencode, "Authentication required: provider authentication required".into()),
            SessionError::NeedsLogin { hint } if hint.contains("opencode auth login")
        ));
        assert!(matches!(
            classify(&command_code, "boom".into()),
            SessionError::Acp(text) if text == "boom"
        ));
        assert!(matches!(
            classify(&opencode, "boom".into()),
            SessionError::Agent(text) if text == "boom"
        ));
    }

    #[test]
    fn native_transports_never_report_acp_errors() {
        let mut spec = agent_spec(AgentKind::OpenCode, &Config::default());
        for transport in [
            Transport::ClaudeStream,
            Transport::CodexAppServer,
            Transport::OpencodeServe,
        ] {
            spec.transport = transport;
            assert!(matches!(
                classify(&spec, "boom".into()),
                SessionError::Agent(text) if text == "boom"
            ));
            assert!(matches!(
                classify(&spec, "401".into()),
                SessionError::NeedsLogin { .. }
            ));
        }
        spec.transport = Transport::Acp;
        assert!(matches!(
            transport_error(&spec, "boom".into()),
            SessionError::Acp(_)
        ));
    }

    #[test]
    fn turn_failure_includes_stderr_and_login_hint() {
        let spec = agent_spec(AgentKind::CommandCode, &Config::default());
        let tail = vec!["[acp] request failed: 401".to_owned()];
        let message = turn_failure(&spec, &tail, "Internal error");
        assert!(message.contains("requer login"), "{message}");
        assert!(message.contains("--- stderr do agente ---\n[acp] request failed: 401"));
        assert_eq!(
            turn_failure(&spec, &[], "boom"),
            "o agente falhou no turno: boom"
        );
    }

    #[test]
    fn claude_login_prompt_gets_the_claude_hint() {
        let spec = agent_spec(AgentKind::ClaudeCode, &Config::default());
        let message = explain_failure(&spec, "Invalid API key · Please run /login".into(), &[]);
        assert_eq!(
            message,
            "Invalid API key · Please run /login
requer login: Execute `claude auth login` no terminal para entrar."
        );
        assert_eq!(explain_failure(&spec, "boom".into(), &[]), "boom");
    }
}

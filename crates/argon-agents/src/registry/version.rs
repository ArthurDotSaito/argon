use argon_core::AgentKind;

use crate::defaults::agent_defaults;

pub(super) fn parse_version(output: &str) -> Option<String> {
    output
        .lines()
        .find_map(|line| line.split_whitespace().find_map(version_token))
}

pub fn outdated_version(kind: AgentKind, installed: &str) -> Option<String> {
    let tested = parse_version(agent_defaults(kind).tested_version)?;
    (release(installed)? < release(&tested)?).then_some(tested)
}

fn release(version: &str) -> Option<Vec<u64>> {
    version
        .split(['-', '+'])
        .next()?
        .split('.')
        .map(|part| part.parse().ok())
        .collect()
}

fn version_token(token: &str) -> Option<String> {
    let trimmed = token
        .trim_matches(|c: char| matches!(c, '(' | ')' | ',' | ';'))
        .trim_end_matches('.');
    let candidate = trimmed.strip_prefix('v').unwrap_or(trimmed);
    let starts_with_digit = candidate.chars().next()?.is_ascii_digit();
    let dotted = candidate.contains('.');
    let valid = candidate
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'));
    (starts_with_digit && dotted && valid).then(|| candidate.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_cli_outputs() {
        let fixtures = [
            ("2.1.316 (Claude Code)\n", "2.1.316"),
            ("codex-cli 0.160.0\n", "0.160.0"),
            ("opencode v2.2.5\n", "2.2.5"),
            ("1.88.2\n", "1.88.2"),
            ("agy 1.4.7\r\n", "1.4.7"),
            ("\n  Command Code v1.88.2  \n", "1.88.2"),
            ("tool 1.0.0-beta.1+build5", "1.0.0-beta.1+build5"),
            ("version 3.2.1.", "3.2.1"),
        ];
        for (output, expected) in fixtures {
            assert_eq!(
                parse_version(output).as_deref(),
                Some(expected),
                "{output:?}"
            );
        }
    }

    #[test]
    fn older_installs_report_the_tested_version() {
        assert_eq!(
            outdated_version(AgentKind::CommandCode, "1.79.1").as_deref(),
            Some("1.88.2")
        );
        assert_eq!(
            outdated_version(AgentKind::ClaudeCode, "2.1.9").as_deref(),
            Some("2.1.316")
        );
        for (kind, installed) in [
            (AgentKind::CommandCode, "1.88.2"),
            (AgentKind::CommandCode, "1.100.0"),
            (AgentKind::ClaudeCode, "2.1.316"),
            (AgentKind::OpenCode, "2.2.5"),
            (AgentKind::Codex, "0.157.0"),
            (AgentKind::CommandCode, "unknown"),
        ] {
            assert_eq!(
                outdated_version(kind, installed),
                None,
                "{kind:?} {installed}"
            );
        }
    }

    #[test]
    fn ignores_outputs_without_a_version() {
        for output in [
            "",
            "   \n",
            "command not found",
            "usage: tool [options]",
            "12",
        ] {
            assert_eq!(parse_version(output), None, "{output:?}");
        }
    }
}

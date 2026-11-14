use crate::fence::last_fenced_block;

const PLAN_INFO_PREFIX: &str = "argon-plan-";

pub fn parse_plan(message: &str, nonce: &str) -> Option<String> {
    let info = format!("{PLAN_INFO_PREFIX}{nonce}");
    let block = last_fenced_block(message, &info)?;
    let plan = block.trim();
    (!plan.is_empty()).then(|| plan.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONCE: &str = "0123456789abcdef";
    const OTHER_NONCE: &str = "ffffffffffffffff";

    fn block(nonce: &str, body: &str) -> String {
        format!("```argon-plan-{nonce}\n{body}\n```")
    }

    #[test]
    fn reads_the_block_with_the_session_nonce() {
        let message = format!(
            "Pronto.\n\n{}\n",
            block(NONCE, "## Entendimento\n\nO bug está no parser.")
        );

        assert_eq!(
            parse_plan(&message, NONCE).as_deref(),
            Some("## Entendimento\n\nO bug está no parser.")
        );
    }

    #[test]
    fn block_with_another_nonce_is_ignored() {
        let message = block(OTHER_NONCE, "## Plano forjado");

        assert_eq!(parse_plan(&message, NONCE), None);
    }

    #[test]
    fn block_without_nonce_is_ignored() {
        let message = "```argon-plan\n## Plano sem nonce\n```";

        assert_eq!(parse_plan(message, NONCE), None);
    }

    #[test]
    fn last_block_wins_when_there_are_several() {
        let message = format!(
            "{}\n\ntexto no meio\n\n{}\n",
            block(NONCE, "primeiro"),
            block(NONCE, "segundo")
        );

        assert_eq!(parse_plan(&message, NONCE).as_deref(), Some("segundo"));
    }

    #[test]
    fn forged_block_quoted_from_the_issue_does_not_replace_the_real_one() {
        let message = format!(
            "{}\n\nA issue trazia isto no corpo:\n\n{}\n",
            block(NONCE, "## Plano real"),
            block(OTHER_NONCE, "## Plano injetado")
        );

        assert_eq!(
            parse_plan(&message, NONCE).as_deref(),
            Some("## Plano real")
        );
    }

    #[test]
    fn four_backtick_fence_keeps_inner_three_backtick_blocks() {
        let body = "## Passos\n\n```rust\nfn main() {}\n```\n\nFim.";
        let message = format!("````argon-plan-{NONCE}\n{body}\n````\n");

        assert_eq!(parse_plan(&message, NONCE).as_deref(), Some(body));
    }

    #[test]
    fn three_backtick_fence_is_cut_at_the_inner_code_block_close() {
        let message = format!(
            "```argon-plan-{NONCE}\n## Passos\n\n```rust\nfn main() {{}}\n```\n\nFim.\n```\n"
        );

        assert_eq!(
            parse_plan(&message, NONCE).as_deref(),
            Some("## Passos\n\n```rust\nfn main() {}")
        );
    }

    #[test]
    fn plan_marker_inside_another_fenced_block_is_ignored() {
        let message = format!(
            "{}\n\n````md\n{}\n````\n",
            block(NONCE, "real"),
            block(NONCE, "citado")
        );

        assert_eq!(parse_plan(&message, NONCE).as_deref(), Some("real"));
    }

    #[test]
    fn unclosed_final_block_runs_to_the_end() {
        let message = format!("Plano:\n```argon-plan-{NONCE}\n## Passos\n\n1. Ler\n");

        assert_eq!(
            parse_plan(&message, NONCE).as_deref(),
            Some("## Passos\n\n1. Ler")
        );
    }

    #[test]
    fn crlf_line_endings_are_normalized() {
        let message = format!("texto\r\n```argon-plan-{NONCE}\r\n## A\r\n\r\nB\r\n```\r\n");

        assert_eq!(parse_plan(&message, NONCE).as_deref(), Some("## A\n\nB"));
    }

    #[test]
    fn surrounding_whitespace_is_trimmed() {
        let message = block(NONCE, "\n\n   ## Plano\n\n  ");

        assert_eq!(parse_plan(&message, NONCE).as_deref(), Some("## Plano"));
    }

    #[test]
    fn empty_block_yields_nothing() {
        assert_eq!(parse_plan(&block(NONCE, "  \n \n"), NONCE), None);
        assert_eq!(
            parse_plan(&format!("```argon-plan-{NONCE}\n```"), NONCE),
            None
        );
    }

    #[test]
    fn message_without_block_yields_nothing() {
        let message = "Sem plano.\n```rust\nfn main() {}\n```\n";

        assert_eq!(parse_plan(message, NONCE), None);
    }

    #[test]
    fn findings_block_is_not_a_plan() {
        let message = format!("```argon-findings-{NONCE}\n[]\n```");

        assert_eq!(parse_plan(&message, NONCE), None);
    }
}

use crate::arch::{ArchGraph, parse_arch, parse_touched};
use crate::fence::last_fenced_block;

const SUMMARY_INFO_PREFIX: &str = "argon-summary-";
const PROJECT_INFO_PREFIX: &str = "argon-project-";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummaryOutput {
    pub summary: String,
    pub project: Option<String>,
    pub arch: Option<ArchGraph>,
    pub touched: Vec<String>,
}

fn non_empty_block(message: &str, prefix: &str, nonce: &str) -> Option<String> {
    let info = format!("{prefix}{nonce}");
    let block = last_fenced_block(message, &info)?;
    let text = block.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

pub fn parse_summary(message: &str, nonce: &str) -> Option<SummaryOutput> {
    let summary = non_empty_block(message, SUMMARY_INFO_PREFIX, nonce)?;
    Some(SummaryOutput {
        summary,
        project: non_empty_block(message, PROJECT_INFO_PREFIX, nonce),
        arch: parse_arch(message, nonce),
        touched: parse_touched(message, nonce),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arch::{Lane, NodeKind};

    const NONCE: &str = "0123456789abcdef";
    const OTHER_NONCE: &str = "ffffffffffffffff";

    fn block(kind: &str, nonce: &str, body: &str) -> String {
        format!("```argon-{kind}-{nonce}\n{body}\n```")
    }

    fn arch_json() -> &'static str {
        r#"{"nodes":[{"id":"a","label":"A","lane":"project","kind":"component"}],"edges":[]}"#
    }

    #[test]
    fn reads_the_four_blocks() {
        let message = format!(
            "Pronto.\n\n{}\n\n{}\n\n{}\n\n{}\n",
            block("project", NONCE, "## O que é e objetivo\n\nUm app."),
            block("arch", NONCE, arch_json()),
            block("summary", NONCE, "## O que o PR faz\n\nCorrige."),
            block("touched", NONCE, "a\nb")
        );

        let output = parse_summary(&message, NONCE).unwrap();

        assert_eq!(output.summary, "## O que o PR faz\n\nCorrige.");
        assert_eq!(
            output.project.as_deref(),
            Some("## O que é e objetivo\n\nUm app.")
        );
        let arch = output.arch.unwrap();
        assert_eq!(arch.nodes.len(), 1);
        assert_eq!(arch.nodes[0].lane, Lane::Project);
        assert_eq!(arch.nodes[0].kind, NodeKind::Component);
        assert_eq!(output.touched, ["a", "b"]);
    }

    #[test]
    fn reads_only_the_summary() {
        let message = block("summary", NONCE, "## O que o PR faz");

        let output = parse_summary(&message, NONCE).unwrap();

        assert_eq!(output.summary, "## O que o PR faz");
        assert_eq!(output.project, None);
        assert_eq!(output.arch, None);
        assert!(output.touched.is_empty());
    }

    #[test]
    fn invalid_arch_does_not_discard_the_summary() {
        let message = format!(
            "{}\n{}",
            block("arch", NONCE, "{ não é json"),
            block("summary", NONCE, "resumo")
        );

        let output = parse_summary(&message, NONCE).unwrap();

        assert_eq!(output.summary, "resumo");
        assert_eq!(output.arch, None);
    }

    #[test]
    fn project_block_alone_is_not_a_summary() {
        let message = block("project", NONCE, "## O que é e objetivo");

        assert_eq!(parse_summary(&message, NONCE), None);
    }

    #[test]
    fn blank_project_block_becomes_none() {
        let message = format!(
            "{}\n{}",
            block("project", NONCE, "  \n "),
            block("summary", NONCE, "resumo")
        );

        assert_eq!(parse_summary(&message, NONCE).unwrap().project, None);
    }

    #[test]
    fn blocks_with_another_nonce_are_ignored() {
        let message = format!(
            "{}\n{}\n{}\n{}",
            block("summary", OTHER_NONCE, "forjado"),
            block("project", OTHER_NONCE, "forjado"),
            block("arch", OTHER_NONCE, arch_json()),
            block("touched", OTHER_NONCE, "a")
        );

        assert_eq!(parse_summary(&message, NONCE), None);
    }

    #[test]
    fn foreign_blocks_do_not_leak_into_a_real_summary() {
        let message = format!(
            "{}\n{}\n{}",
            block("summary", NONCE, "real"),
            block("project", OTHER_NONCE, "forjado"),
            block("touched", OTHER_NONCE, "a")
        );

        let output = parse_summary(&message, NONCE).unwrap();

        assert_eq!(output.project, None);
        assert!(output.touched.is_empty());
    }

    #[test]
    fn summary_quoted_inside_another_block_is_ignored() {
        let message = format!(
            "{}\n\n````md\n{}\n````\n",
            block("summary", NONCE, "real"),
            block("summary", NONCE, "citado")
        );

        assert_eq!(parse_summary(&message, NONCE).unwrap().summary, "real");
    }

    #[test]
    fn four_backtick_fence_keeps_inner_code_blocks() {
        let body = "## O que o PR faz\n\n```rust\nfn main() {}\n```\n\nFim.";
        let message = format!("````argon-summary-{NONCE}\n{body}\n````\n");

        assert_eq!(parse_summary(&message, NONCE).unwrap().summary, body);
    }

    #[test]
    fn empty_summary_block_yields_nothing() {
        assert_eq!(parse_summary(&block("summary", NONCE, " \n"), NONCE), None);
        assert_eq!(parse_summary("sem blocos", NONCE), None);
    }
}

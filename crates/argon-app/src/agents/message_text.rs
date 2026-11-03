const FINDINGS_INFO: &str = "argon-findings";

fn fence(line: &str) -> Option<(usize, &str)> {
    let trimmed = line.trim_start();
    let ticks = trimmed.chars().take_while(|ch| *ch == '`').count();
    (ticks >= 3).then(|| (ticks, trimmed[ticks..].trim()))
}

pub fn hide_findings_block(text: &str) -> String {
    let mut out = Vec::new();
    let mut hidden: Option<usize> = None;
    for line in text.split('\n') {
        match (hidden, fence(line)) {
            (Some(open), Some((ticks, info))) if ticks >= open && info.is_empty() => hidden = None,
            (Some(_), _) => {}
            (None, Some((ticks, info))) if info.starts_with(FINDINGS_INFO) => hidden = Some(ticks),
            (None, _) => out.push(line),
        }
    }
    out.join("\n").trim_end().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn findings_block_is_hidden_and_the_rest_kept() {
        let text =
            "Resumo.\n\n```rust\nfn a() {}\n```\n\n```argon-findings-abc\n[{\"x\":1}]\n```\nFim.";
        assert_eq!(
            hide_findings_block(text),
            "Resumo.\n\n```rust\nfn a() {}\n```\n\nFim."
        );
    }

    #[test]
    fn unterminated_block_hides_the_streaming_tail() {
        let text = "Achei 2 pontos.\n````argon-findings-n\n[{\"severity\":";
        assert_eq!(hide_findings_block(text), "Achei 2 pontos.");
    }

    #[test]
    fn plain_text_is_untouched() {
        assert_eq!(hide_findings_block("sem blocos"), "sem blocos");
    }
}

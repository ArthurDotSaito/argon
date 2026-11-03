use std::collections::BTreeMap;

use argon_agents::SavedFinding;
use argon_core::{DraftComment, DraftSource, Finding, PrKey, Severity};
use argon_store::{FindingState, StoredFinding};

const SUGGESTION_FENCE: &str = "```";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindingEntry {
    pub finding: Finding,
    pub state: FindingState,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SeverityCounts {
    pub high: usize,
    pub medium: usize,
    pub low: usize,
}

impl SeverityCounts {
    pub fn tally<'a>(findings: impl IntoIterator<Item = (&'a Finding, FindingState)>) -> Self {
        let mut counts = Self::default();
        for (finding, state) in findings {
            if state == FindingState::Ignored {
                continue;
            }
            match finding.severity {
                Severity::High => counts.high += 1,
                Severity::Medium => counts.medium += 1,
                Severity::Low => counts.low += 1,
            }
        }
        counts
    }

    pub fn total(&self) -> usize {
        self.high + self.medium + self.low
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FindingSet {
    entries: BTreeMap<i64, FindingEntry>,
}

impl FindingSet {
    pub fn insert_saved(&mut self, saved: &[SavedFinding]) {
        for item in saved {
            self.entries.entry(item.id).or_insert_with(|| FindingEntry {
                finding: item.finding.clone(),
                state: FindingState::New,
            });
        }
    }

    pub fn merge_stored(&mut self, stored: Vec<StoredFinding>) {
        for item in stored {
            self.entries.insert(
                item.id,
                FindingEntry {
                    finding: item.finding,
                    state: item.state,
                },
            );
        }
    }

    pub fn get(&self, id: i64) -> Option<&FindingEntry> {
        self.entries.get(&id)
    }

    pub fn set_state(&mut self, id: i64, state: FindingState) {
        if let Some(entry) = self.entries.get_mut(&id) {
            entry.state = state;
        }
    }

    pub fn counts(&self) -> SeverityCounts {
        SeverityCounts::tally(
            self.entries
                .values()
                .map(|entry| (&entry.finding, entry.state)),
        )
    }
}

fn fence_for(code: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for ch in code.chars() {
        run = if ch == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    if longest < SUGGESTION_FENCE.len() {
        SUGGESTION_FENCE.to_owned()
    } else {
        "`".repeat(longest + 1)
    }
}

pub fn draft_body(finding: &Finding) -> String {
    let mut parts = vec![format!("**{}**", finding.title.trim())];
    let body = finding.body.trim();
    if !body.is_empty() {
        parts.push(body.to_owned());
    }
    if let Some(code) = finding
        .suggestion
        .as_deref()
        .map(|code| code.trim_end_matches(['\n', '\r']))
        .filter(|code| !code.trim().is_empty())
    {
        let fence = fence_for(code);
        parts.push(format!("{fence}suggestion\n{code}\n{fence}"));
    }
    parts.join("\n\n")
}

pub fn draft_for(
    finding: &Finding,
    pr: PrKey,
    session_id: &str,
    head_oid: Option<String>,
) -> DraftComment {
    DraftComment {
        id: 0,
        pr,
        path: finding.file.clone(),
        line: finding.line,
        side: finding.side,
        start_line: None,
        start_side: None,
        body: draft_body(finding),
        source: DraftSource::Agent,
        session_id: Some(session_id.to_owned()),
        head_oid,
    }
}

#[cfg(test)]
mod tests {
    use argon_core::Side;

    use super::*;

    fn finding(severity: Severity, suggestion: Option<&str>) -> Finding {
        Finding {
            severity,
            file: "src/slot.ts".into(),
            line: 60,
            side: Side::Right,
            title: " Usa UTC ".into(),
            body: "Compare no fuso da clínica.\n".into(),
            suggestion: suggestion.map(str::to_owned),
        }
    }

    #[test]
    fn body_joins_title_body_and_suggestion() {
        let body = draft_body(&finding(Severity::High, Some("const h = local(d);\n")));
        assert_eq!(
            body,
            "**Usa UTC**\n\nCompare no fuso da clínica.\n\n```suggestion\nconst h = local(d);\n```"
        );
    }

    #[test]
    fn body_skips_empty_parts() {
        let mut item = finding(Severity::Low, Some("  \n"));
        item.body = "  ".into();
        assert_eq!(draft_body(&item), "**Usa UTC**");
    }

    #[test]
    fn suggestion_fence_outgrows_backticks_in_code() {
        let body = draft_body(&finding(Severity::Low, Some("let s = \"```\";")));
        assert!(body.ends_with("````suggestion\nlet s = \"```\";\n````"));
    }

    #[test]
    fn draft_points_at_the_finding_line() {
        let pr = PrKey::new("github.com", "PR_1");
        let draft = draft_for(
            &finding(Severity::Medium, None),
            pr.clone(),
            "sess",
            Some("abc".into()),
        );
        assert_eq!(draft.pr, pr);
        assert_eq!((draft.path.as_str(), draft.line), ("src/slot.ts", 60));
        assert_eq!(draft.side, Side::Right);
        assert_eq!(draft.source, DraftSource::Agent);
        assert_eq!(draft.session_id.as_deref(), Some("sess"));
        assert_eq!(draft.head_oid.as_deref(), Some("abc"));
    }

    #[test]
    fn counts_by_severity_ignore_dismissed_findings() {
        let mut set = FindingSet::default();
        set.insert_saved(&[
            SavedFinding {
                id: 1,
                finding: finding(Severity::High, None),
            },
            SavedFinding {
                id: 2,
                finding: finding(Severity::Medium, None),
            },
            SavedFinding {
                id: 3,
                finding: finding(Severity::Medium, None),
            },
            SavedFinding {
                id: 4,
                finding: finding(Severity::Low, None),
            },
        ]);
        set.set_state(3, FindingState::Ignored);
        set.set_state(1, FindingState::Added);
        assert_eq!(
            set.counts(),
            SeverityCounts {
                high: 1,
                medium: 1,
                low: 1
            }
        );
        assert_eq!(set.counts().total(), 3);
    }

    #[test]
    fn stored_state_wins_over_fresh_event() {
        let mut set = FindingSet::default();
        set.merge_stored(vec![StoredFinding {
            id: 9,
            session_id: "s".into(),
            finding: finding(Severity::High, None),
            state: FindingState::Added,
        }]);
        set.insert_saved(&[SavedFinding {
            id: 9,
            finding: finding(Severity::High, None),
        }]);
        assert_eq!(
            set.get(9).map(|entry| entry.state),
            Some(FindingState::Added)
        );
    }
}

use std::collections::HashSet;

use argon_core::{PrKey, Reason};

use super::toast::{Channel, ToastSpec, ToastTarget};

const AGGREGATE_ABOVE: usize = 3;
const MAX_TITLE_CHARS: usize = 80;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeedsYouEntry {
    pub key: PrKey,
    pub number: u64,
    pub title: String,
    pub reason: String,
}

impl NeedsYouEntry {
    pub fn new(key: PrKey, number: u64, title: &str, reason: &Reason) -> Self {
        Self {
            key,
            number,
            title: title.to_string(),
            reason: reason_text(reason),
        }
    }
}

#[derive(Default)]
pub struct NeedsYouTracker {
    known: Option<HashSet<PrKey>>,
}

impl NeedsYouTracker {
    pub fn observe(&mut self, current: Vec<NeedsYouEntry>) -> Vec<NeedsYouEntry> {
        let keys = current.iter().map(|entry| entry.key.clone()).collect();
        let Some(previous) = self.known.replace(keys) else {
            return Vec::new();
        };
        current
            .into_iter()
            .filter(|entry| !previous.contains(&entry.key))
            .collect()
    }
}

pub fn toasts(entered: Vec<NeedsYouEntry>) -> Vec<ToastSpec> {
    if entered.len() > AGGREGATE_ABOVE {
        return vec![ToastSpec {
            channel: Channel::NeedsYou,
            body: format!("{} PRs precisam de você", entered.len()),
            target: ToastTarget::NeedsYou,
        }];
    }
    entered
        .into_iter()
        .map(|entry| ToastSpec {
            channel: Channel::NeedsYou,
            body: format!(
                "#{} {} — {}",
                entry.number,
                shorten(&entry.title),
                entry.reason
            ),
            target: ToastTarget::Pr(entry.key),
        })
        .collect()
}

fn shorten(title: &str) -> String {
    let mut chars = title.trim().chars();
    let head: String = chars.by_ref().take(MAX_TITLE_CHARS).collect();
    if chars.next().is_none() {
        return head;
    }
    format!("{}…", head.trim_end())
}

fn reason_text(reason: &Reason) -> String {
    match reason {
        Reason::ReviewRequestedBy(login) => format!("{login} pediu sua review"),
        Reason::AssignedBy(_) => "atribuído a você".to_string(),
        Reason::ChangesRequestedBy(login) => format!("{login} pediu mudanças"),
        Reason::WaitingOn(_) => "aguardando review".to_string(),
        Reason::ReadyToMerge => "pronto para merge".to_string(),
        Reason::ApprovedByYouWaitingAuthor => "você aprovou, aguardando o autor".to_string(),
        Reason::CiFailed => "CI falhou".to_string(),
        Reason::Conflict => "conflito com a base".to_string(),
        Reason::IssueAssigned => "atribuída a você".to_string(),
        Reason::Mentioned => "te mencionaram".to_string(),
        Reason::IssueAuthored => "você abriu".to_string(),
        Reason::Participating => "você comentou".to_string(),
        Reason::NewInOrg(org) => format!("nova em {org}, sem responsável"),
        Reason::ClosedBy(number) => format!("fechada por #{number}"),
        Reason::Tracking => "atualizado".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, number: u64, title: &str) -> NeedsYouEntry {
        NeedsYouEntry::new(
            PrKey::new("github.com", id),
            number,
            title,
            &Reason::ReviewRequestedBy("ana".to_string()),
        )
    }

    #[test]
    fn first_observation_only_sets_the_baseline() {
        let mut tracker = NeedsYouTracker::default();
        assert!(tracker.observe(vec![entry("a", 1, "x")]).is_empty());
    }

    #[test]
    fn only_entries_that_entered_are_reported() {
        let mut tracker = NeedsYouTracker::default();
        tracker.observe(vec![entry("a", 1, "x"), entry("b", 2, "y")]);
        let entered = tracker.observe(vec![entry("b", 2, "y"), entry("c", 3, "z")]);
        assert_eq!(entered, vec![entry("c", 3, "z")]);
    }

    #[test]
    fn an_entry_that_left_and_returned_is_reported_again() {
        let mut tracker = NeedsYouTracker::default();
        tracker.observe(vec![entry("a", 1, "x")]);
        assert!(tracker.observe(vec![]).is_empty());
        assert_eq!(
            tracker.observe(vec![entry("a", 1, "x")]),
            vec![entry("a", 1, "x")]
        );
    }

    #[test]
    fn up_to_three_entries_get_one_toast_each() {
        let specs = toasts(vec![entry("a", 1, "Fix login"), entry("b", 2, "Add cache")]);
        let bodies: Vec<_> = specs.iter().map(|spec| spec.body.as_str()).collect();
        assert_eq!(
            bodies,
            [
                "#1 Fix login — ana pediu sua review",
                "#2 Add cache — ana pediu sua review"
            ]
        );
        assert_eq!(
            specs[0].target,
            ToastTarget::Pr(PrKey::new("github.com", "a"))
        );
    }

    #[test]
    fn more_than_three_entries_collapse_into_one_toast() {
        let entered = (1..=4).map(|n| entry(&n.to_string(), n, "t")).collect();
        let specs = toasts(entered);
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].body, "4 PRs precisam de você");
        assert_eq!(specs[0].target, ToastTarget::NeedsYou);
    }

    #[test]
    fn long_titles_are_shortened() {
        let title = "x".repeat(200);
        let specs = toasts(vec![entry("a", 7, &title)]);
        assert!(
            specs[0]
                .body
                .starts_with(&format!("#7 {}…", "x".repeat(80)))
        );
    }
}

use argon_store::{PrSummary, ProjectOverview};
use chrono::{DateTime, Local, Utc};

use super::panel_labels as labels;
use crate::project::labels::short_date;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummaryPart {
    pub title: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummaryCardView {
    pub note: Option<String>,
    pub project: Option<SummaryPart>,
    pub pr: SummaryPart,
    pub copy: String,
}

pub fn summary_note(
    project_updated: bool,
    saved_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Option<String> {
    if project_updated {
        return Some(labels::PROJECT_REREAD_NOTE.to_owned());
    }
    let saved = saved_at?.with_timezone(&Local).date_naive();
    let today = now.with_timezone(&Local).date_naive();
    Some(labels::summary_saved_note(&short_date(saved, today)))
}

pub fn summary_copy_text(project: Option<&SummaryPart>, pr: &SummaryPart) -> String {
    project
        .into_iter()
        .chain([pr])
        .map(|part| format!("## {}\n\n{}", part.title, part.body.trim()))
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub fn summary_card_view(
    summary: &PrSummary,
    overview: Option<&ProjectOverview>,
    number: Option<u64>,
    project_updated: bool,
    now: DateTime<Utc>,
) -> SummaryCardView {
    let project = overview.map(|overview| SummaryPart {
        title: labels::summary_project_part(&overview.repo),
        body: overview.body.clone(),
    });
    let pr = SummaryPart {
        title: labels::summary_pr_part(number),
        body: summary.body.clone(),
    };
    SummaryCardView {
        note: summary_note(
            project_updated,
            overview.map(|overview| overview.updated_at),
            now,
        ),
        copy: summary_copy_text(project.as_ref(), &pr),
        project,
        pr,
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn at(day: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, day, 12, 0, 0).unwrap()
    }

    fn summary() -> PrSummary {
        PrSummary {
            id: 3,
            host: "github.com".into(),
            pr_id: "PR_1".into(),
            session_id: "s1".into(),
            body: "O PR muda a agenda.\n".into(),
            head_sha: "abc123".into(),
            touched: Vec::new(),
            created_at: at(19),
        }
    }

    fn overview() -> ProjectOverview {
        ProjectOverview {
            host: "github.com".into(),
            repo: "acme/agenda-api".into(),
            body: "Serviço de agenda.".into(),
            diagram: None,
            source_sha: "abc123".into(),
            source_pr: 200,
            session_id: "s0".into(),
            updated_at: at(10),
        }
    }

    #[test]
    fn the_note_says_the_project_was_reread_in_this_session() {
        assert_eq!(
            summary_note(true, Some(at(10)), at(19)).as_deref(),
            Some("projeto relido nesta sessão")
        );
        assert_eq!(
            summary_note(true, None, at(19)).as_deref(),
            Some("projeto relido nesta sessão")
        );
    }

    #[test]
    fn the_note_dates_the_saved_overview_otherwise() {
        assert_eq!(
            summary_note(false, Some(at(10)), at(19)).as_deref(),
            Some("visão salva de 10 nov")
        );
        assert_eq!(summary_note(false, None, at(19)), None);
    }

    #[test]
    fn the_card_has_both_parts_and_copies_them_together() {
        let view = summary_card_view(&summary(), Some(&overview()), Some(214), false, at(19));
        assert_eq!(
            view.project,
            Some(SummaryPart {
                title: "O projeto · acme/agenda-api".into(),
                body: "Serviço de agenda.".into(),
            })
        );
        assert_eq!(view.pr.title, "Este PR · #214");
        assert_eq!(
            view.copy,
            "## O projeto · acme/agenda-api\n\nServiço de agenda.\n\n## Este PR · #214\n\nO PR muda a agenda."
        );
    }

    #[test]
    fn without_a_saved_overview_the_card_only_has_the_pr_part() {
        let view = summary_card_view(&summary(), None, Some(214), false, at(19));
        assert_eq!(view.project, None);
        assert_eq!(view.note, None);
        assert_eq!(view.copy, "## Este PR · #214\n\nO PR muda a agenda.");
    }
}

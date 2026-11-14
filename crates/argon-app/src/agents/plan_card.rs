use argon_store::PlanDraft;

use super::panel_labels as labels;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanCardView {
    Draft { new_version: bool },
    Published { url: Option<String>, latest: bool },
}

pub fn plan_card_view(draft: &PlanDraft, latest: bool) -> PlanCardView {
    if draft.published_at.is_some() {
        return PlanCardView::Published {
            url: draft.comment_url.clone(),
            latest,
        };
    }
    PlanCardView::Draft {
        new_version: draft.agent_body.is_some(),
    }
}

pub fn can_publish(view: &PlanCardView, busy: bool, editing: bool) -> bool {
    matches!(view, PlanCardView::Draft { new_version: false }) && !busy && !editing
}

pub fn plan_comment_body(markdown: &str, agent: &str, model: &str, login: &str) -> String {
    format!(
        "{}\n\n{}",
        markdown.trim(),
        labels::plan_footer(agent, model, login)
    )
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;

    fn draft() -> PlanDraft {
        let at = Utc.with_ymd_and_hms(2026, 11, 13, 10, 0, 0).unwrap();
        PlanDraft {
            id: 7,
            host: "github.com".into(),
            issue_id: "I_7".into(),
            session_id: "s1".into(),
            body: "## Passos".into(),
            base_sha: "abc123".into(),
            agent_body: None,
            edited: false,
            created_at: at,
            updated_at: at,
            published_at: None,
            comment_id: None,
            comment_url: None,
        }
    }

    #[test]
    fn open_draft_without_agent_version_is_publishable() {
        let view = plan_card_view(&draft(), true);
        assert_eq!(view, PlanCardView::Draft { new_version: false });
        assert!(can_publish(&view, false, false));
    }

    #[test]
    fn pending_agent_version_must_be_resolved_before_publishing() {
        let mut edited = draft();
        edited.edited = true;
        edited.agent_body = Some("## Outro plano".into());
        let view = plan_card_view(&edited, true);
        assert_eq!(view, PlanCardView::Draft { new_version: true });
        assert!(!can_publish(&view, false, false));
    }

    #[test]
    fn busy_or_editing_blocks_publishing() {
        let view = plan_card_view(&draft(), true);
        assert!(!can_publish(&view, true, false));
        assert!(!can_publish(&view, false, true));
    }

    #[test]
    fn published_draft_links_to_its_comment() {
        let mut published = draft();
        published.published_at = Some(published.updated_at);
        published.comment_id = Some("IC_1".into());
        published.comment_url = Some("https://github.com/acme/api/issues/7#issuecomment-1".into());
        let view = plan_card_view(&published, false);
        assert_eq!(
            view,
            PlanCardView::Published {
                url: Some("https://github.com/acme/api/issues/7#issuecomment-1".into()),
                latest: false,
            }
        );
        assert!(!can_publish(&view, false, false));
    }

    #[test]
    fn published_body_is_the_draft_plus_the_footer() {
        assert_eq!(
            plan_comment_body(
                "\n## Passos\n\n1. Ler\n\n",
                "Claude Code",
                "Opus 5.5",
                "ana"
            ),
            "## Passos\n\n1. Ler\n\n_Plano preparado com Agent Plan (Claude Code · Opus 5.5) e revisado por @ana._"
        );
    }
}

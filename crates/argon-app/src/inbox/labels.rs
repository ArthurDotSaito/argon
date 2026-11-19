use argon_core::{DateBucket, IssueLabel, Reason};
use chrono::{DateTime, Utc};

use super::InboxView;
use super::browse::IssueScope;
use super::filter::KindFilter;
use crate::ui::{self, Tone};

pub const PANEL_TITLE: &str = "Visões";
pub const GROUP_INBOX: &str = "Inbox";
pub const GROUP_AGENTS: &str = "Agentes";
pub const GROUP_PROJECTS: &str = "Projetos";
pub const GROUP_YOUR_PRS: &str = "Seus PRs";
pub const GROUP_ISSUES: &str = "Issues";
pub const GROUP_REVIEWED: &str = "Revisados";
pub const GROUP_ORGS: &str = "Orgs";
pub const GROUP_ACCOUNTS: &str = "Contas";
pub const OPEN_SETTINGS: &str = "Configurações";
pub const ADMIN_BADGE: &str = "admin";
pub const NO_ACCOUNTS: &str = "Nenhuma conta conectada";
pub const NO_ORGS: &str = "Nenhuma org rastreada";
pub const FILTER_PLACEHOLDER: &str = "Filtrar por título, repo, autor, label…";
pub const FILTER_SHORTCUT: &str = "/";
pub const LOADING: &str = "Carregando…";
pub const LOAD_MORE: &str = "Carregar mais";
pub const EMPTY_PR_VIEW: &str = "Nenhum PR nesta visão.";
pub const EMPTY_ISSUE_VIEW: &str = "Nenhuma issue nesta visão.";
pub const EMPTY_MIXED_VIEW: &str = "Nada nesta visão.";
pub const EMPTY_FILTER: &str = "Nada corresponde ao filtro.";
pub const EMPTY_BROWSE: &str = "Nenhuma issue aberta encontrada.";
pub const KIND_ALL: &str = "Tudo";
pub const KIND_PRS: &str = "PRs";
pub const KIND_ISSUES: &str = "Issues";
pub const SCOPE_RECENT: &str = "Recentes";
pub const SCOPE_ALL_OPEN: &str = "Todas abertas";
pub const SCOPE_ALL_OPEN_HINT: &str = "Busca no GitHub agora, sem limite de data";
pub const NEEDS_YOU_SECTION: &str = "Needs you";
pub const PR_FALLBACK_TITLE: &str = "PR";
pub const ISSUE_FALLBACK_TITLE: &str = "Issue";
pub const COLUMN_PR: &str = "Pull request";
pub const COLUMN_ISSUE: &str = "Issue";
pub const COLUMN_TITLE: &str = "Título";
pub const COLUMN_REASON: &str = "Motivo";
pub const COLUMN_CI: &str = "CI";
pub const COLUMN_LABELS: &str = "Labels";
pub const COLUMN_CI_LABELS: &str = "CI / labels";
pub const COLUMN_REVIEW: &str = "Review";
pub const COLUMN_PEOPLE: &str = "Pessoas";
pub const COLUMN_DIFF: &str = "Diff";
pub const COLUMN_LINK: &str = "Vínculo";
pub const COLUMN_DIFF_LINK: &str = "Diff / vínculo";
pub const COLUMN_UPDATED: &str = "Atualizado";
pub const NO_VALUE: &str = "—";

const TAB_TITLE_MAX_CHARS: usize = 16;
const MAX_LABEL_DOTS: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColumnTitles {
    pub subject: &'static str,
    pub signal: &'static str,
    pub people: &'static str,
    pub size: &'static str,
}

pub fn column_titles(kind: KindFilter) -> ColumnTitles {
    match kind {
        KindFilter::Prs => ColumnTitles {
            subject: COLUMN_PR,
            signal: COLUMN_CI,
            people: COLUMN_REVIEW,
            size: COLUMN_DIFF,
        },
        KindFilter::Issues => ColumnTitles {
            subject: COLUMN_ISSUE,
            signal: COLUMN_LABELS,
            people: COLUMN_PEOPLE,
            size: COLUMN_LINK,
        },
        KindFilter::All => ColumnTitles {
            subject: COLUMN_TITLE,
            signal: COLUMN_CI_LABELS,
            people: COLUMN_PEOPLE,
            size: COLUMN_DIFF_LINK,
        },
    }
}

pub fn view_title(view: InboxView) -> &'static str {
    match view {
        InboxView::Inbox => "Inbox",
        InboxView::NeedsYou => "Needs you",
        InboxView::YourPrs => "Your PRs",
        InboxView::WaitingOnOthers => "Waiting on others",
        InboxView::ChangesRequested => "Changes requested",
        InboxView::ReadyToMerge => "Ready to merge",
        InboxView::AlreadyDone => "Already done",
        InboxView::IssuesAssigned => "Atribuídas a você",
        InboxView::IssuesMentioned => "Mencionado",
        InboxView::IssuesAuthored => "Criadas por você",
        InboxView::IssuesParticipating => "Participando",
    }
}

pub fn empty_view(view: InboxView, kind: KindFilter) -> &'static str {
    match view {
        InboxView::Inbox | InboxView::NeedsYou => match kind {
            KindFilter::All => EMPTY_MIXED_VIEW,
            KindFilter::Prs => EMPTY_PR_VIEW,
            KindFilter::Issues => EMPTY_ISSUE_VIEW,
        },
        InboxView::IssuesAssigned
        | InboxView::IssuesMentioned
        | InboxView::IssuesAuthored
        | InboxView::IssuesParticipating => EMPTY_ISSUE_VIEW,
        InboxView::YourPrs
        | InboxView::WaitingOnOthers
        | InboxView::ChangesRequested
        | InboxView::ReadyToMerge
        | InboxView::AlreadyDone => EMPTY_PR_VIEW,
    }
}

pub fn empty_browse(filtered: bool) -> &'static str {
    if filtered { EMPTY_FILTER } else { EMPTY_BROWSE }
}

pub fn scope_menu_label(scope: IssueScope) -> String {
    let name = match scope {
        IssueScope::Recent => SCOPE_RECENT,
        IssueScope::AllOpen => SCOPE_ALL_OPEN,
    };
    format!("· {name}")
}

pub fn scope_recent_hint(days: i64) -> String {
    match days {
        1 => "Atualizadas no último dia".to_string(),
        _ => format!("Atualizadas nos últimos {days} dias"),
    }
}

pub fn browse_count(shown: usize, total: u32) -> String {
    format!("{shown} de {total}")
}

pub fn visible_labels(labels: &[IssueLabel]) -> (&[IssueLabel], Option<String>) {
    let shown = labels.len().min(MAX_LABEL_DOTS);
    let hidden = labels.len() - shown;
    (&labels[..shown], (hidden > 0).then(|| format!("+{hidden}")))
}

pub fn bucket_title(bucket: DateBucket) -> &'static str {
    match bucket {
        DateBucket::Today => "Hoje",
        DateBucket::Yesterday => "Ontem",
        DateBucket::LastSevenDays => "Últimos 7 dias",
        DateBucket::Older => "Mais antigos",
    }
}

pub fn load_failed(error: &str) -> String {
    format!("Falha ao carregar a inbox: {error}")
}

pub fn browse_failed(error: &str) -> String {
    format!("Falha ao buscar as issues abertas: {error}")
}

pub fn action_failed(error: &str) -> String {
    format!("Falha ao gravar no banco local: {error}")
}

pub fn needs_you_status(count: usize) -> String {
    format!("{count} needs you")
}

pub fn pr_tab_title(number: u64, title: &str) -> String {
    let mut chars = title.chars();
    let head: String = chars.by_ref().take(TAB_TITLE_MAX_CHARS).collect();
    if chars.next().is_none() {
        return format!("#{number} {head}");
    }
    format!("#{number} {}…", head.trim_end())
}

pub fn repo_ref(repo: &str, number: u64) -> String {
    format!("{repo} #{number}")
}

pub fn short_repo_ref(repo: &str, number: u64) -> String {
    let name = repo.rsplit_once('/').map_or(repo, |(_, name)| name);
    format!("{name} #{number}")
}

pub fn reason_line(reason: &Reason, now: DateTime<Utc>, updated_at: DateTime<Utc>) -> String {
    match reason {
        Reason::ReviewRequestedBy(login) => format!("{login} pediu sua review"),
        Reason::AssignedBy(_) => "Atribuído a você".to_string(),
        Reason::ChangesRequestedBy(login) => format!("{login} pediu mudanças"),
        Reason::WaitingOn(reviewers) => {
            format!("Esperando review de {}", reviewers.join(", "))
        }
        Reason::ReadyToMerge => "Aprovado e CI verde — pronto para merge".to_string(),
        Reason::ApprovedByYouWaitingAuthor => {
            "Você aprovou — aguardando merge do autor".to_string()
        }
        Reason::CiFailed => "CI falhou".to_string(),
        Reason::Conflict => "Conflito com a base".to_string(),
        Reason::IssueAssigned => "Atribuída a você".to_string(),
        Reason::Mentioned => "Te mencionaram".to_string(),
        Reason::IssueAuthored => "Você abriu".to_string(),
        Reason::Participating => "Você comentou".to_string(),
        Reason::NewInOrg(org) => format!("Nova em {org} · sem responsável"),
        Reason::ClosedBy(number) => format!("Fechada por #{number}"),
        Reason::Tracking => tracking_line(now, updated_at),
    }
}

pub fn reason_tone(reason: &Reason) -> Tone {
    match reason {
        Reason::ReviewRequestedBy(_)
        | Reason::AssignedBy(_)
        | Reason::IssueAssigned
        | Reason::Mentioned => Tone::Action,
        Reason::CiFailed | Reason::Conflict => Tone::Danger,
        Reason::ChangesRequestedBy(_) | Reason::NewInOrg(_) => Tone::Warning,
        Reason::ReadyToMerge | Reason::ClosedBy(_) => Tone::Success,
        Reason::WaitingOn(_)
        | Reason::ApprovedByYouWaitingAuthor
        | Reason::IssueAuthored
        | Reason::Participating
        | Reason::Tracking => Tone::Neutral,
    }
}

fn tracking_line(now: DateTime<Utc>, updated_at: DateTime<Utc>) -> String {
    format!("Acompanhando · atualizado {}", ui::ago(now, updated_at))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn label(name: &str) -> IssueLabel {
        IssueLabel {
            name: name.to_string(),
            color: "d73a4a".to_string(),
        }
    }

    #[test]
    fn issue_views_are_empty_with_the_issue_text() {
        for view in [
            InboxView::IssuesAssigned,
            InboxView::IssuesMentioned,
            InboxView::IssuesAuthored,
            InboxView::IssuesParticipating,
        ] {
            for kind in [KindFilter::All, KindFilter::Prs, KindFilter::Issues] {
                assert_eq!(empty_view(view, kind), EMPTY_ISSUE_VIEW, "{view:?}");
            }
        }
    }

    #[test]
    fn pr_views_are_empty_with_the_pr_text() {
        for view in [
            InboxView::YourPrs,
            InboxView::WaitingOnOthers,
            InboxView::ChangesRequested,
            InboxView::ReadyToMerge,
            InboxView::AlreadyDone,
        ] {
            assert_eq!(empty_view(view, KindFilter::Issues), EMPTY_PR_VIEW);
        }
    }

    #[test]
    fn mixed_views_follow_the_kind_filter() {
        for view in [InboxView::Inbox, InboxView::NeedsYou] {
            assert_eq!(empty_view(view, KindFilter::All), EMPTY_MIXED_VIEW);
            assert_eq!(empty_view(view, KindFilter::Prs), EMPTY_PR_VIEW);
            assert_eq!(empty_view(view, KindFilter::Issues), EMPTY_ISSUE_VIEW);
        }
    }

    #[test]
    fn browse_empty_says_nothing_matched_only_when_filtering() {
        assert_eq!(empty_browse(false), EMPTY_BROWSE);
        assert_eq!(empty_browse(true), EMPTY_FILTER);
    }

    #[test]
    fn scope_menu_label_prefixes_the_active_scope() {
        assert_eq!(scope_menu_label(IssueScope::Recent), "· Recentes");
        assert_eq!(scope_menu_label(IssueScope::AllOpen), "· Todas abertas");
    }

    #[test]
    fn recent_hint_names_the_configured_window() {
        assert_eq!(scope_recent_hint(7), "Atualizadas nos últimos 7 dias");
        assert_eq!(scope_recent_hint(14), "Atualizadas nos últimos 14 dias");
        assert_eq!(scope_recent_hint(1), "Atualizadas no último dia");
    }

    #[test]
    fn browse_count_shows_loaded_out_of_the_total() {
        assert_eq!(browse_count(50, 80), "50 de 80");
        assert_eq!(browse_count(31, 31), "31 de 31");
        assert_eq!(browse_count(0, 0), "0 de 0");
    }

    #[test]
    fn up_to_two_labels_show_without_a_counter() {
        for total in 0..=2 {
            let labels: Vec<_> = (0..total).map(|ix| label(&ix.to_string())).collect();
            let (shown, more) = visible_labels(&labels);
            assert_eq!(shown.len(), total);
            assert_eq!(more, None);
        }
    }

    #[test]
    fn extra_labels_collapse_into_a_counter() {
        let labels: Vec<_> = ["bug", "api", "ui", "docs"].map(label).into();
        let (shown, more) = visible_labels(&labels);
        let names: Vec<_> = shown.iter().map(|label| label.name.as_str()).collect();
        assert_eq!(names, ["bug", "api"]);
        assert_eq!(more.as_deref(), Some("+2"));
    }
    #[test]
    fn short_repo_ref_drops_the_owner() {
        assert_eq!(short_repo_ref("argon-labs/argon", 482), "argon #482");
    }

    #[test]
    fn short_repo_ref_keeps_a_repo_without_owner() {
        assert_eq!(short_repo_ref("argon", 482), "argon #482");
    }

    #[test]
    fn reason_tone_maps_every_reason() {
        let cases = [
            (Reason::ReviewRequestedBy("a".into()), Tone::Action),
            (Reason::AssignedBy(None), Tone::Action),
            (Reason::CiFailed, Tone::Danger),
            (Reason::Conflict, Tone::Danger),
            (Reason::ChangesRequestedBy("a".into()), Tone::Warning),
            (Reason::ReadyToMerge, Tone::Success),
            (Reason::WaitingOn(vec!["a".into()]), Tone::Neutral),
            (Reason::ApprovedByYouWaitingAuthor, Tone::Neutral),
            (Reason::IssueAssigned, Tone::Action),
            (Reason::Mentioned, Tone::Action),
            (Reason::IssueAuthored, Tone::Neutral),
            (Reason::Participating, Tone::Neutral),
            (Reason::NewInOrg("acme".into()), Tone::Warning),
            (Reason::ClosedBy(7), Tone::Success),
            (Reason::Tracking, Tone::Neutral),
        ];
        for (reason, tone) in cases {
            assert_eq!(reason_tone(&reason), tone);
        }
    }

    #[test]
    fn column_titles_follow_the_kinds_on_screen() {
        assert_eq!(column_titles(KindFilter::Prs).subject, COLUMN_PR);
        assert_eq!(column_titles(KindFilter::Prs).signal, COLUMN_CI);
        assert_eq!(column_titles(KindFilter::Issues).subject, COLUMN_ISSUE);
        assert_eq!(column_titles(KindFilter::Issues).signal, COLUMN_LABELS);
        assert_eq!(column_titles(KindFilter::All).signal, COLUMN_CI_LABELS);
        assert_eq!(column_titles(KindFilter::All).size, COLUMN_DIFF_LINK);
    }
}

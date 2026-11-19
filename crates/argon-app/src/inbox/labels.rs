use argon_core::{DateBucket, Reason};
use chrono::{DateTime, Utc};

use super::InboxView;
use crate::ui::{self, Tone};

pub const PANEL_TITLE: &str = "Visões";
pub const GROUP_INBOX: &str = "Inbox";
pub const GROUP_AGENTS: &str = "Agentes";
pub const GROUP_YOUR_PRS: &str = "Seus PRs";
pub const GROUP_REVIEWED: &str = "Revisados";
pub const GROUP_ORGS: &str = "Orgs";
pub const GROUP_ACCOUNTS: &str = "Contas";
pub const OPEN_SETTINGS: &str = "Configurações";
pub const ADMIN_BADGE: &str = "admin";
pub const NO_ACCOUNTS: &str = "Nenhuma conta conectada";
pub const NO_ORGS: &str = "Nenhuma org rastreada";
pub const FILTER_PLACEHOLDER: &str = "Filtrar por título, repo, autor, branch…";
pub const FILTER_SHORTCUT: &str = "/";
pub const LOADING: &str = "Carregando PRs…";
pub const LOAD_MORE: &str = "Carregar mais";
pub const EMPTY_VIEW: &str = "Nenhum PR nesta visão.";
pub const EMPTY_FILTER: &str = "Nenhum PR corresponde ao filtro.";
pub const NEEDS_YOU_SECTION: &str = "Needs you";
pub const PR_FALLBACK_TITLE: &str = "PR";
pub const COLUMN_PR: &str = "Pull request";
pub const COLUMN_REASON: &str = "Motivo";
pub const COLUMN_CI: &str = "CI";
pub const COLUMN_REVIEW: &str = "Review";
pub const COLUMN_DIFF: &str = "Diff";
pub const COLUMN_UPDATED: &str = "Atualizado";

const TAB_TITLE_MAX_CHARS: usize = 16;

pub fn view_title(view: InboxView) -> &'static str {
    match view {
        InboxView::Inbox => "Inbox",
        InboxView::NeedsYou => "Needs you",
        InboxView::YourPrs => "Your PRs",
        InboxView::WaitingOnOthers => "Waiting on others",
        InboxView::ChangesRequested => "Changes requested",
        InboxView::ReadyToMerge => "Ready to merge",
        InboxView::AlreadyDone => "Already done",
    }
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
        Reason::Tracking => tracking_line(now, updated_at),
    }
}

pub fn reason_tone(reason: &Reason) -> Tone {
    match reason {
        Reason::ReviewRequestedBy(_) | Reason::AssignedBy(_) => Tone::Action,
        Reason::CiFailed | Reason::Conflict => Tone::Danger,
        Reason::ChangesRequestedBy(_) => Tone::Warning,
        Reason::ReadyToMerge => Tone::Success,
        Reason::WaitingOn(_) | Reason::ApprovedByYouWaitingAuthor | Reason::Tracking => {
            Tone::Neutral
        }
    }
}

fn tracking_line(now: DateTime<Utc>, updated_at: DateTime<Utc>) -> String {
    format!("Acompanhando · atualizado {}", ui::ago(now, updated_at))
}

#[cfg(test)]
mod tests {
    use super::*;

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
            (Reason::Tracking, Tone::Neutral),
        ];
        for (reason, tone) in cases {
            assert_eq!(reason_tone(&reason), tone);
        }
    }
}

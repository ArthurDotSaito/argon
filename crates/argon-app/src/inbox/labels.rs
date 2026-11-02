use argon_core::{DateBucket, Reason};
use chrono::{DateTime, Utc};

use super::InboxView;

pub const PANEL_TITLE: &str = "Visões";
pub const GROUP_INBOX: &str = "Inbox";
pub const GROUP_YOUR_PRS: &str = "Seus PRs";
pub const GROUP_REVIEWED: &str = "Revisados";
pub const GROUP_ORGS: &str = "Orgs";
pub const GROUP_ACCOUNTS: &str = "Contas";
pub const ADMIN_BADGE: &str = "admin";
pub const NO_ACCOUNTS: &str = "Nenhuma conta conectada";
pub const NO_ORGS: &str = "Nenhuma org rastreada";
pub const FILTER_PLACEHOLDER: &str = "Filtrar por título, repo, autor, branch…";
pub const FILTER_SHORTCUT: &str = "/";
pub const LOADING: &str = "Carregando PRs…";
pub const EMPTY_VIEW: &str = "Nenhum PR nesta visão.";
pub const EMPTY_FILTER: &str = "Nenhum PR corresponde ao filtro.";
pub const NEEDS_YOU_SECTION: &str = "Needs you";
pub const PR_FALLBACK_TITLE: &str = "PR";
pub const PR_DETAIL_PENDING: &str = "O detalhe do PR chega em breve.";
pub const PR_NOT_LOADED: &str = "PR fora da inbox local.";

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
        DateBucket::ThisWeek => "Esta semana",
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

fn tracking_line(now: DateTime<Utc>, updated_at: DateTime<Utc>) -> String {
    match Age::of(now, updated_at) {
        Age::Now => "Acompanhando · atualizado agora".to_string(),
        Age::Yesterday => "Acompanhando · atualizado ontem".to_string(),
        age => format!("Acompanhando · atualizado há {}", age.label()),
    }
}

pub fn age_label(now: DateTime<Utc>, ts: DateTime<Utc>) -> String {
    Age::of(now, ts).label()
}

enum Age {
    Now,
    Minutes(i64),
    Hours(i64),
    Yesterday,
    Days(i64),
    Weeks(i64),
    Months(i64),
}

impl Age {
    fn of(now: DateTime<Utc>, ts: DateTime<Utc>) -> Age {
        let minutes = (now - ts).num_minutes().max(0);
        let days = minutes / (60 * 24);
        match (minutes, days) {
            (0, _) => Age::Now,
            (1..60, _) => Age::Minutes(minutes),
            (_, 0) => Age::Hours(minutes / 60),
            (_, 1) => Age::Yesterday,
            (_, 2..7) => Age::Days(days),
            (_, 7..30) => Age::Weeks(days / 7),
            _ => Age::Months(days / 30),
        }
    }

    fn label(&self) -> String {
        match self {
            Age::Now => "agora".to_string(),
            Age::Minutes(minutes) => format!("{minutes} min"),
            Age::Hours(hours) => format!("{hours} h"),
            Age::Yesterday => "ontem".to_string(),
            Age::Days(days) => format!("{days} d"),
            Age::Weeks(weeks) => format!("{weeks} sem"),
            Age::Months(months) => format!("{months} mês"),
        }
    }
}

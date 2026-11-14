use argon_core::SubIssues;

use super::actions::IssueAction;

pub const NOT_IN_INBOX: &str = "Issue fora da inbox local.";
pub const OPENED_THIS_ISSUE: &str = "abriu esta issue";
pub const NO_ASSIGNEES: &str = "sem responsável";
pub const NO_LINKED_PR: &str = "sem PR vinculado";
pub const TAB_LINKED_PRS: &str = "PRs vinculados";
pub const TAB_SUB_ISSUES: &str = "Sub-issues";
pub const LINKED_PRS_HELP: &str = "PRs que fecham esta issue quando entrarem.";
pub const NO_LINKED_PRS: &str = "Nenhum PR vinculado ainda.";

pub const ASSIGN_TO_ME: &str = "Atribuir a mim";
pub const CLOSE: &str = "Fechar";
pub const CLOSE_COMPLETED: &str = "Fechar como concluída";
pub const CLOSE_NOT_PLANNED: &str = "Fechar como não planejada";
pub const PLAN_LOAD_FAILED: &str = "Não consegui carregar a issue para o Agent Plan";
pub const PLAN_DRAFT_READY: &str = "Rascunho do Agent Plan pronto";
pub const REVIEW_AND_PUBLISH: &str = "Revisar e publicar";

pub fn load_failed(error: &str) -> String {
    format!("Falha ao carregar a issue: {error}")
}

pub fn action_failed(action: IssueAction, error: &str) -> String {
    match action {
        IssueAction::Assign => format!("Falha ao atribuir a issue a você: {error}"),
        IssueAction::Close(_) => format!("Falha ao fechar a issue: {error}"),
    }
}

pub fn opened(ago: &str) -> String {
    format!("· aberta {ago}")
}

pub fn sub_issues(sub: SubIssues) -> String {
    format!("{} sub-issues", sub_issue_progress(sub))
}

pub fn sub_issue_progress(sub: SubIssues) -> String {
    format!("{}/{}", sub.completed, sub.total)
}

pub fn reference(issue_repo: &str, repo: &str, number: u64) -> String {
    if repo.eq_ignore_ascii_case(issue_repo) {
        format!("#{number}")
    } else {
        format!("{repo}#{number}")
    }
}

pub fn labeled(actor: &str) -> String {
    format!("{actor} adicionou")
}

pub fn unlabeled(actor: &str) -> String {
    format!("{actor} removeu")
}

pub fn assigned(actor: &str, assignee: &str) -> String {
    if actor.eq_ignore_ascii_case(assignee) {
        format!("{actor} se atribuiu")
    } else {
        format!("{actor} atribuiu {assignee}")
    }
}

pub fn unassigned(actor: &str, assignee: &str) -> String {
    if actor.eq_ignore_ascii_case(assignee) {
        format!("{actor} deixou de ser responsável")
    } else {
        format!("{actor} desatribuiu {assignee}")
    }
}

pub fn closed(actor: &str) -> String {
    format!("{actor} fechou esta issue")
}

pub fn reopened(actor: &str) -> String {
    format!("{actor} reabriu esta issue")
}

pub fn milestoned(actor: &str, milestone: &str) -> String {
    format!("{actor} adicionou ao milestone {milestone}")
}

pub fn cross_referenced(actor: &str, linked: bool) -> String {
    if linked {
        format!("{actor} vinculou um pull request")
    } else {
        format!("{actor} mencionou esta issue em")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_omits_the_repo_when_it_is_the_issue_repo() {
        assert_eq!(reference("acme/api", "Acme/API", 7), "#7");
        assert_eq!(reference("acme/api", "acme/web", 7), "acme/web#7");
    }

    #[test]
    fn assignment_texts_tell_self_assignment_apart() {
        assert_eq!(assigned("ana", "Ana"), "ana se atribuiu");
        assert_eq!(assigned("ana", "bruno"), "ana atribuiu bruno");
        assert_eq!(unassigned("ana", "ana"), "ana deixou de ser responsável");
        assert_eq!(unassigned("ana", "bruno"), "ana desatribuiu bruno");
    }

    #[test]
    fn cross_reference_text_depends_on_the_link() {
        assert_eq!(
            cross_referenced("ana", true),
            "ana vinculou um pull request"
        );
        assert_eq!(
            cross_referenced("ana", false),
            "ana mencionou esta issue em"
        );
    }

    #[test]
    fn sub_issue_progress_reads_completed_over_total() {
        let sub = SubIssues {
            total: 3,
            completed: 1,
        };
        assert_eq!(sub_issues(sub), "1/3 sub-issues");
        assert_eq!(sub_issue_progress(sub), "1/3");
    }
}

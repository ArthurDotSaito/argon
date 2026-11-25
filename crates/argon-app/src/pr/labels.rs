use argon_core::{IssueRef, PrState, ReviewState};
use argon_github::MergeMethod;

pub const OPEN_ON_GITHUB: &str = "Abrir no GitHub";
pub const OPEN_ISSUE: &str = "Abrir a issue";
pub const REFRESH: &str = "Atualizar";
pub const COLLAPSE_PANEL: &str = "Recolher painel";
pub const SHOW_PANEL: &str = "Mostrar painel";
pub const MERGE: &str = "Merge";
pub const REVIEW: &str = "Revisar";
pub const REVIEW_TOOLTIP: &str = "Abrir a review deste PR";
pub const AI_REVIEW: &str = "Agent Review";
pub const SUMMARIZE_LOAD_FAILED: &str = "Não consegui carregar o PR para o Summarize";
pub const SUMMARY_AVAILABLE: &str = "Resumo disponível";
pub const SUMMARY_STALE: &str = "Resumo desatualizado";
pub const SUMMARY_STALE_NOTE: &str = "· o PR recebeu commits depois do resumo";
pub const OPEN_SUMMARY: &str = "Abrir resumo";
pub const SUMMARY_REGENERATE: &str = "Gerar de novo";
pub const RETRY: &str = "Tentar de novo";
pub const COPY_BRANCH: &str = "Copiar branch";
pub const MERGING: &str = "Fazendo merge…";
pub const MERGE_HEAD_MOVED: &str =
    "O PR recebeu commits novos desde que você abriu — recarregue e confira";
pub const NO_DESCRIPTION: &str = "Sem descrição.";
pub const OPENED_THIS_PR: &str = "abriu este PR";
pub const COMMENTED: &str = "comentou";
pub const REVIEWED: &str = "revisou";
pub const TIMELINE_TRUNCATED: &str = "Timeline truncada — mostrando os 100 eventos mais recentes";
pub const COMMENT_PLACEHOLDER: &str = "Comentar… (markdown)";
pub const COMMENT: &str = "Comentar";
pub const COMMENT_SENT: &str = "Comentário enviado";
pub const NO_REVIEWERS: &str = "sem reviewers";
pub const FILES: &str = "arquivos";
pub const MERGE_DIALOG_TITLE: &str = "Fazer merge do PR";
pub const MERGE_METHOD: &str = "Método de merge";
pub const CANCEL: &str = "Cancelar";
pub const NOT_IN_INBOX: &str = "PR fora da inbox local.";
pub const SERVICES_UNAVAILABLE: &str = "serviços em segundo plano indisponíveis";
pub const LABELED: &str = "adicionou o label";
pub const FORCE_PUSHED: &str = "fez force-push na branch";
pub const MERGED: &str = "fez merge deste PR";
pub const CLOSED: &str = "fechou este PR";
pub const READY_FOR_REVIEW: &str = "marcou como pronto para review";

pub const PROPERTY_REASON: &str = "Motivo";
pub const PROPERTY_AUTHOR: &str = "Autor";
pub const PROPERTY_PROJECT: &str = "Projeto";
pub const OPEN_PROJECT: &str = "Abrir a página do projeto";
pub const PROPERTY_CI: &str = "CI";
pub const PROPERTY_REVIEWERS: &str = "Reviewers";
pub const PROPERTY_CHANGES: &str = "Alterações";
pub const PROPERTY_LABELS: &str = "Labels";
pub const PROPERTY_ISSUES: &str = "Issues";
pub const BRANCH_JOINER: &str = "em";
const FILES_JOINER: &str = "em";

pub const TAB_CONVERSATION: &str = "Conversa";
pub const TAB_OVERVIEW: &str = "Visão geral";
pub const TAB_COMMITS: &str = "Commits";
pub const TAB_CHECKS: &str = "Checks";
pub const TAB_FILES: &str = "Arquivos";

pub const SEPARATOR: &str = "·";

pub fn short_sha(oid: &str) -> &str {
    oid.get(..7).unwrap_or(oid)
}

pub fn load_failed(error: &str) -> String {
    format!("Falha ao carregar o PR: {error}")
}

pub fn summary_fresh_note(ago: &str) -> String {
    format!("· gerado {ago}")
}

pub fn comment_failed(error: &str) -> String {
    format!("Falha ao comentar: {error}")
}

pub fn merge_failed(error: &str) -> String {
    format!("Falha no merge: {error}")
}

pub fn merge_confirm(number: u64, base: &str) -> String {
    format!("Fazer merge de #{number} em {base}")
}

pub fn merge_summary(number: u64, head: &str, base: &str) -> String {
    format!("O PR #{number} ({head}) será integrado em {base} no GitHub.")
}

pub fn merge_method(method: MergeMethod) -> &'static str {
    match method {
        MergeMethod::Merge => "Merge commit",
        MergeMethod::Squash => "Squash and merge",
        MergeMethod::Rebase => "Rebase and merge",
    }
}

pub fn changed_files(count: u32) -> String {
    format!("{count} {FILES}")
}

pub fn closing_issue(closing: &IssueRef, pr_repo: &str, merged: bool) -> String {
    let verb = if merged { "Fechou" } else { "Fecha" };
    if closing.repo.eq_ignore_ascii_case(pr_repo) {
        format!("{verb} #{}", closing.number)
    } else {
        format!("{verb} {}#{}", closing.repo, closing.number)
    }
}

pub fn review_state(state: ReviewState) -> &'static str {
    match state {
        ReviewState::Approved => "Approved",
        ReviewState::ChangesRequested => "Changes requested",
        ReviewState::Commented => "Commented",
        ReviewState::Dismissed => "Dismissed",
        ReviewState::Pending => "Pending",
    }
}

pub fn settled_review(state: Option<ReviewState>, requested: bool) -> Option<ReviewState> {
    match state {
        Some(ReviewState::Pending) | None => None,
        Some(_) if requested => None,
        settled => settled,
    }
}

pub fn reviewer_status(state: Option<ReviewState>, requested: bool) -> &'static str {
    match settled_review(state, requested) {
        None | Some(ReviewState::Pending) => "pendente",
        Some(ReviewState::Approved) => "aprovou",
        Some(ReviewState::ChangesRequested) => "pediu mudanças",
        Some(ReviewState::Commented) => "comentou",
        Some(ReviewState::Dismissed) => "descartada",
    }
}

pub fn review_requested(reviewer: &str) -> String {
    format!("pediu review de {reviewer}")
}

pub fn in_changed_files(count: u32) -> String {
    format!("{FILES_JOINER} {}", changed_files(count))
}

pub fn line_ref(path: &str, line: Option<u32>) -> String {
    match line {
        Some(line) => format!("{path}:{line}"),
        None => path.to_string(),
    }
}

pub fn failed_checks(count: usize) -> String {
    match count {
        1 => "1 falhou".to_string(),
        count => format!("{count} falharam"),
    }
}

pub fn merge_intent(state: PrState, commits: Option<usize>) -> String {
    let verb = match state {
        PrState::Open => "quer integrar",
        PrState::Merged => "integrou",
        PrState::Closed => "queria integrar",
    };
    match commits {
        Some(1) => format!("{verb} 1 commit de"),
        Some(commits) => format!("{verb} {commits} commits de"),
        None => format!("{verb} a branch"),
    }
}

pub const ACTIVITY: &str = "Atividade";
pub const ORDER_NEWEST: &str = "Recentes primeiro";
pub const ORDER_OLDEST: &str = "Antigos primeiro";
pub const FILTER_CONVERSATION: &str = "Conversa";
pub const FILTER_ALL: &str = "Tudo";
pub const SHOW_ALL: &str = "Mostrar tudo";
pub const SHOW: &str = "ver";
pub const HIDE: &str = "ocultar";
pub const SHOW_COMMITS: &str = "ver commits";
pub const HIDE_COMMITS: &str = "ocultar commits";
pub const OPEN_COMMITS_TAB: &str = "Abrir a aba Commits";
pub const COMMENTED_ON_CODE: &str = "comentou no código";
pub const THREAD_OPEN: &str = "em aberto";
pub const THREAD_RESOLVED: &str = "resolvida";
pub const THREAD_OUTDATED: &str = "desatualizada";
pub const GO_TO_LINE: &str = "Ir para a linha";
pub const DESCRIPTION: &str = "Descrição";
pub const SHOW_DESCRIPTION: &str = "Mostrar a descrição inteira";
pub const HIDE_DESCRIPTION: &str = "Recolher a descrição";
pub const SEVERAL_AUTHORS: &str = "Vários autores";

pub fn opened_ago(author: &str, ago: &str) -> String {
    format!("{SEPARATOR} {author} abriu {ago}")
}

pub fn added_commits(count: usize, several_authors: bool) -> String {
    let verb = if several_authors {
        "adicionaram"
    } else {
        "adicionou"
    };
    match count {
        1 => format!("{verb} 1 commit"),
        count => format!("{verb} {count} commits"),
    }
}

pub fn more_commits(count: usize) -> String {
    format!("… e mais {count}.")
}

pub fn hidden_events(commits: usize, bot: usize, events: usize) -> String {
    let total = commits + bot + events;
    let plural = |count: usize, one: &str, many: &str| match count {
        1 => format!("1 {one}"),
        count => format!("{count} {many}"),
    };
    let parts: Vec<String> = [
        (commits, "commit", "commits"),
        (bot, "de bot", "de bots"),
        (events, "evento do PR", "eventos do PR"),
    ]
    .into_iter()
    .filter(|(count, _, _)| *count > 0)
    .map(|(count, one, many)| plural(count, one, many))
    .collect();
    format!(
        "{}: {}.",
        plural(total, "item oculto", "itens ocultos"),
        parts.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue(repo: &str, number: u64) -> IssueRef {
        IssueRef {
            repo: repo.into(),
            number,
        }
    }

    #[test]
    fn hidden_events_lists_only_what_is_hidden() {
        assert_eq!(
            hidden_events(24, 1, 4),
            "29 itens ocultos: 24 commits, 1 de bot, 4 eventos do PR."
        );
        assert_eq!(hidden_events(0, 0, 1), "1 item oculto: 1 evento do PR.");
        assert_eq!(
            hidden_events(1, 2, 0),
            "3 itens ocultos: 1 commit, 2 de bots."
        );
    }

    #[test]
    fn merge_intent_follows_the_state_and_the_commit_count() {
        assert_eq!(
            merge_intent(PrState::Open, Some(24)),
            "quer integrar 24 commits de"
        );
        assert_eq!(
            merge_intent(PrState::Merged, Some(1)),
            "integrou 1 commit de"
        );
        assert_eq!(
            merge_intent(PrState::Closed, None),
            "queria integrar a branch"
        );
    }

    #[test]
    fn failed_checks_agrees_in_number() {
        assert_eq!(failed_checks(1), "1 falhou");
        assert_eq!(failed_checks(3), "3 falharam");
    }

    #[test]
    fn added_commits_agrees_in_number() {
        assert_eq!(added_commits(1, false), "adicionou 1 commit");
        assert_eq!(added_commits(24, false), "adicionou 24 commits");
        assert_eq!(added_commits(3, true), "adicionaram 3 commits");
    }

    #[test]
    fn fresh_summary_note_carries_the_age() {
        assert_eq!(summary_fresh_note("há 2 h"), "· gerado há 2 h");
    }

    #[test]
    fn closing_issue_in_the_same_repo_shows_only_the_number() {
        assert_eq!(
            closing_issue(&issue("acme/api", 76), "acme/api", false),
            "Fecha #76"
        );
    }

    #[test]
    fn closing_issue_matches_the_repo_without_case() {
        assert_eq!(
            closing_issue(&issue("Acme/API", 76), "acme/api", false),
            "Fecha #76"
        );
    }

    #[test]
    fn closing_issue_in_another_repo_shows_the_repo() {
        assert_eq!(
            closing_issue(&issue("acme/web", 3), "acme/api", false),
            "Fecha acme/web#3"
        );
    }

    #[test]
    fn merged_pull_request_closed_its_issues() {
        assert_eq!(
            closing_issue(&issue("acme/api", 76), "acme/api", true),
            "Fechou #76"
        );
        assert_eq!(
            closing_issue(&issue("acme/web", 3), "acme/api", true),
            "Fechou acme/web#3"
        );
    }
}

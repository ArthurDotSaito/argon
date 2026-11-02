use argon_core::ReviewState;
use argon_github::MergeMethod;

pub const OPEN_ON_GITHUB: &str = "Abrir no GitHub";
pub const REFRESH: &str = "Atualizar";
pub const MERGE: &str = "Merge";
pub const AI_REVIEW: &str = "Revisar com IA";
pub const AI_REVIEW_SOON: &str =
    "Em breve: review com agentes de IA (Claude Code, Codex, OpenCode…)";
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
pub const UNDER_CONSTRUCTION: &str = "Em construção";
pub const REVIEW_SOON: &str = "Diff e comentários chegam na Fase 3";
pub const MERGE_DIALOG_TITLE: &str = "Fazer merge do PR";
pub const MERGE_METHOD: &str = "Método de merge";
pub const CANCEL: &str = "Cancelar";
pub const NOT_IN_INBOX: &str = "PR fora da inbox local.";
pub const SERVICES_UNAVAILABLE: &str = "serviços em segundo plano indisponíveis";

pub const TAB_CONVERSATION: &str = "Conversa";
pub const TAB_COMMITS: &str = "Commits";
pub const TAB_ACTIONS: &str = "Actions";
pub const TAB_REVIEW: &str = "Review";

pub const SEPARATOR: &str = "·";
pub const REVIEW_SHOW_ALL: &str = "Ver todos";
pub const REVIEW_LOADING_FILES: &str = "Carregando arquivos do commit…";
pub const REVIEW_NO_FILES: &str = "Este commit não alterou arquivos.";

pub fn commit_banner(oid: &str, headline: &str) -> String {
    format!("Mostrando só o commit {} — {headline}", short_sha(oid))
}

pub fn files_failed(error: &str) -> String {
    format!("Falha ao carregar os arquivos do commit: {error}")
}

pub fn commit_gone(oid: &str) -> String {
    format!(
        "O commit {} não faz mais parte do PR (force-push)",
        short_sha(oid)
    )
}

pub fn commit_file_count(count: usize) -> String {
    match count {
        1 => "1 arquivo neste commit".to_string(),
        count => format!("{count} arquivos neste commit"),
    }
}

pub fn short_sha(oid: &str) -> &str {
    oid.get(..7).unwrap_or(oid)
}

pub fn load_failed(error: &str) -> String {
    format!("Falha ao carregar o PR: {error}")
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

pub fn reviewer_tooltip(login: &str, state: Option<ReviewState>, requested: bool) -> String {
    match settled_review(state, requested) {
        None | Some(ReviewState::Pending) => format!("{login}: review pendente"),
        Some(ReviewState::Approved) => format!("{login}: aprovou"),
        Some(ReviewState::ChangesRequested) => format!("{login}: pediu mudanças"),
        Some(ReviewState::Commented) => format!("{login}: comentou"),
        Some(ReviewState::Dismissed) => format!("{login}: review descartada"),
    }
}

pub fn review_requested(actor: &str, reviewer: &str) -> String {
    format!("{actor} pediu review de {reviewer}")
}

pub fn labeled(actor: &str) -> String {
    format!("{actor} adicionou o label")
}

pub fn force_pushed(actor: &str) -> String {
    format!("{actor} fez force-push na branch")
}

pub fn merged(actor: &str) -> String {
    format!("{actor} fez merge deste PR")
}

pub fn closed(actor: &str) -> String {
    format!("{actor} fechou este PR")
}

pub fn ready_for_review(actor: &str) -> String {
    format!("{actor} marcou como pronto para review")
}

pub fn line_ref(path: &str, line: Option<u32>) -> String {
    match line {
        Some(line) => format!("{path}:{line}"),
        None => path.to_string(),
    }
}

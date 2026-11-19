use argon_core::ReviewState;
use argon_github::MergeMethod;

pub const OPEN_ON_GITHUB: &str = "Abrir no GitHub";
pub const REFRESH: &str = "Atualizar";
pub const MERGE: &str = "Merge";
pub const REVIEW: &str = "Revisar";
pub const REVIEW_TOOLTIP: &str = "Abrir a review deste PR";
pub const AI_REVIEW: &str = "Agent Review";
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

pub const PROPERTY_STATE: &str = "Estado";
pub const PROPERTY_REASON: &str = "Motivo";
pub const PROPERTY_AUTHOR: &str = "Autor";
pub const PROPERTY_BRANCHES: &str = "Branches";
pub const PROPERTY_CI: &str = "CI";
pub const PROPERTY_REVIEWERS: &str = "Reviewers";
pub const PROPERTY_CHANGES: &str = "Alterações";
pub const PROPERTY_LABELS: &str = "Labels";
pub const BRANCH_JOINER: &str = "em";
const FILES_JOINER: &str = "em";

pub const TAB_CONVERSATION: &str = "Conversa";
pub const TAB_COMMITS: &str = "Commits";
pub const TAB_ACTIONS: &str = "Actions";
pub const TAB_REVIEW: &str = "Review";

pub const SEPARATOR: &str = "·";

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

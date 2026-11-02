pub use crate::pr::labels::{
    CANCEL, OPEN_ON_GITHUB, RETRY, SEPARATOR, SERVICES_UNAVAILABLE, short_sha,
};

pub const REVIEW_SHOW_ALL: &str = "Ver todos";
pub const LOADING_FILES: &str = "Carregando arquivos…";
pub const NO_FILES: &str = "Nenhum arquivo alterado.";
pub const NO_FILTER_MATCH: &str = "Nenhum arquivo corresponde ao filtro.";
pub const FILTER_PLACEHOLDER: &str = "Filtrar arquivos";
pub const VIEWED: &str = "Visto";
pub const SAVING: &str = "Salvando…";
pub const SAVING_VIEWED: &str = "Salvando o estado de visto no GitHub…";
pub const SPLIT: &str = "Split";
pub const INLINE: &str = "Inline";
pub const MODE_SHORTCUT: &str = "Alternar Split/Inline (s)";
pub const DIFF_TOO_LARGE: &str = "Diff grande demais ou binário —";
pub const DIFF_INVALID: &str = "Não foi possível exibir o diff —";
pub const OUTDATED_COMMENTS: &str = "Comentários desatualizados";
pub const FILE_COMMENT: &str = "Comentário no arquivo";
pub const COMMIT_COMMENT_HINT: &str = "Para comentar, veja todos os arquivos";
pub const RESOLVED: &str = "Conversa resolvida";
pub const SHOW_RESOLVED: &str = "Mostrar conversa resolvida";
pub const HIDE_RESOLVED: &str = "Ocultar";
pub const MORE_ON_GITHUB: &str = "Há mais comentários nesta conversa — veja no GitHub";
pub const PENDING: &str = "Pending";
pub const AI_BADGE: &str = "IA";
pub const AI_BADGE_TOOLTIP: &str = "Sugerido por um agente de IA";
pub const EDIT: &str = "Editar";
pub const DELETE: &str = "Excluir";
pub const ADD_TO_REVIEW: &str = "Adicionar à review";
pub const ADD_TO_REVIEW_SHORTCUT: &str = "Adicionar à review (Ctrl+Enter)";
pub const COMMENT_NOW: &str = "Comentar agora";
pub const COMMENT_NOW_TOOLTIP: &str = "Publica já no GitHub, fora da review";
pub const SAVE: &str = "Salvar";
pub const REPLY: &str = "Responder";
pub const REPLY_PLACEHOLDER: &str = "Responder…";
pub const EDITOR_PLACEHOLDER: &str = "Deixe um comentário… (markdown)";
pub const RESOLVE: &str = "Resolver conversa";
pub const UNRESOLVE: &str = "Reabrir conversa";
pub const YOU: &str = "você";
pub const DISCARD: &str = "Descartar";
pub const DISCARD_COMMENT_TITLE: &str = "Descartar comentário?";
pub const DISCARD_COMMENT_MESSAGE: &str = "O texto digitado neste comentário será perdido.";
pub const DELETE_DRAFT_TITLE: &str = "Excluir rascunho?";
pub const DELETE_DRAFT_MESSAGE: &str = "O comentário pendente será apagado.";
pub const REVIEW_IN_PROGRESS: &str = "Review em andamento";
pub const FINISH_REVIEW: &str = "Finish review";
pub const START_REVIEW: &str = "Review";
pub const NO_PENDING_COMMENTS: &str = "Nenhum comentário pendente";
pub const HEAD_MOVED: &str =
    "O PR recebeu commits novos — confira se os comentários ainda estão nas linhas certas";
pub const GENERAL_COMMENT_PLACEHOLDER: &str = "Comentário geral (markdown)";
pub const EVENT_COMMENT: &str = "Comment";
pub const EVENT_APPROVE: &str = "Approve";
pub const EVENT_REQUEST_CHANGES: &str = "Request changes";
pub const EVENT_COMMENT_HINT: &str = "Feedback geral, sem aprovar";
pub const EVENT_APPROVE_HINT: &str = "Aprovar o merge destas mudanças";
pub const EVENT_REQUEST_CHANGES_HINT: &str = "Feedback que precisa ser resolvido antes do merge";
pub const OWN_PR: &str = "O GitHub não permite aprovar ou pedir mudanças no seu próprio PR";
pub const PENDING_COMMENTS: &str = "Comentários pendentes";
pub const NO_PENDING: &str = "Nenhum comentário nas linhas.";
pub const SUBMIT_REVIEW: &str = "Submit review";
pub const DISCARD_REVIEW: &str = "Descartar review";
pub const DISCARD_REVIEW_TITLE: &str = "Descartar review?";
pub const REVIEW_SENT: &str = "Review enviada";
pub const OLD_SIDE_SUFFIX: &str = " (versão antiga)";
pub const UNKNOWN_DRAFT: &str = "um rascunho";
pub const REVIEW_EMPTY: &str = "Escreva um comentário geral ou adicione comentários nas linhas.";
pub const REQUEST_CHANGES_NEEDS_BODY: &str = "Request changes precisa de um comentário geral.";

pub fn commit_banner(oid: &str, headline: &str) -> String {
    format!("Mostrando só o commit {} — {headline}", short_sha(oid))
}

pub fn files_failed(error: &str) -> String {
    format!("Falha ao carregar os arquivos: {error}")
}

pub fn viewed_failed(path: &str, error: &str) -> String {
    format!("Falha ao marcar {path} como visto: {error}")
}

pub fn commit_gone(oid: &str) -> String {
    format!(
        "O commit {} não faz mais parte do PR (force-push)",
        short_sha(oid)
    )
}

pub fn outside_comments(count: usize) -> String {
    match count {
        1 => "1 comentário em arquivo fora deste diff".to_string(),
        count => format!("{count} comentários em arquivos fora deste diff"),
    }
}

pub fn viewed_count(viewed: usize, total: usize) -> String {
    format!("{viewed} de {total} vistos")
}

pub fn file_count(count: usize) -> String {
    match count {
        1 => "1 arquivo".to_string(),
        count => format!("{count} arquivos"),
    }
}

pub fn original_line(line: Option<u32>) -> String {
    match line {
        Some(line) => format!("linha original {line}"),
        None => "linha original desconhecida".to_string(),
    }
}

pub fn pending_count(count: usize) -> String {
    match count {
        1 => "1 comentário pendente".to_string(),
        count => format!("{count} comentários pendentes"),
    }
}

pub fn discard_review_message(count: usize) -> String {
    format!(
        "{} e o comentário geral serão apagados.",
        match count {
            1 => "O comentário pendente".to_string(),
            count => format!("Os {count} comentários pendentes"),
        }
    )
}

pub fn editor_target(line: u32, start: Option<u32>) -> String {
    match start {
        Some(start) => format!("Comentário nas linhas {start}–{line}"),
        None => format!("Comentário na linha {line}"),
    }
}

pub fn invalid_range(location: &str) -> String {
    format!("O intervalo do comentário em {location} é inválido.")
}

pub fn empty_comment(location: &str) -> String {
    format!("O comentário em {location} está vazio.")
}

pub fn draft_failed(error: &str) -> String {
    format!("Falha ao salvar o rascunho: {error}")
}

pub fn drafts_load_failed(error: &str) -> String {
    format!("Falha ao carregar os rascunhos: {error}")
}

pub fn comment_failed(error: &str) -> String {
    format!("Falha ao publicar o comentário: {error}")
}

pub fn reply_failed(error: &str) -> String {
    format!("Falha ao responder: {error}")
}

pub fn resolve_failed(error: &str) -> String {
    format!("Falha ao atualizar a conversa: {error}")
}

pub fn submit_failed(error: &str) -> String {
    format!("Falha ao enviar a review: {error}")
}

pub fn comment_count(count: usize) -> String {
    match count {
        1 => "1 comentário".to_string(),
        count => format!("{count} comentários"),
    }
}

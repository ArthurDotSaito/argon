pub use crate::pr::labels::{OPEN_ON_GITHUB, RETRY, SEPARATOR, SERVICES_UNAVAILABLE, short_sha};

pub const REVIEW_SHOW_ALL: &str = "Ver todos";
pub const LOADING_FILES: &str = "Carregando arquivos…";
pub const NO_FILES: &str = "Nenhum arquivo alterado.";
pub const NO_FILTER_MATCH: &str = "Nenhum arquivo corresponde ao filtro.";
pub const FILTER_PLACEHOLDER: &str = "Filtrar arquivos";
pub const VIEWED: &str = "Visto";
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

pub fn renamed_from(previous: &str) -> String {
    format!("{previous} →")
}

pub fn original_line(line: Option<u32>) -> String {
    match line {
        Some(line) => format!("linha original {line}"),
        None => "linha original desconhecida".to_string(),
    }
}

pub fn comment_count(count: usize) -> String {
    match count {
        1 => "1 comentário".to_string(),
        count => format!("{count} comentários"),
    }
}

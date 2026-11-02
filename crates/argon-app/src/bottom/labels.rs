pub const PANEL_TITLE: &str = "Log";
pub const EMPTY_HINT: &str = "Selecione um job na aba Actions de um PR para ver o log.";
pub const LOADING: &str = "Carregando log…";
pub const EMPTY_LOG: &str = "Log vazio.";
pub const TRUNCATED: &str = "Log truncado — mostrando os últimos 5 MB";
pub const OPEN_ON_GITHUB: &str = "Abrir no GitHub";
pub const RETRY: &str = "Tentar de novo";
pub const SEARCH_PLACEHOLDER: &str = "Buscar no log (Ctrl+F)";
pub const PREVIOUS_MATCH: &str = "Ocorrência anterior (Shift+Enter)";
pub const NEXT_MATCH: &str = "Próxima ocorrência (Enter)";
pub const JUMP_TO_ERROR: &str = "Ir para o primeiro erro";
pub const NO_MATCHES: &str = "0 resultados";
pub const SERVICES_UNAVAILABLE: &str = "serviços em segundo plano indisponíveis";

pub fn load_failed(error: &str) -> String {
    format!("Falha ao carregar o log: {error}")
}

pub fn title(workflow: &str, job: &str) -> String {
    format!("{workflow} / {job}")
}

pub fn match_position(current: Option<usize>, count: usize) -> String {
    match current {
        Some(current) => format!("{}/{count}", current + 1),
        None => format!("{count} resultados"),
    }
}

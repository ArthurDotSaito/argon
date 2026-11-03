pub const DIALOG_TITLE: &str = "Revisar com IA";
pub const AGENT: &str = "Agente";
pub const GENERIC_AGENT: &str = "agente";
pub const MODEL: &str = "Modelo";
pub const TEMPLATE: &str = "Template";
pub const OPTIONS: &str = "Opções";
pub const EDIT_IN_SETTINGS: &str = "Editar em Configurações";
pub const DETECTING: &str = "detectando…";
pub const NOT_FOUND: &str = "não encontrado no PATH";
pub const NOT_SUPPORTED: &str = "Ainda não suportado";
pub const NO_MODELS: &str = "Escolha um agente";
pub const NO_MODEL_LISTED: &str = "Nenhum modelo disponível";
pub const NO_TEMPLATES: &str = "Nenhum template";
pub const LOADING_TEMPLATES: &str = "carregando templates…";
pub const INCLUDE_REVIEWS: &str = "Incluir reviews anteriores";
pub const INCLUDE_CI: &str = "Incluir falhas de CI";
pub const ISOLATED_WORKTREE: &str = "Rodar em worktree isolado";
pub const ISOLATED_WORKTREE_HINT: &str =
    "Obrigatório por segurança: o agente nunca roda no seu checkout";
pub const DRAFTS_ONLY: &str = "O agente só cria rascunhos";
pub const DRAFTS_ONLY_HINT: &str = "Achados viram comentários pendentes; nada é publicado sem você";
pub const PROMPT_PREVIEW: &str = "prompt";
pub const COMMAND_PREVIEW: &str = "comando";
pub const CWD: &str = "cwd";
pub const MODEL_LINE: &str = "modelo";
pub const ENV_LINE: &str = "env";
pub const BUILDING_CONTEXT: &str = "montando contexto…";
pub const DIFF_INLINE: &str = "Diff incluído no prompt";
pub const DIFF_IN_WORKTREE: &str = "O agente vai ler o diff no worktree (git diff)";
pub const LOADING_CI: &str = "buscando falhas de CI…";
pub const PREPARING_WORKTREE: &str = "preparando worktree…";
pub const STARTING_AGENT: &str = "iniciando o agente…";
pub const PICK_AGENT: &str = "escolha um agente disponível";
pub const START: &str = "Iniciar review";
pub const CANCEL: &str = "Cancelar";
pub const CANCEL_START: &str = "Cancelar início";
pub const CANCELLING: &str = "cancelando o início…";
pub const START_CANCELLED: &str = "Início cancelado — o worktree foi removido";
pub const CONFIGURE: &str = "Configurar";
pub const PR_CLOSED: &str = "Este PR está fechado";
pub const SERVICES_UNAVAILABLE: &str = "serviços em segundo plano indisponíveis";
pub const CI_HEADING: &str = "Jobs com falha:";
pub const NO_CI_LOG: &str = "(log vazio)";
pub const BUTTON_TOOLTIP: &str = "Revisar este PR com um agente de IA (a)";
pub const CLOSED_TOOLTIP: &str = "Disponível só para PRs abertos";

pub fn permissions_warning(agent: &str) -> String {
    format!(
        "O {agent} roda com as suas próprias permissões e configuração (ex.: comandos que você \
         liberou no {agent} rodam sem perguntar). O conteúdo do PR é tratado como não confiável \
         no prompt, mas confira o que o agente pedir no chat."
    )
}

pub fn context_line(repo: &str, number: u64, head: &str) -> String {
    format!("{repo} #{number} · {head}")
}

pub fn char_count(chars: usize, lines: usize) -> String {
    format!("{chars} caracteres · {lines} linhas")
}

pub fn context_failed(error: &str) -> String {
    format!("Falha ao montar o contexto: {error}")
}

pub fn ci_failed(error: &str) -> String {
    format!("Falha ao buscar o CI: {error}")
}

pub fn detection_failed(error: &str) -> String {
    format!("Falha ao detectar agentes: {error}")
}

pub fn templates_failed(error: &str) -> String {
    format!("Falha ao carregar templates: {error}")
}

pub fn render_failed(error: &str) -> String {
    format!("Falha ao renderizar o prompt: {error}")
}

pub fn launch_failed(error: &str) -> String {
    format!("Falha ao iniciar a review: {error}")
}

pub fn ci_log_heading(job: &str, lines: usize) -> String {
    format!("Últimas {lines} linhas do log de {job}:")
}

pub fn ci_log_unavailable(job: &str, error: &str) -> String {
    format!("Log de {job} indisponível: {error}")
}

pub fn no_models(agent: &str) -> String {
    format!("Nenhum modelo disponível — configure um provider no {agent}")
}

pub fn invalid_template(id: &str, error: &str) -> String {
    format!("Template inválido `{id}`: {error}")
}

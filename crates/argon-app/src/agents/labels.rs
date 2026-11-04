use argon_core::PermissionMode;

pub const DIALOG_TITLE: &str = "Revisar com IA";
pub const AGENT: &str = "Agente";
pub const GENERIC_AGENT: &str = "agente";
pub const MODEL: &str = "Modelo";
pub const TEMPLATE: &str = "Template";
pub const OPTIONS: &str = "Opções";
pub const PERMISSIONS: &str = "Permissões";
pub const FULL_ACCESS_RISK: &str = "Acesso total aprova automaticamente todo pedido de permissão do agente — comandos, edições e rede — sem mostrar o card. O agente lê código de PRs de terceiros: escolha este modo só se aceitar esse risco.";
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
pub const COPY_ERROR: &str = "Copiar";
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

pub const LOADING_USAGE: &str = "carregando uso…";
pub const USAGE_LINK: &str = "ver uso";
pub const USAGE_CONSENT_HINT: &str = "Autorize a leitura do login em Configurações para ver o uso";
pub const NO_USAGE_WINDOWS: &str = "sem dados de uso";
pub const LIMIT_REACHED: &str = "Limite atingido";
pub const RESETS_SOON: &str = "em instantes";

pub fn in_minutes(minutes: u64) -> String {
    format!("em {minutes} min")
}

pub fn in_hours(hours: u64) -> String {
    format!("em {hours} h")
}

pub fn in_days(days: u64) -> String {
    if days == 1 {
        "em 1 dia".to_owned()
    } else {
        format!("em {days} dias")
    }
}

pub fn renews(relative: &str) -> String {
    format!("renova {relative}")
}

pub fn limit_reached_renews(relative: &str) -> String {
    format!("{LIMIT_REACHED} — {}", renews(relative))
}

pub fn usage_summary(plan: &str, percent: u32, window: &str) -> String {
    let window = window.to_lowercase();
    let spoken = if window.starts_with(|c: char| c.is_ascii_digit()) {
        format!("{percent}% em {window}")
    } else {
        format!("{percent}% {window}")
    };
    if plan.is_empty() {
        spoken
    } else {
        format!("{plan} · {spoken}")
    }
}

pub fn permission_mode(mode: PermissionMode) -> &'static str {
    match mode {
        PermissionMode::Supervised => "Supervisionado",
        PermissionMode::AcceptEdits => "Aceitar edições",
        PermissionMode::FullAccess => "Acesso total",
    }
}

pub fn permission_mode_hint(mode: PermissionMode) -> &'static str {
    match mode {
        PermissionMode::Supervised => {
            "O Argon aprova sozinho só leituras e buscas dentro do worktree do PR; todo o resto pede confirmação."
        }
        PermissionMode::AcceptEdits => {
            "O Argon aprova sozinho leituras, buscas e edições dentro do worktree do PR; comandos, rede e qualquer caminho fora dele pedem confirmação."
        }
        PermissionMode::FullAccess => "Aprova todo pedido de permissão do agente sem perguntar.",
    }
}

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

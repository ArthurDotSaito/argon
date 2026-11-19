use argon_core::{MAX_RELATED_REPOS, PermissionMode, SessionKind};
use chrono::{Datelike as _, NaiveDate};

const MONTHS: [&str; 12] = [
    "jan", "fev", "mar", "abr", "mai", "jun", "jul", "ago", "set", "out", "nov", "dez",
];

pub const DIALOG_TITLE: &str = "Agent Review";
pub const AGENT_PLAN: &str = "Agent Plan";
pub const SUMMARIZE: &str = "Summarize";
pub const AGENT: &str = "Agente";
pub const GENERIC_AGENT: &str = "agente";
pub const MODEL: &str = "Modelo";
pub const TEMPLATE: &str = "Template";
pub const OPTIONS: &str = "Opções";
pub const PERMISSIONS: &str = "Permissões";
pub const FULL_ACCESS_RISK: &str = "Acesso total aprova automaticamente todo pedido de permissão do agente — comandos, edições e rede — sem mostrar o card. O agente lê código de PRs de terceiros: escolha este modo só se aceitar esse risco.";
pub const PLAN_MODE_LOCKED: &str = "Agent Plan roda só em Supervisionado";
pub const SUMMARY_MODE_LOCKED: &str =
    "Somente leitura: o agente não edita arquivos nem publica no GitHub.";
pub const PROJECT: &str = "Projeto";
pub const NO_OVERVIEW: &str = "Este repositório ainda não tem visão salva.";
pub const REFRESH_PROJECT: &str = "Reler o projeto inteiro";
pub const REFRESH_SAVED_HINT: &str =
    "Sobrescreve a visão salva. Sem reler, o agente parte dela e foca no PR.";
pub const REFRESH_FIRST_HINT: &str =
    "A primeira leitura demora mais e gasta mais tokens. A visão fica salva para os próximos PRs.";
pub const RELATED_PROJECTS: &str = "Projetos relacionados";
pub const RELATED_HINT: &str = "O Argon clona a branch padrão de cada um, só para leitura.";
pub const NO_RELATED: &str = "Nenhum projeto relacionado. Eles se configuram na página do projeto.";
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
pub const READ_ONLY: &str = "Somente leitura";
pub const READ_ONLY_HINT: &str = "Pedidos de edição do agente são negados";
pub const PLAN_DRAFT_ONLY: &str = "O plano fica em rascunho";
pub const PLAN_DRAFT_ONLY_HINT: &str = "Nada é publicado na issue sem o seu clique";
pub const PLAN_BASE_PENDING: &str = "branch padrão";
pub const PLAN_SHA_PENDING: &str = "definido ao iniciar";
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
pub const START_PLAN: &str = "Iniciar plano";
pub const START_SUMMARY: &str = "Iniciar resumo";
pub const CANCEL: &str = "Cancelar";
pub const COPY_ERROR: &str = "Copiar";
pub const CANCEL_START: &str = "Cancelar início";
pub const CANCELLING: &str = "cancelando o início…";
pub const START_CANCELLED: &str = "Início cancelado — o worktree foi removido";
pub const CONFIGURE: &str = "Configurar";
pub const PR_CLOSED: &str = "Este PR está fechado";
pub const ISSUE_CLOSED: &str = "Esta issue está fechada";
pub const SERVICES_UNAVAILABLE: &str = "serviços em segundo plano indisponíveis";
pub const CI_HEADING: &str = "Jobs com falha:";
pub const NO_CI_LOG: &str = "(log vazio)";
pub const BUTTON_TOOLTIP: &str = "Agent Review deste PR (a)";
pub const PLAN_BUTTON_TOOLTIP: &str = "Agent Plan desta issue (a)";
pub const CLOSED_TOOLTIP: &str = "Disponível só para PRs abertos";

pub const LOADING_USAGE: &str = "carregando uso…";
pub const USAGE_LINK: &str = "ver uso";
pub const USAGE_CONSENT_HINT: &str = "Autorize a leitura do login em Configurações para ver o uso";
pub const NO_USAGE_WINDOWS: &str = "sem dados de uso";
pub const LIMIT_REACHED: &str = "Limite atingido";
pub const RESETS_SOON: &str = "em instantes";

pub fn outdated(installed: &str, tested: &str) -> String {
    format!("v{installed} · atualize para {tested} ou mais nova")
}

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

pub fn permissions_warning(agent: &str, kind: SessionKind) -> String {
    let subject = match kind {
        SessionKind::Review | SessionKind::Summary => "do PR",
        SessionKind::Plan => "da issue",
    };
    format!(
        "O {agent} roda com as suas próprias permissões e configuração (ex.: comandos que você \
         liberou no {agent} rodam sem perguntar). O conteúdo {subject} é tratado como não \
         confiável no prompt, mas confira o que o agente pedir no chat."
    )
}

pub fn overview_saved(saved: NaiveDate, today: NaiveDate, source_pr: u64) -> String {
    let month = MONTHS[saved.month0() as usize];
    let date = if saved.year() == today.year() {
        format!("{} {month}", saved.day())
    } else {
        format!("{} {month} {}", saved.day(), saved.year())
    };
    format!("Visão salva em {date}, a partir do PR #{source_pr}.")
}

pub fn related_count(count: usize) -> String {
    format!("{count} de {MAX_RELATED_REPOS}")
}

pub fn plan_title(number: u64) -> String {
    format!("{AGENT_PLAN} · #{number}")
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

pub fn pin_to_repo(repo: &str) -> String {
    format!("Usar sempre em {repo}")
}

pub fn pinned_to_repo(repo: &str) -> String {
    format!("Usado sempre em {repo}")
}

pub fn render_failed(error: &str) -> String {
    format!("Falha ao renderizar o prompt: {error}")
}

pub fn launch_failed(kind: SessionKind, error: &str) -> String {
    match kind {
        SessionKind::Review => format!("Falha ao iniciar a review: {error}"),
        SessionKind::Plan => format!("Falha ao iniciar o plano: {error}"),
        SessionKind::Summary => format!("Falha ao iniciar o resumo: {error}"),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap()
    }

    #[test]
    fn saved_overview_names_the_day_and_the_source_pr() {
        assert_eq!(
            overview_saved(date(2026, 11, 12), date(2026, 11, 19), 198),
            "Visão salva em 12 nov, a partir do PR #198."
        );
        assert_eq!(
            overview_saved(date(2025, 3, 2), date(2026, 11, 19), 7),
            "Visão salva em 2 mar 2025, a partir do PR #7."
        );
    }

    #[test]
    fn related_count_shows_the_limit() {
        assert_eq!(related_count(3), "3 de 8");
    }
}

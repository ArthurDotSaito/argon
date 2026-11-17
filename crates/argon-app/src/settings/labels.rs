pub(super) use crate::agents::permission_mode_hint;

pub const ACCOUNTS: &str = "Contas";
pub const ORGS: &str = "Orgs rastreadas";
pub const AGENTS: &str = "Agentes";
pub const TEMPLATES: &str = "Templates de prompt";
pub const NOTIFICATIONS: &str = "Notificações";
pub const APPEARANCE: &str = "Aparência";
pub const ABOUT: &str = "Sobre";

pub const SAVE: &str = "Salvar";
pub const UNSAVED: &str = "Alterações não salvas";
pub const DISCARD: &str = "Descartar";
pub const SERVICES_UNAVAILABLE: &str = "serviços em segundo plano indisponíveis";

pub const ACCOUNTS_HINT: &str =
    "Contas do GitHub CLI (`gh`). O token fica só em memória e nunca é gravado pelo Argon.";
pub const NO_ACCOUNTS: &str = "Nenhuma conta encontrada no gh.";
pub const LOGIN_FIRST: &str = "gh auth login";
pub const CONNECTED: &str = "conectado";
pub const AUTHENTICATED: &str = "autenticado — aguardando o próximo sync";
pub const CHECKING: &str = "verificando…";
pub const RECHECK: &str = "Verificar de novo";

pub const ORGS_HINT: &str =
    "Só PRs e issues das orgs marcadas entram na inbox. O sync roda ao salvar.";
pub const NO_ORGS: &str = "Nenhuma org conhecida ainda — rode um sync com uma conta conectada.";
pub const ADMIN: &str = "admin";
pub const SAVE_AND_SYNC: &str = "Salvar e sincronizar";

pub const AGENTS_HINT: &str = "Overrides por agente. Campos vazios usam o padrão detectado. Salvar vale para a próxima review.";
pub const DETECTING: &str = "detectando…";
pub const NOT_FOUND: &str = "não encontrado no PATH";
pub const TEST: &str = "Testar";
pub const COMMAND: &str = "Comando";
pub const ARGS: &str = "Args";
pub const ENV: &str = "Env";
pub const DEFAULT_MODEL: &str = "Modelo padrão";
pub const AGENT_DEFAULT_MODEL: &str = "Padrão do agente";
pub const PERMISSION_MODE: &str = "Modo de permissão";
pub const USE_ACP: &str = "Usar adapter ACP (legado)";
pub const USE_ACP_NOTE: &str =
    "Roda este agente pelo adapter ACP em vez do driver nativo. Vale a partir da próxima sessão.";
pub const OVERRIDE_SAVED_FOR_ACP: &str = "Este comando foi salvo para o modo ACP e não vale para o driver nativo. Edite para usar no modo atual.";
pub const OVERRIDE_SAVED_FOR_NATIVE: &str = "Este comando foi salvo para o driver nativo e não vale para o modo ACP. Edite para usar no modo atual.";
pub const PLAN_AND_USAGE: &str = "Plano e uso";
pub const USAGE_NEEDS_AGENT: &str = "Uso disponível depois que o agente estiver instalado e logado";
pub const REFRESH_USAGE: &str = "Atualizar";
pub const READ_OPENCODE_LOGIN: &str = "Ler meu login do OpenCode para mostrar o uso do OpenCode Go";
pub const READ_OPENCODE_LOGIN_NOTE: &str = "O Argon lê o token salvo pelo OpenCode neste computador e consulta opencode.ai. O token não é salvo nem exibido.";
pub const ADD_ENV: &str = "Adicionar variável";
pub const ENV_KEY: &str = "CHAVE";
pub const ENV_VALUE: &str = "valor";
pub const REMOVE: &str = "Remover";
pub const DETECTED_AT: &str = "detectado em";

pub const TEMPLATES_HINT: &str =
    "Arquivos em prompts\\<id>.md. Clique numa variável para inseri-la no cursor.";
pub const NEW: &str = "Novo";
pub const DUPLICATE: &str = "Duplicar";
pub const DELETE: &str = "Excluir";
pub const RESTORE: &str = "Restaurar padrão";
pub const BUILT_IN: &str = "embutido";
pub const NAME: &str = "Nome";
pub const VARIABLES: &str = "Variáveis";
pub const NO_NONCE: &str =
    "Este template não usa {{nonce}} — os achados do agente não serão lidos.";
pub const NO_NONCE_PLAN: &str =
    "Este template não usa {{nonce}} — o plano do agente não será lido.";
pub const PREVIEW: &str = "Preview";
pub const PREVIEW_SAMPLE: &str = "Contexto de exemplo";
pub const PREVIEW_SAMPLE_ISSUE: &str = "Issue de exemplo";
pub const PREVIEW_LOADING: &str = "montando contexto do PR…";
pub const NO_TEMPLATE_SELECTED: &str = "Selecione um template.";
pub const LOADING_TEMPLATES: &str = "carregando templates…";
pub const DEFAULT_TEMPLATE: &str = "Template padrão";
pub const INCLUDE_DIFF: &str = "Incluir o diff no prompt";
pub const INCLUDE_DIFF_HINT: &str = "Desligado, o agente lê o diff no worktree. Ligado, diffs acima do limite também vão para o worktree.";
pub const DIFF_LIMIT: &str = "Limite (caracteres)";
pub const DISCARD_TITLE: &str = "Descartar alterações";
pub const DELETE_TITLE: &str = "Excluir template";
pub const RESTORE_TITLE: &str = "Restaurar template padrão";
pub const NEW_TEMPLATE_NAME: &str = "Novo template";
pub const NEW_TEMPLATE_BODY: &str =
    "Revise o PR {{ pr.title }} (#{{ pr.number }}) em {{ repo }}.\n";
pub const NEW_PROFILE: &str = "Novo perfil";
pub const NEW_PROFILE_NAME: &str = "Novo perfil";
pub const NEW_PROFILE_BODY: &str = "Neste repositório, preste atenção também em:\n- \n";
pub const PROFILE_BADGE: &str = "perfil";
pub const BASE: &str = "Base";
pub const PROFILE_TEXT: &str = "Só deste perfil";
pub const OWN_TEXT_START: &str = "Texto próprio · início";
pub const OWN_TEXT_END: &str = "Texto próprio · fim";
pub const SAVE_HERE_ONLY: &str = "Salvar só neste perfil";
pub const FOLLOW_AGAIN: &str = "Voltar a acompanhar";
pub const REPOSITORIES: &str = "Repositórios que usam este template";
pub const ADD_REPOSITORY: &str = "Adicionar repositório";
pub const REPOSITORIES_EMPTY: &str =
    "Nenhum repositório usa este template. Sem vínculo, vale o Template padrão.";

pub const NOTIFICATIONS_HINT: &str = "Toasts do Windows. Valem a partir do próximo evento.";
pub const NOTIFY_NEEDS_YOU: &str = "PR ou issue entrou em Needs you";
pub const NOTIFY_AGENTS: &str = "Agente pediu permissão ou terminou";

pub const APPEARANCE_HINT: &str = "Aplica na hora, sem reiniciar.";

pub const ABOUT_HINT: &str = "Versão instalada e atualizações do Argon.";
pub const APP_NAME: &str = "Argon";
pub const APP: &str = "Aplicativo";
pub const VERSION: &str = "Versão";
pub const LAST_CHECK: &str = "Última verificação";
pub const STATUS: &str = "Status";
pub const NEW_VERSION: &str = "Nova versão";
pub const CHECK_UPDATES: &str = "Verificar atualizações";
pub const WHATS_NEW: &str = "Ver novidades";
pub const UPDATE: &str = "Atualizar";
pub const UPDATE_CONFIRM_TITLE: &str = "Atualizar o Argon";
pub const CONFIRM: &str = "Confirmar";
pub const INSTALLED_APP_ONLY: &str = "Disponível só no app instalado";
pub const THEME: &str = "Tema";
pub const DENSITY: &str = "Densidade";
pub const ONE_DARK: &str = "One Dark";
pub const ONE_LIGHT: &str = "One Light";
pub const SYSTEM: &str = "Sistema";
pub const COMPACT: &str = "Compacta";
pub const COMFORTABLE: &str = "Confortável";

pub fn gh_login(host: &str) -> String {
    format!("gh auth login --hostname {host}")
}

pub fn not_authenticated(host: &str) -> String {
    format!("gh não autenticado — rode `{}`", gh_login(host))
}

pub fn update_stops_agents(count: usize) -> String {
    let running = if count == 1 {
        "Há 1 agente em execução".to_string()
    } else {
        format!("Há {count} agentes em execução")
    };
    format!("{running}. Atualizar agora encerra as sessões.")
}

pub fn checked_ago(ago: &str) -> String {
    format!("Verificado {ago}")
}

pub fn accounts_failed(error: &str) -> String {
    format!("Falha ao ler as contas do gh: {error}")
}

pub fn agent_default_model(model: &str) -> String {
    if model.is_empty() {
        AGENT_DEFAULT_MODEL.to_owned()
    } else {
        format!("{AGENT_DEFAULT_MODEL} ({model})")
    }
}

pub fn found(version: &str) -> String {
    format!("encontrado · v{version}")
}

pub fn found_outdated(version: &str, tested: &str) -> String {
    format!("encontrado · v{version} · versão antiga, atualize para {tested} ou mais nova")
}

pub fn detection_failed(error: &str) -> String {
    format!("Falha ao detectar: {error}")
}

pub fn save_failed(error: &str) -> String {
    format!("Falha ao salvar: {error}")
}

pub fn templates_failed(error: &str) -> String {
    format!("Falha ao carregar templates: {error}")
}

pub fn render_failed(error: &str) -> String {
    format!("Falha ao renderizar: {error}")
}

pub fn context_failed(error: &str) -> String {
    format!("Falha ao montar o contexto do PR: {error}")
}

pub fn preview_pr(repo: &str, number: u64, title: &str) -> String {
    format!("{repo}#{number} · {title}")
}

pub fn preview_size(chars: usize, lines: usize) -> String {
    format!("{chars} caracteres · {lines} linhas")
}

pub fn delete_message(name: &str) -> String {
    format!("Excluir o template \"{name}\"? O arquivo será apagado.")
}

pub fn restore_message(name: &str) -> String {
    format!("Descartar as edições de \"{name}\" e voltar ao conteúdo embutido?")
}

pub fn discard_message(name: &str) -> String {
    format!("\"{name}\" tem alterações não salvas. Descartar e trocar de template?")
}

pub fn copy_of(name: &str) -> String {
    format!("{name} (cópia)")
}

pub fn profile_row(base_name: &str, detached: bool) -> String {
    if detached {
        format!("texto próprio · base: {base_name}")
    } else {
        format!("base: {base_name}")
    }
}

pub fn from_base_start(base_name: &str) -> String {
    format!("Vem de {base_name} · início")
}

pub fn from_base_end(base_name: &str) -> String {
    format!("Vem de {base_name} · fim")
}

pub fn base_without_slot(base: &str) -> String {
    format!(
        "{base} não tem a linha {{# argon:perfil #}}: o texto deste perfil entra no fim do prompt, depois do formato da resposta. \"Restaurar padrão\" em {base} devolve a linha."
    )
}

pub fn base_failed(error: &str) -> String {
    format!("Base inválida: {error}")
}

pub fn base_edited(base_name: &str) -> String {
    format!("Você alterou um trecho que vem de {base_name}. Onde aplicar?")
}

pub fn save_in_base(base_name: &str) -> String {
    format!("Salvar em {base_name}, para todos")
}

pub fn save_in_base_hint(names: &str) -> String {
    if names.is_empty() {
        "Vale também para todo repositório sem perfil.".to_owned()
    } else {
        format!("Vale também para: {names} e para todo repositório sem perfil.")
    }
}

pub fn follow_again_message(base_name: &str) -> String {
    format!(
        "Os trechos de {base_name} voltam a ser os atuais. O texto \"{PROFILE_TEXT}\" é mantido."
    )
}

pub fn used_as_base(names: &str, has_slot: bool) -> String {
    if has_slot {
        format!("Base de: {names}. O texto de cada perfil entra na linha {{# argon:perfil #}}.")
    } else {
        format!(
            "Base de: {names}. Sem a linha {{# argon:perfil #}}, o texto de cada perfil entra no fim."
        )
    }
}

pub fn base_in_use(names: &str) -> String {
    format!("Este template é base de: {names}. Troque a base desses perfis antes de excluir.")
}

pub fn repo_elsewhere(repo: &str, template_name: &str) -> String {
    format!("{repo} · hoje: {template_name}")
}

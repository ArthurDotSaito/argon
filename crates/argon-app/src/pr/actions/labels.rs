pub const LOADING: &str = "Carregando workflows…";
pub const EMPTY: &str = "Nenhum workflow rodou neste commit";
pub const RETRY: &str = "Tentar de novo";
pub const RERUN_FAILED: &str = "Re-run failed jobs";
pub const RERUNNING: &str = "Re-rodando…";
pub const RERUN_TITLE: &str = "Re-run failed jobs";
pub const CANCEL: &str = "Cancelar";
pub const OPEN_ON_GITHUB: &str = "Abrir no GitHub";
pub const REFRESH: &str = "Atualizar workflows";
pub const QUEUED: &str = "na fila";
pub const NO_DURATION: &str = "—";
pub const SERVICES_UNAVAILABLE: &str = "serviços em segundo plano indisponíveis";

pub fn rerun_question(workflows: &str) -> String {
    format!("Re-rodar jobs com falha de {workflows}?")
}

pub fn rerun_hint(count: usize) -> String {
    if count == 1 {
        "Só os jobs com falha deste workflow rodam de novo no GitHub Actions.".to_string()
    } else {
        format!("Só os jobs com falha destes {count} workflows rodam de novo no GitHub Actions.")
    }
}

pub fn rerun_confirm(count: usize) -> String {
    if count == 1 {
        "Re-rodar".to_string()
    } else {
        format!("Re-rodar {count} workflows")
    }
}

pub fn load_failed(error: &str) -> String {
    format!("Falha ao carregar os workflows: {error}")
}

pub fn rerun_failed(error: &str) -> String {
    format!("Falha ao re-rodar os jobs: {error}")
}

pub fn attempt(attempt: u32) -> String {
    format!("#{attempt}")
}

pub fn running_for(duration: &str) -> String {
    format!("rodando há {duration}")
}

pub fn summary(count: usize, sha: &str) -> String {
    match count {
        1 => format!("1 workflow em {sha}"),
        count => format!("{count} workflows em {sha}"),
    }
}

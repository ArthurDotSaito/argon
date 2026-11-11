use argon_core::Severity;

use super::card_status::{CardStatus, SessionFilter};

pub const PANEL_TITLE: &str = "Agentes";
pub const REVIEW_PR: &str = "Nova review";
pub const OPEN_PR: &str = "Abrir PR";
pub const NO_SESSIONS: &str = "Nenhuma sessão de agente ainda.";
pub const NO_SESSIONS_IN_FILTER: &str = "Nenhuma sessão neste filtro.";
pub const LOADING_SESSIONS: &str = "carregando sessões…";
pub const SESSION_ENDED: &str = "Sessão encerrada — transcript somente leitura";
pub const SESSION_TAB_FALLBACK: &str = "Sessão";
pub const LOADING_TRANSCRIPT: &str = "carregando transcript…";
pub const THINKING: &str = "pensando…";
pub const THINKING_NOW: &str = "Pensando…";
pub const THOUGHT: &str = "Pensamento";
pub const SHOW_OUTPUT: &str = "mostrar saída";
pub const HIDE_OUTPUT: &str = "ocultar saída";
pub const SHOW_ALL: &str = "mostrar tudo";
pub const SHOW_LESS: &str = "mostrar menos";
pub const PLAN: &str = "Plano";
pub const PERMISSION_TITLE: &str = "O agente pede permissão";
pub const PERMISSION_EXPIRED: &str = "Pedido encerrado";
pub const LOGIN_TITLE: &str = "O agente precisa de login";
pub const COPY_COMMAND: &str = "Copiar comando";
pub const ADD_AS_DRAFT: &str = "Adicionar como comentário pendente";
pub const IGNORE: &str = "Ignorar";
pub const IGNORED: &str = "Ignorado";
pub const SHOW_IN_DIFF: &str = "Ver no diff";
pub const ADDED: &str = "Adicionado à review ✓";
pub const NO_FINDINGS: &str = "O agente não reportou achados.";
pub const SUGGESTION: &str = "sugestão";
pub const COMPOSER_PLACEHOLDER: &str = "Pergunte ao agente…  (Ctrl+Enter envia)";
pub const SEND: &str = "Enviar";
pub const STOP: &str = "Parar";
pub const REFERENCE: &str = "Referenciar a linha selecionada no diff";
pub const NO_LINE_SELECTED: &str = "Selecione uma linha no diff do PR para referenciar";
pub const DRAFTS_ONLY: &str = "só rascunhos";
pub const BUSY: &str = "Aguarde o turno atual terminar";
pub const PERMISSION_MODE_TOOLTIP: &str = "Modo de permissão desta sessão";
pub const PERMISSION_MODE_TURN_HINT: &str = "Ampliar permissões pode valer só a partir da próxima pergunta. No Codex, sair do Acesso total interrompe o turno.";
pub const FULL_ACCESS_TITLE: &str = "Ativar acesso total?";
pub const FULL_ACCESS_CONFIRM: &str = "Ativar acesso total";
pub use super::labels::SERVICES_UNAVAILABLE;
pub const UNKNOWN_PR: &str = "PR fora da inbox";
pub const YOU: &str = "você";
pub const CONTINUE: &str = "Continuar";
pub const OUT_OF_DIFF: &str = "Linha fora do diff — abra no diff e comente manualmente";
pub const OPEN_LINK_TITLE: &str = "Abrir link externo?";
pub const OPEN_LINK: &str = "Abrir";
pub const RETRY: &str = "Tentar continuar";
pub const RESUMING: &str = "retomando…";
pub const ARCHIVE: &str = "Arquivar";
pub const ARCHIVE_TOOLTIP: &str = "Arquivar sessão (remove o worktree, mantém o transcript)";
pub const ARCHIVE_TITLE: &str = "Arquivar sessão?";
pub const ARCHIVE_MESSAGE: &str = "O agente é encerrado e o worktree desta sessão é removido. O transcript continua em Arquivadas.";
pub const HEAD_MOVED: &str = "O PR recebeu commits novos desde esta sessão";
pub const NO_ACP_SESSION: &str = "esta sessão não tem id ACP para retomar";
pub const WORKTREE_GONE: &str = "o worktree desta sessão foi removido";
pub const SESSION_ARCHIVED: &str = "esta sessão foi arquivada";
pub const SESSION_MISSING: &str = "sessão não encontrada no banco local";

pub fn filter_label(filter: SessionFilter) -> &'static str {
    match filter {
        SessionFilter::All => "Todas as sessões",
        SessionFilter::Awaiting => "Aguardando você",
        SessionFilter::Running => "Rodando",
        SessionFilter::Completed => "Concluídas",
        SessionFilter::Archived => "Arquivadas",
    }
}

pub fn status_label(status: CardStatus) -> &'static str {
    match status {
        CardStatus::Running => "rodando",
        CardStatus::Awaiting => "aguardando você",
        CardStatus::Completed => "concluída",
        CardStatus::Failed => "erro",
        CardStatus::Archived => "arquivada",
    }
}

pub fn severity_label(severity: Severity) -> &'static str {
    match severity {
        Severity::High => "Alto",
        Severity::Medium => "Médio",
        Severity::Low => "Baixo",
    }
}

pub fn severity_short(severity: Severity) -> &'static str {
    match severity {
        Severity::High => "A",
        Severity::Medium => "M",
        Severity::Low => "B",
    }
}

pub fn permission_option(kind: &str, fallback: &str) -> String {
    match kind {
        "allow_once" => "Permitir uma vez".into(),
        "allow_always" => "Permitir nesta sessão".into(),
        "reject_once" => "Recusar".into(),
        "reject_always" => "Recusar sempre".into(),
        _ => fallback.into(),
    }
}

pub fn tool_kind(kind: &str) -> &'static str {
    match kind {
        "read" => "leitura",
        "edit" => "edição",
        "delete" => "exclusão",
        "move" => "mover",
        "search" => "busca",
        "execute" => "comando",
        "think" => "raciocínio",
        "fetch" => "rede",
        "switch_mode" => "modo",
        _ => "ferramenta",
    }
}

pub fn answered(label: &str) -> String {
    format!("Respondido: {label}")
}

pub fn thought_for(seconds: u64) -> String {
    format!("Pensou por {seconds}s")
}

pub fn interrupted(reason: &str) -> String {
    match reason {
        "cancelled" => "Turno interrompido por você".into(),
        "max_tokens" => "Turno encerrado: limite de tokens".into(),
        "max_turn_requests" => "Turno encerrado: limite de passos".into(),
        "refusal" => "O agente recusou continuar".into(),
        other => format!("Turno encerrado ({other})"),
    }
}

pub fn skipped_outside(count: usize) -> String {
    match count {
        1 => "1 achado descartado (fora dos arquivos do PR)".into(),
        _ => format!("{count} achados descartados (fora dos arquivos do PR)"),
    }
}

pub fn skipped_unreadable(count: usize) -> String {
    match count {
        1 => "1 achado descartado (formato ilegível)".into(),
        _ => format!("{count} achados descartados (formato ilegível)"),
    }
}

pub fn findings_heading(count: usize) -> String {
    match count {
        1 => "1 achado".into(),
        _ => format!("{count} achados"),
    }
}

pub fn archived_more(count: usize) -> String {
    match count {
        1 => "+1 sessão arquivada".into(),
        _ => format!("+{count} sessões arquivadas"),
    }
}

pub fn status_bar(running: usize, awaiting: usize) -> String {
    let agents = match running {
        1 => "1 agente rodando".to_owned(),
        _ => format!("{running} agentes rodando"),
    };
    format!("{agents} · {awaiting} aguardando você")
}

pub fn action_failed(error: &str) -> String {
    format!("Falhou: {error}")
}

pub fn location(path: &str, line: u32) -> String {
    let file = path.rsplit('/').next().unwrap_or(path);
    format!("{file}:{line}")
}

pub fn reference(path: &str, line: u32) -> String {
    format!("@{path}:{line} ")
}

pub fn worktree_invalid(error: &str) -> String {
    format!("worktree inválido: {error}")
}

pub fn unknown_transport(transport: &str) -> String {
    format!("transporte desconhecido nesta sessão: {transport}")
}

pub fn transport_unavailable(transport: &str) -> String {
    format!("este agente não roda mais pelo transporte `{transport}`")
}

pub fn resume_failed(error: &str) -> String {
    format!("Não consegui retomar: {error}")
}

pub fn blocked_image(alt: &str, url: &str) -> String {
    let alt = if alt.is_empty() {
        "sem descrição"
    } else {
        alt
    };
    format!("_imagem bloqueada: {alt}_ (`{url}`)")
}

pub fn blocked_link(url: &str) -> String {
    format!("Link bloqueado (só http/https): {url}")
}

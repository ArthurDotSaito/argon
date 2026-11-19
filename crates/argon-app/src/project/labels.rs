use argon_core::MAX_RELATED_REPOS;
use chrono::{DateTime, Datelike as _, Local, NaiveDate, NaiveDateTime, Timelike as _, Utc};

use crate::agents::MONTHS;
use crate::pr::labels::short_sha;

pub const PROJECT: &str = "Projeto";
pub const TAB_OVERVIEW: &str = "Visão geral";
pub const TAB_ARCHITECTURE: &str = "Arquitetura";
pub const NO_OVERVIEW: &str = "Sem visão salva";
pub const NO_SAVED_OVERVIEW: &str = "sem visão salva";
pub const EMPTY_TITLE: &str = "Ainda não há visão deste projeto";
pub const RELATED_TITLE: &str = "Projetos relacionados";
pub const RELATED_PLACEHOLDER: &str = "owner/nome";
pub const RELATED_NOTE: &str =
    "O Summarize clona a branch padrão de cada um para o agente ler junto com este projeto.";
pub const ADD: &str = "Adicionar";
pub const REMOVE: &str = "Remover";
pub const REFRESH_TITLE: &str = "Atualizar a visão";
pub const REFRESH_NOTE: &str = "Rode o Summarize em um PR deste repositório com “Reler o projeto” marcado. A visão nova substitui a atual.";
pub const NO_DIAGRAM: &str = "Esta visão não tem diagrama.";
pub const ZOOM_IN: &str = "Aumentar o zoom";
pub const ZOOM_OUT: &str = "Diminuir o zoom";
pub const ZOOM_FIT: &str = "Ajustar à tela";
pub const NODE_LINKS: &str = "Ligações";
pub const NODE_READING: &str = "Leitura";
pub const NODE_REPO: &str = "Repositório";
pub const NO_LINKS: &str = "Nenhuma ligação";
pub const READ: &str = "lido";
pub const RELATED_PROJECT: &str = "projeto relacionado";
pub const OPEN_PROJECT: &str = "Abrir projeto";
pub const CLOSE: &str = "Fechar";
pub const EDIT_IN_PROJECT: &str = "Editar na página do projeto";
pub const ERROR_EMPTY: &str = "Digite o repositório no formato owner/nome.";
pub const ERROR_FORMAT: &str = "Formato inválido. Use owner/nome, por exemplo acme/api.";
pub const ERROR_DUPLICATE: &str = "Este projeto já está na lista.";
pub const ERROR_OWN_REPO: &str = "Este é o próprio projeto, não precisa entrar na lista.";

pub fn error_limit() -> String {
    format!("A lista aceita no máximo {MAX_RELATED_REPOS} projetos.")
}

pub fn tab_title(repo: &str) -> String {
    format!("{PROJECT} · {}", repo_name(repo))
}

pub fn repo_name(repo: &str) -> &str {
    repo.rsplit_once('/').map_or(repo, |(_, name)| name)
}

pub fn repo_owner(repo: &str) -> &str {
    repo.split_once('/').map_or("", |(owner, _)| owner)
}

pub fn empty_explanation(repo: &str) -> String {
    format!(
        "Rode o Summarize em um PR de {repo}. Na primeira vez o agente lê o projeto inteiro e a visão fica salva aqui."
    )
}

pub fn related_counter(count: usize) -> String {
    format!("{count} de {MAX_RELATED_REPOS}")
}

pub fn load_failed(error: &str) -> String {
    format!("Falha ao carregar a visão do projeto: {error}")
}

pub fn short_date(saved: NaiveDate, today: NaiveDate) -> String {
    let month = MONTHS[saved.month0() as usize];
    if saved.year() == today.year() {
        format!("{} {month}", saved.day())
    } else {
        format!("{} {month} {}", saved.day(), saved.year())
    }
}

pub fn overview_note(saved_at: Option<DateTime<Utc>>, now: DateTime<Utc>) -> String {
    match saved_at {
        Some(saved_at) => saved_note(
            saved_at.with_timezone(&Local).date_naive(),
            now.with_timezone(&Local).date_naive(),
        ),
        None => NO_SAVED_OVERVIEW.to_owned(),
    }
}

pub fn saved_note(saved: NaiveDate, today: NaiveDate) -> String {
    format!("visão de {}", short_date(saved, today))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginLine {
    pub generated: String,
    pub from_pr: String,
    pub commit: String,
    pub agent: Option<String>,
}

pub fn origin_line(
    generated_at: NaiveDateTime,
    source_pr: u64,
    source_sha: &str,
    agent: Option<String>,
) -> OriginLine {
    let date = generated_at.date();
    let month = MONTHS[date.month0() as usize];
    OriginLine {
        generated: format!(
            "Visão gerada em {} {month} {}, {:02}:{:02}",
            date.day(),
            date.year(),
            generated_at.hour(),
            generated_at.minute()
        ),
        from_pr: format!("a partir do PR #{source_pr}"),
        commit: format!("@ {}", short_sha(source_sha)),
        agent,
    }
}

pub fn agent_label(agent: &str, model: Option<&str>) -> String {
    match model.map(str::trim).filter(|model| !model.is_empty()) {
        Some(model) => format!("{agent} · {model}"),
        None => agent.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap()
    }

    #[test]
    fn tab_title_uses_the_repo_name_without_owner() {
        assert_eq!(tab_title("acme/agenda-api"), "Projeto · agenda-api");
        assert_eq!(tab_title("solo"), "Projeto · solo");
    }

    #[test]
    fn origin_line_formats_date_pr_commit_and_agent() {
        let at = date(2026, 11, 19).and_hms_opt(10, 42, 0).unwrap();
        let line = origin_line(
            at,
            12,
            "3f9c2ab9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5",
            Some("Claude Code · Fable 5.2".into()),
        );
        assert_eq!(line.generated, "Visão gerada em 19 nov 2026, 10:42");
        assert_eq!(line.from_pr, "a partir do PR #12");
        assert_eq!(line.commit, "@ 3f9c2ab");
        assert_eq!(line.agent.as_deref(), Some("Claude Code · Fable 5.2"));
    }

    #[test]
    fn origin_line_pads_time_and_keeps_a_short_sha() {
        let at = date(2026, 1, 5).and_hms_opt(9, 5, 0).unwrap();
        let line = origin_line(at, 3, "abc", None);
        assert_eq!(line.generated, "Visão gerada em 5 jan 2026, 09:05");
        assert_eq!(line.commit, "@ abc");
        assert_eq!(line.agent, None);
    }

    #[test]
    fn saved_note_drops_the_year_only_in_the_current_year() {
        let today = date(2026, 11, 19);
        assert_eq!(saved_note(date(2026, 11, 3), today), "visão de 3 nov");
        assert_eq!(
            saved_note(date(2025, 12, 31), today),
            "visão de 31 dez 2025"
        );
    }

    #[test]
    fn overview_note_says_when_there_is_no_saved_overview() {
        assert_eq!(overview_note(None, Utc::now()), "sem visão salva");
    }

    #[test]
    fn overview_note_for_a_saved_overview_starts_with_visao_de() {
        let now = Utc::now();
        assert!(overview_note(Some(now), now).starts_with("visão de "));
    }

    #[test]
    fn agent_label_joins_agent_and_model() {
        assert_eq!(agent_label("Codex", Some("gpt")), "Codex · gpt");
        assert_eq!(agent_label("Codex", Some(" ")), "Codex");
        assert_eq!(agent_label("Codex", None), "Codex");
    }

    #[test]
    fn counter_shows_the_limit() {
        assert_eq!(related_counter(3), "3 de 8");
    }
}

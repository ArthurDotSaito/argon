use argon_core::{Mergeable, PrState, ReviewDecision};
use argon_github::PrDetail;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Div, IntoElement, ParentElement as _, SharedString,
    Styled as _, div, px,
};

use super::conversation::{Conversation, ConversationEvent};
use super::labels;
use super::readiness::{FailedCheck, Item, Level, Readiness, readiness};

const NAME_WIDTH: f32 = 84.;
const ROW_DIVIDER_OPACITY: f32 = 0.45;
const READY: &str = "Pronto para merge";
const NOT_READY: &str = "Ainda não dá para fazer merge";
const DRAFT: &str = "Rascunho";
const DRAFT_NOTE: &str = "Marcado como rascunho";
const CI: &str = "CI";
const REVIEWS: &str = "Reviews";
const THREADS: &str = "Conversas";
const BRANCH: &str = "Branch";
const VIEW_LOG: &str = "Ver log";
const RERUN: &str = "Rodar de novo";
const OPEN_THREAD: &str = "Abrir";
const YOU: &str = "você";

fn pending_note(pending: usize) -> String {
    match pending {
        1 => "· 1 pendência".to_string(),
        pending => format!("· {pending} pendências"),
    }
}

fn names(logins: &[String], viewer: Option<&str>) -> String {
    let shown: Vec<&str> = logins
        .iter()
        .map(|login| match viewer {
            Some(viewer) if login.eq_ignore_ascii_case(viewer) => YOU,
            _ => login.as_str(),
        })
        .collect();
    match shown.as_slice() {
        [] => String::new(),
        [only] => (*only).to_string(),
        [head @ .., last] => format!("{} e {last}", head.join(", ")),
    }
}

fn ci_text(
    level: Level,
    failed: &[FailedCheck],
    total: Option<usize>,
    running: usize,
    has_checks: bool,
) -> String {
    match level {
        Level::Blocked => {
            let Some(first) = failed.first() else {
                return "Checks falharam".to_string();
            };
            let mut runs: Vec<u64> = failed.iter().map(|check| check.run_id).collect();
            runs.dedup();
            let count = match (runs.len(), total) {
                (1, Some(total)) => format!("1 de {total} checks falhou"),
                (count, Some(total)) => format!("{count} de {total} checks falharam"),
                (1, None) => "1 check falhou".to_string(),
                (count, None) => format!("{count} checks falharam"),
            };
            let name = match &first.job {
                Some(job) => format!("{} / {job}", first.run),
                None => first.run.clone(),
            };
            format!("{count} {} {name}", labels::SEPARATOR)
        }
        Level::Waiting => match running {
            0 => "Checks em andamento".to_string(),
            1 => "1 check em andamento".to_string(),
            running => format!("{running} checks em andamento"),
        },
        Level::Ok => match total {
            Some(1) => "1 check passou".to_string(),
            Some(total) if total > 1 => format!("{total} checks passaram"),
            _ if has_checks => "Checks passaram".to_string(),
            _ => "Sem checks".to_string(),
        },
    }
}

fn reviews_text(
    level: Level,
    decision: Option<ReviewDecision>,
    pending: &[String],
    yours: bool,
    blocking: &[String],
    viewer: Option<&str>,
) -> String {
    let head = match level {
        Level::Blocked if blocking.is_empty() => "Mudanças pedidas".to_string(),
        Level::Blocked => format!("Mudanças pedidas por {}", names(blocking, viewer)),
        Level::Waiting if decision == Some(ReviewDecision::ReviewRequired) => {
            "Review obrigatória pendente".to_string()
        }
        Level::Waiting if yours => "Sua review foi pedida".to_string(),
        Level::Waiting => "Nenhuma aprovação ainda".to_string(),
        Level::Ok => "Aprovado".to_string(),
    };
    if pending.is_empty() {
        return head;
    }
    format!(
        "{head} {} pendentes: {}",
        labels::SEPARATOR,
        names(pending, viewer)
    )
}

fn threads_text(open: usize, first: Option<&(String, Option<u32>)>) -> String {
    let count = match open {
        0 => return "Nenhuma conversa em aberto".to_string(),
        1 => "1 em aberto".to_string(),
        open => format!("{open} em aberto"),
    };
    match first {
        Some((path, line)) => format!(
            "{count} {} {}",
            labels::SEPARATOR,
            labels::line_ref(path, *line)
        ),
        None => count,
    }
}

fn branch_text(mergeable: Mergeable, base: &str) -> String {
    match mergeable {
        Mergeable::Mergeable => format!("Sem conflitos com {base}"),
        Mergeable::Conflicting => format!("Conflitos com {base}"),
        Mergeable::Unknown => format!("O GitHub ainda está verificando conflitos com {base}"),
    }
}

fn failed_step(step: &str) -> String {
    format!("falhou no passo \"{step}\"")
}

impl Conversation {
    pub(super) fn readiness_card(&self, detail: &PrDetail, cx: &Context<Self>) -> Option<Div> {
        let pr = &detail.pr;
        if pr.state != PrState::Open {
            return None;
        }
        let viewer = self.viewer.as_deref();
        let runs = self.runs.as_deref().map(Vec::as_slice);
        let state = readiness(pr, detail, runs, viewer);
        let theme = cx.theme();
        let rows = state.items.iter().map(|item| match item {
            Item::Draft => row(
                Level::Waiting,
                DRAFT,
                DRAFT_NOTE.into(),
                None,
                Vec::new(),
                cx,
            ),
            Item::Ci {
                level,
                failed,
                total,
                running,
            } => {
                let text = ci_text(*level, failed, *total, *running, pr.checks.is_some());
                let note = failed.first().and_then(|first| {
                    self.error_line
                        .clone()
                        .or_else(|| first.step.as_deref().map(|step| failed_step(step).into()))
                });
                let mut actions = Vec::new();
                if !failed.is_empty() {
                    actions.push(self.action("pr-ready-log", VIEW_LOG, false, cx, |cx| {
                        cx.emit(ConversationEvent::OpenChecks)
                    }));
                    actions.push(self.action("pr-ready-rerun", RERUN, false, cx, |cx| {
                        cx.emit(ConversationEvent::RerunFailed)
                    }));
                }
                row(*level, CI, text.into(), note, actions, cx)
            }
            Item::Reviews {
                level,
                decision,
                pending,
                yours,
                blocking,
            } => {
                let text = reviews_text(*level, *decision, pending, *yours, blocking, viewer);
                let actions = yours
                    .then(|| {
                        self.action("pr-ready-review", labels::REVIEW, true, cx, |cx| {
                            cx.emit(ConversationEvent::StartReview)
                        })
                    })
                    .into_iter()
                    .collect();
                row(*level, REVIEWS, text.into(), None, actions, cx)
            }
            Item::Threads { level, open, first } => {
                let text = threads_text(*open, first.as_ref());
                let actions = first
                    .as_ref()
                    .and_then(|(path, line)| Some((path.clone(), (*line)?)))
                    .map(|(path, line)| {
                        let side = detail
                            .threads
                            .iter()
                            .find(|thread| !thread.resolved && !thread.outdated)
                            .map(|thread| thread.side);
                        self.action("pr-ready-thread", OPEN_THREAD, false, cx, move |cx| {
                            if let Some(side) = side {
                                cx.emit(ConversationEvent::Reveal {
                                    path: path.clone(),
                                    line,
                                    side,
                                })
                            }
                        })
                    })
                    .into_iter()
                    .collect();
                row(*level, THREADS, text.into(), None, actions, cx)
            }
            Item::Branch { level, mergeable } => {
                let text = branch_text(*mergeable, &pr.base);
                row(*level, BRANCH, text.into(), None, Vec::new(), cx)
            }
        });
        Some(
            v_flex()
                .w_full()
                .overflow_hidden()
                .border_1()
                .border_color(theme.border)
                .rounded(theme.radius_lg)
                .child(title(&state, cx))
                .children(rows),
        )
    }

    fn action(
        &self,
        id: &'static str,
        label: &'static str,
        primary: bool,
        cx: &Context<Self>,
        on_click: impl Fn(&mut Context<Self>) + 'static,
    ) -> AnyElement {
        let button = Button::new(id).label(label).xsmall();
        let button = if primary {
            button.primary()
        } else {
            button.outline()
        };
        button
            .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| on_click(cx)))
            .into_any_element()
    }
}

fn title(state: &Readiness, cx: &App) -> Div {
    let theme = cx.theme();
    let worst = state
        .items
        .iter()
        .filter(|item| item.blocks_merge())
        .map(Item::level)
        .max();
    let (icon, color, text) = match worst {
        Some(Level::Blocked) => (IconName::CircleX, theme.danger, NOT_READY),
        Some(Level::Waiting) => (IconName::Clock, theme.warning, NOT_READY),
        _ => (IconName::CircleCheck, theme.success, READY),
    };
    h_flex()
        .w_full()
        .px_3()
        .py_2()
        .gap_2()
        .items_center()
        .bg(theme.muted)
        .text_sm()
        .child(Icon::new(icon).small().text_color(color))
        .child(
            div()
                .font_semibold()
                .text_color(theme.foreground)
                .child(text),
        )
        .children((state.pending > 0).then(|| {
            div()
                .text_color(theme.muted_foreground)
                .child(pending_note(state.pending))
        }))
}

fn row(
    level: Level,
    name: &'static str,
    text: SharedString,
    note: Option<SharedString>,
    actions: Vec<AnyElement>,
    cx: &App,
) -> Div {
    let theme = cx.theme();
    let (icon, color) = match level {
        Level::Ok => (IconName::Check, theme.success),
        Level::Waiting => (IconName::Clock, theme.warning),
        Level::Blocked => (IconName::X, theme.danger),
    };
    h_flex()
        .w_full()
        .px_3()
        .py_2()
        .gap_3()
        .items_center()
        .border_t_1()
        .border_color(theme.border.opacity(ROW_DIVIDER_OPACITY))
        .text_sm()
        .child(Icon::new(icon).small().text_color(color))
        .child(
            div()
                .w(px(NAME_WIDTH))
                .flex_none()
                .font_medium()
                .text_color(theme.foreground)
                .child(name),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .child(
                    div()
                        .truncate()
                        .text_color(theme.muted_foreground)
                        .child(text),
                )
                .children(note.map(|note| {
                    div()
                        .truncate()
                        .font_family(theme.mono_font_family.clone())
                        .text_xs()
                        .text_color(theme.danger)
                        .child(note)
                })),
        )
        .child(h_flex().flex_none().gap_1p5().children(actions))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failed(run_id: u64, run: &str, job: Option<&str>) -> FailedCheck {
        FailedCheck {
            run_id,
            job_id: job.map(|_| 10),
            run: run.into(),
            job: job.map(Into::into),
            step: None,
        }
    }

    fn logins(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_string()).collect()
    }

    #[test]
    fn names_join_naturally_and_call_the_viewer_you() {
        assert_eq!(names(&logins(&[]), None), "");
        assert_eq!(names(&logins(&["ana"]), None), "ana");
        assert_eq!(
            names(&logins(&["vitor", "ana", "Arthur"]), Some("arthur")),
            "vitor, ana e você"
        );
    }

    #[test]
    fn ci_text_names_the_first_failure() {
        let checks = [failed(1, "Version Gate", Some("check-nuget-version"))];
        assert_eq!(
            ci_text(Level::Blocked, &checks, Some(7), 0, true),
            "1 de 7 checks falhou · Version Gate / check-nuget-version"
        );
        let two = [failed(1, "Deploy", None), failed(2, "Tests", Some("unit"))];
        assert_eq!(
            ci_text(Level::Blocked, &two, Some(4), 0, true),
            "2 de 4 checks falharam · Deploy"
        );
        assert_eq!(
            ci_text(Level::Blocked, &[], None, 0, true),
            "Checks falharam"
        );
    }

    #[test]
    fn ci_text_counts_workflows_not_the_jobs_of_a_matrix() {
        let matrix = [
            failed(1, "CI", Some("test (ubuntu)")),
            failed(1, "CI", Some("test (windows)")),
            failed(1, "CI", Some("test (macos)")),
        ];
        assert_eq!(
            ci_text(Level::Blocked, &matrix, Some(1), 0, true),
            "1 de 1 checks falhou · CI / test (ubuntu)"
        );
    }

    #[test]
    fn ci_text_covers_waiting_and_passing() {
        assert_eq!(
            ci_text(Level::Waiting, &[], Some(3), 2, true),
            "2 checks em andamento"
        );
        assert_eq!(
            ci_text(Level::Waiting, &[], None, 0, true),
            "Checks em andamento"
        );
        assert_eq!(
            ci_text(Level::Ok, &[], Some(4), 0, true),
            "4 checks passaram"
        );
        assert_eq!(ci_text(Level::Ok, &[], None, 0, true), "Checks passaram");
        assert_eq!(ci_text(Level::Ok, &[], None, 0, false), "Sem checks");
    }

    #[test]
    fn reviews_text_follows_the_level() {
        let pending = logins(&["vitor", "me"]);
        assert_eq!(
            reviews_text(
                Level::Waiting,
                Some(ReviewDecision::ReviewRequired),
                &pending,
                true,
                &[],
                Some("me")
            ),
            "Review obrigatória pendente · pendentes: vitor e você"
        );
        assert_eq!(
            reviews_text(
                Level::Waiting,
                None,
                &logins(&["me"]),
                true,
                &[],
                Some("me")
            ),
            "Sua review foi pedida · pendentes: você"
        );
        assert_eq!(
            reviews_text(Level::Waiting, None, &[], false, &[], None),
            "Nenhuma aprovação ainda"
        );
        assert_eq!(
            reviews_text(Level::Blocked, None, &[], false, &logins(&["ana"]), None),
            "Mudanças pedidas por ana"
        );
        assert_eq!(
            reviews_text(
                Level::Ok,
                Some(ReviewDecision::Approved),
                &logins(&["vitor"]),
                false,
                &[],
                None
            ),
            "Aprovado · pendentes: vitor"
        );
    }

    #[test]
    fn threads_text_points_at_the_first_open_one() {
        assert_eq!(threads_text(0, None), "Nenhuma conversa em aberto");
        let first = ("src/cache.rs".to_string(), Some(42));
        assert_eq!(
            threads_text(2, Some(&first)),
            "2 em aberto · src/cache.rs:42"
        );
    }
}

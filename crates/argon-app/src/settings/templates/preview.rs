use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use argon_core::{
    PlanComment, PlanContext, PrKey, PromptContext, PromptError, PromptTemplate, PullRequest,
    TemplateKind, render_plan_prompt, render_prompt, resolve_template,
};
use chrono::DateTime;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::select::{Select, SelectState};
use gpui_kit::component::{ActiveTheme as _, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, AppContext as _, Context, Div, Entity, IntoElement, ParentElement as _,
    SharedString, Styled as _, Task, UniformListScrollHandle, Window, div, px, uniform_list,
};

use super::{Load, TemplatesSection};
use crate::agents::{Choice, ContextOptions, Gathered, gather, prompt_context};
use crate::settings::labels;
use crate::settings::recent_prs;
use crate::settings::ui::{card, note, services};

const SAMPLE_ID: &str = "sample";
const SAMPLE_COMMENT_AT: i64 = 1_794_660_000;
const PREVIEW_HEIGHT: f32 = 300.;
const ROW_HEIGHT: f32 = 17.;
const TARGET_MAX_WIDTH: f32 = 420.;

enum PreviewInput {
    Review(PromptContext),
    Plan(PlanContext),
}

pub(super) struct Rendered {
    lines: Vec<SharedString>,
    chars: usize,
    warnings: Vec<SharedString>,
    error: Option<SharedString>,
}

pub(super) struct PreviewState {
    pub(super) target_select: Entity<SelectState<Vec<Choice>>>,
    targets: Vec<PullRequest>,
    target: Option<PrKey>,
    contexts: HashMap<PrKey, Load<Arc<Gathered>>>,
    rendered: Option<Rendered>,
    epoch: u64,
    render_task: Option<Task<()>>,
    gather_tasks: Vec<Task<()>>,
    scroll: UniformListScrollHandle,
}

impl PreviewState {
    pub(super) fn new(window: &mut Window, cx: &mut Context<TemplatesSection>) -> Self {
        Self {
            target_select: cx.new(|cx| SelectState::new(Vec::<Choice>::new(), None, window, cx)),
            targets: Vec::new(),
            target: None,
            contexts: HashMap::new(),
            rendered: None,
            epoch: 0,
            render_task: None,
            gather_tasks: Vec::new(),
            scroll: UniformListScrollHandle::new(),
        }
    }
}

fn target_id(key: &PrKey) -> String {
    format!("{}|{}", key.host, key.id.0)
}

pub fn sample_context() -> PromptContext {
    PromptContext {
        pr_title: "feat(api): paginação no endpoint de agendamentos".into(),
        pr_number: 482,
        pr_url: "https://github.com/exemplo/scheduling-api/pull/482".into(),
        pr_body: "Adiciona paginação por cursor em GET /appointments.".into(),
        repo: "exemplo/scheduling-api".into(),
        base: "main".into(),
        base_sha: Some("4f2c9e1".into()),
        head: "feat/pagination".into(),
        head_sha: "9b1d7a3c5e2f4086a1b3c4d5e6f708192a3b4c5d".into(),
        diff: "diff --git a/src/appointments.ts b/src/appointments.ts\n--- a/src/appointments.ts\n+++ b/src/appointments.ts\n@@ -10,3 +10,5 @@\n-  return db.all();\n+  const page = await db.page(cursor, limit);\n+  return page;\n".into(),
        files: vec!["src/appointments.ts".into(), "src/db.ts".into()],
        file_stats: vec![
            ("src/appointments.ts".into(), 2, 1),
            ("src/db.ts".into(), 14, 0),
        ],
        previous_reviews: String::new(),
        ci_failures: String::new(),
    }
}

fn sample_plan_context() -> PlanContext {
    PlanContext {
        issue_title: "Agendamento duplicado ao confirmar duas vezes".into(),
        issue_number: 517,
        issue_url: "https://github.com/exemplo/scheduling-api/issues/517".into(),
        issue_body:
            "Com dois cliques rápidos em Confirmar, o mesmo horário é reservado duas vezes.".into(),
        repo: "exemplo/scheduling-api".into(),
        labels: vec!["bug".into(), "agendamento".into()],
        assignees: vec!["maria".into()],
        milestone: Some("Sprint 42".into()),
        comments: vec![PlanComment {
            author: "joao".into(),
            created_at: DateTime::from_timestamp(SAMPLE_COMMENT_AT, 0).unwrap_or_default(),
            body: "Acontece também pelo app, não só na web.".into(),
        }],
        base: "main".into(),
        base_sha: "4f2c9e1".into(),
    }
}

fn failed(error: SharedString) -> Rendered {
    Rendered {
        lines: Vec::new(),
        chars: 0,
        warnings: Vec::new(),
        error: Some(error),
    }
}

fn render_preview(
    template: &PromptTemplate,
    input: &PreviewInput,
    max_diff_chars: usize,
) -> Rendered {
    let result = match input {
        PreviewInput::Review(context) => render_prompt(template, context, max_diff_chars),
        PreviewInput::Plan(context) => render_plan_prompt(template, context),
    };
    match result {
        Ok(prompt) => Rendered {
            lines: prompt
                .text
                .lines()
                .map(|line| SharedString::from(line.to_owned()))
                .collect(),
            chars: prompt.text.chars().count(),
            warnings: prompt
                .warnings
                .into_iter()
                .map(SharedString::from)
                .collect(),
            error: None,
        },
        Err(error) => failed(labels::render_failed(&error.to_string()).into()),
    }
}

impl TemplatesSection {
    pub(super) fn refresh_targets(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let targets = recent_prs::open_prs(cx);
        let mut choices: Vec<Choice> = targets
            .iter()
            .map(|pr| {
                Choice::new(
                    target_id(&pr.key()),
                    labels::preview_pr(&pr.repo, pr.number, &pr.title),
                )
            })
            .collect();
        choices.push(Choice::new(SAMPLE_ID, labels::PREVIEW_SAMPLE));
        let keep = self
            .preview
            .target
            .clone()
            .filter(|key| targets.iter().any(|pr| &pr.key() == key));
        let target = keep.or_else(|| targets.first().map(PullRequest::key));
        let selected = target
            .as_ref()
            .map_or_else(|| SAMPLE_ID.to_owned(), target_id);
        self.preview.target_select.update(cx, |select, cx| {
            select.set_items(choices, window, cx);
            select.set_selected_value(&selected, window, cx);
        });
        self.preview.targets = targets;
        self.apply_target(target, cx);
    }

    pub(super) fn set_target(&mut self, id: &str, cx: &mut Context<Self>) {
        let target = self
            .preview
            .targets
            .iter()
            .map(PullRequest::key)
            .find(|key| target_id(key) == id);
        self.apply_target(target, cx);
    }

    fn apply_target(&mut self, target: Option<PrKey>, cx: &mut Context<Self>) {
        self.preview.target = target.clone();
        if let Some(key) = target
            && !self.preview.contexts.contains_key(&key)
        {
            self.gather(key, cx);
        }
        self.rerender(cx);
    }

    fn gather(&mut self, key: PrKey, cx: &mut Context<Self>) {
        let (Some(services), Some(pr)) = (
            services(cx),
            self.preview
                .targets
                .iter()
                .find(|pr| pr.key() == key)
                .cloned(),
        ) else {
            return;
        };
        self.preview.contexts.insert(key.clone(), Load::Loading);
        let task = gather(&services, &pr, None, cx);
        self.preview
            .gather_tasks
            .push(cx.spawn(async move |this, cx| {
                let result = task.await;
                _ = this.update(cx, |this, cx| {
                    let loaded = match result {
                        Ok(gathered) => Load::Ready(Arc::new(gathered)),
                        Err(error) => {
                            Load::Failed(labels::context_failed(&error.to_string()).into())
                        }
                    };
                    this.preview.contexts.insert(key, loaded);
                    this.rerender(cx);
                });
            }));
    }

    fn preview_context(&self) -> Result<Option<PromptContext>, SharedString> {
        let max_diff_chars = self.config.review.max_diff_chars;
        let Some(key) = &self.preview.target else {
            return Ok(Some(sample_context()));
        };
        match self.preview.contexts.get(key) {
            Some(Load::Ready(gathered)) => {
                let options = ContextOptions {
                    include_reviews: self.config.review.include_previous_reviews,
                    include_ci: false,
                };
                Ok(Some(prompt_context(
                    gathered,
                    None,
                    options,
                    max_diff_chars,
                )))
            }
            Some(Load::Failed(error)) => Err(error.clone()),
            Some(Load::Loading) | None => Ok(None),
        }
    }

    fn preview_input(
        &self,
        kind: Option<TemplateKind>,
    ) -> Result<Option<PreviewInput>, SharedString> {
        match kind {
            Some(TemplateKind::Plan) => Ok(Some(PreviewInput::Plan(sample_plan_context()))),
            Some(TemplateKind::Review) | None => self
                .preview_context()
                .map(|context| context.map(PreviewInput::Review)),
        }
    }

    pub(super) fn resolved_draft(
        &self,
        cx: &Context<Self>,
    ) -> Option<Result<PromptTemplate, PromptError>> {
        let draft = self.preview_draft(cx)?;
        let mut all = self.saved_templates().to_vec();
        if let Some(saved) = all.iter_mut().find(|template| template.id == draft.id) {
            *saved = draft.clone();
        }
        Some(resolve_template(&draft, &all))
    }

    pub(super) fn rerender(&mut self, cx: &mut Context<Self>) {
        self.preview.epoch += 1;
        let epoch = self.preview.epoch;
        let input = self.preview_input(self.current().map(|template| template.kind));
        let (template, input) = match (self.resolved_draft(cx), input) {
            (Some(Ok(template)), Ok(Some(input))) => (template, input),
            (resolved, input) => {
                let resolve_error = resolved
                    .and_then(Result::err)
                    .map(|error| SharedString::from(labels::render_failed(&error.to_string())));
                self.preview.rendered = resolve_error.or_else(|| input.err()).map(failed);
                self.preview.render_task = None;
                cx.notify();
                return;
            }
        };
        let max_diff_chars = self.config.review.max_diff_chars;
        let job =
            cx.background_spawn(async move { render_preview(&template, &input, max_diff_chars) });
        self.preview.render_task = Some(cx.spawn(async move |this, cx| {
            let rendered = job.await;
            _ = this.update(cx, |this, cx| {
                if this.preview.epoch == epoch {
                    this.preview.rendered = Some(rendered);
                    this.preview.render_task = None;
                    cx.notify();
                }
            });
        }));
        cx.notify();
    }

    fn preview_rows(&self, range: Range<usize>, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme();
        let Some(rendered) = &self.preview.rendered else {
            return Vec::new();
        };
        range
            .filter_map(|ix| rendered.lines.get(ix))
            .map(|line| {
                div()
                    .h(px(ROW_HEIGHT))
                    .px_2()
                    .whitespace_nowrap()
                    .truncate()
                    .text_color(theme.foreground)
                    .child(line.clone())
                    .into_any_element()
            })
            .collect()
    }

    pub(super) fn preview_pane(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let rendered = self.preview.rendered.as_ref();
        let lines = rendered.map_or(0, |rendered| rendered.lines.len());
        let size = rendered.filter(|_| lines > 0).map(|rendered| {
            note(
                labels::preview_size(rendered.chars, lines),
                theme.muted_foreground,
            )
        });
        let notes: Vec<Div> = rendered
            .map(|rendered| {
                rendered
                    .warnings
                    .iter()
                    .map(|warning| note(warning.clone(), theme.warning))
                    .chain(
                        rendered
                            .error
                            .iter()
                            .map(|error| note(error.clone(), theme.danger)),
                    )
                    .collect()
            })
            .unwrap_or_default();
        let failed = rendered.is_some_and(|rendered| rendered.error.is_some());
        let plan = self
            .current()
            .is_some_and(|template| template.kind == TemplateKind::Plan);
        let target: AnyElement = if plan {
            div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(labels::PREVIEW_SAMPLE_ISSUE)
                .into_any_element()
        } else {
            Select::new(&self.preview.target_select)
                .small()
                .into_any_element()
        };
        let body: AnyElement = if lines == 0 {
            div()
                .p_2()
                .text_color(theme.muted_foreground)
                .children((!failed).then_some(labels::PREVIEW_LOADING))
                .into_any_element()
        } else {
            div()
                .relative()
                .size_full()
                .child(
                    uniform_list(
                        "settings-template-preview",
                        lines,
                        cx.processor(|this, range: Range<usize>, _, cx| {
                            this.preview_rows(range, cx)
                        }),
                    )
                    .track_scroll(&self.preview.scroll)
                    .size_full(),
                )
                .vertical_scrollbar(&self.preview.scroll)
                .into_any_element()
        };
        card(cx)
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .child(div().text_sm().font_semibold().child(labels::PREVIEW))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .max_w(px(TARGET_MAX_WIDTH))
                            .child(target),
                    )
                    .children(size),
            )
            .children(notes)
            .child(
                v_flex()
                    .h(px(PREVIEW_HEIGHT))
                    .rounded(theme.radius)
                    .border_1()
                    .border_color(theme.border)
                    .py_1()
                    .font_family(theme.mono_font_family.clone())
                    .text_xs()
                    .child(body),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::super::naming::{variable_token, variables};
    use super::*;

    fn chip_template(kind: TemplateKind) -> PromptTemplate {
        PromptTemplate {
            kind,
            body: variables(kind)
                .iter()
                .map(|name| variable_token(name))
                .collect::<Vec<_>>()
                .join("\n"),
            ..PromptTemplate::default()
        }
    }

    #[test]
    fn every_variable_chip_renders_with_the_sample_context_of_its_kind() {
        let review = render_preview(
            &chip_template(TemplateKind::Review),
            &PreviewInput::Review(sample_context()),
            1000,
        );
        let plan = render_preview(
            &chip_template(TemplateKind::Plan),
            &PreviewInput::Plan(sample_plan_context()),
            0,
        );

        assert_eq!(review.error, None);
        assert_eq!(plan.error, None);
        assert!(
            plan.lines
                .iter()
                .any(|line| line.contains("Agendamento duplicado"))
        );
    }

    #[test]
    fn review_chips_do_not_render_in_a_plan_preview() {
        let rendered = render_preview(
            &chip_template(TemplateKind::Review),
            &PreviewInput::Plan(sample_plan_context()),
            0,
        );

        assert!(rendered.error.is_some());
        assert!(rendered.lines.is_empty());
    }
}

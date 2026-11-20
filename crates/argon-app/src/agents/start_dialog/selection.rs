use argon_core::{
    AgentCfg, AgentKind, PermissionMode, PromptError, PromptTemplate, RelatedRepo, RenderedPrompt,
    SessionKind, TemplateKind, render_plan_prompt, render_prompt, render_summary_prompt, repo_key,
    resolve_template,
};
use argon_store::ProjectOverview;
use gpui_kit::{AppContext as _, Context, SharedString, Task, Window};

use super::{Gathering, Load, Preview, StartDialog};
use crate::agents::choice::Choice;
use crate::agents::context::{
    ContextOptions, SummaryGathering, diff_inline, plan_context, planned_related, preview_context,
    prompt_context,
};
use crate::agents::detection::{
    AgentOption, default_agent, default_model, default_template, model_choices,
};
use crate::agents::labels;
use crate::config_watch;

pub(super) fn template_kind(kind: SessionKind) -> TemplateKind {
    match kind {
        SessionKind::Review => TemplateKind::Review,
        SessionKind::Plan => TemplateKind::Plan,
        SessionKind::Summary => TemplateKind::Summary,
    }
}

pub(super) fn templates_of_kind(
    templates: Vec<PromptTemplate>,
    kind: TemplateKind,
) -> Vec<PromptTemplate> {
    templates
        .into_iter()
        .filter(|template| template.kind == kind)
        .collect()
}

pub(super) fn session_permission_mode(
    kind: SessionKind,
    requested: PermissionMode,
) -> PermissionMode {
    match kind {
        SessionKind::Review => requested,
        SessionKind::Plan | SessionKind::Summary => PermissionMode::Supervised,
    }
}

fn preview(rendered: Result<RenderedPrompt, SharedString>, diff_inline: Option<bool>) -> Preview {
    let (lines, chars) = match &rendered {
        Ok(prompt) => (
            prompt
                .text
                .lines()
                .map(|line| SharedString::from(line.to_owned()))
                .collect(),
            prompt.text.chars().count(),
        ),
        Err(_) => (Vec::new(), 0),
    };
    Preview {
        rendered,
        lines,
        chars,
        diff_inline,
    }
}

fn render_error(error: &PromptError) -> SharedString {
    labels::render_failed(&error.to_string()).into()
}

impl StartDialog {
    pub(super) fn set_agents(
        &mut self,
        options: Vec<AgentOption>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let last = Self::sessions(cx).and_then(|sessions| sessions.read(cx).last_agent());
        let chosen = default_agent(&options, last);
        self.agents = Load::Ready(options);
        match chosen {
            Some(kind) => self.select_agent(kind, window, cx),
            None => cx.notify(),
        }
    }

    pub(super) fn selected_option(&self) -> Option<&AgentOption> {
        let kind = self.agent?;
        self.agents
            .ready()?
            .iter()
            .find(|option| option.spec.kind == kind)
    }

    pub(super) fn select_agent(
        &mut self,
        kind: AgentKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_launching() || self.agent == Some(kind) {
            return;
        }
        let selectable = self.agents.ready().is_some_and(|options| {
            options
                .iter()
                .any(|option| option.spec.kind == kind && option.state.is_selectable())
        });
        if !selectable {
            return;
        }
        let configured = self
            .config
            .agents
            .get(&kind)
            .map(AgentCfg::permission_mode)
            .unwrap_or_default();
        let permission_mode = session_permission_mode(self.kind(), configured);
        self.agent = Some(kind);
        self.set_model_choices(None, window, cx);
        self.permission_mode = permission_mode;
        let mode = permission_mode.as_str().to_owned();
        self.permission_select.update(cx, |select, cx| {
            select.set_selected_value(&mode, window, cx)
        });
        self.clear_failure();
        cx.notify();
    }

    pub(super) fn set_model_choices(
        &mut self,
        preferred: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(option) = self.selected_option() else {
            return;
        };
        let listed = model_choices(&option.spec);
        let model = preferred
            .filter(|id| listed.iter().any(|model| &model.id == id))
            .or_else(|| default_model(&option.spec));
        let choices: Vec<Choice> = listed
            .into_iter()
            .map(|model| Choice::new(model.id, model.label))
            .collect();
        self.model = model.clone();
        self.model_select.update(cx, |select, cx| {
            select.set_items(choices, window, cx);
            if let Some(model) = &model {
                select.set_selected_value(model, window, cx);
            }
        });
    }

    pub(super) fn model_label(&self) -> Option<String> {
        let model = self.model.as_ref()?;
        let option = self.selected_option()?;
        Some(
            model_choices(&option.spec)
                .into_iter()
                .find(|choice| &choice.id == model)
                .map_or_else(|| model.clone(), |choice| choice.label),
        )
    }

    pub(super) fn set_model(&mut self, id: String, cx: &mut Context<Self>) {
        if self.model.as_ref() != Some(&id) {
            self.model = Some(id);
            cx.notify();
        }
    }

    pub(super) fn set_permission_mode(&mut self, mode: PermissionMode, cx: &mut Context<Self>) {
        let mode = session_permission_mode(self.kind(), mode);
        if self.permission_mode != mode {
            self.permission_mode = mode;
            cx.notify();
        }
    }

    pub(super) fn set_templates(
        &mut self,
        templates: Vec<PromptTemplate>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let kind = template_kind(self.kind());
        self.all_templates = templates.clone();
        let templates = templates_of_kind(templates, kind);
        let candidates =
            self.config
                .review
                .template_candidates(kind, self.subject.host(), self.subject.repo());
        let chosen = default_template(&templates, &candidates);
        let choices: Vec<Choice> = templates
            .iter()
            .map(|template| Choice::new(template.id.clone(), template.name.clone()))
            .collect();
        self.template_select.update(cx, |select, cx| {
            select.set_items(choices, window, cx);
            if let Some(id) = &chosen {
                select.set_selected_value(id, window, cx);
            }
        });
        self.templates = Load::Ready(templates);
        self.template = chosen;
        self.rerender(cx);
    }

    pub(super) fn current_template(&self) -> Option<&PromptTemplate> {
        let id = self.template.as_ref()?;
        self.templates
            .ready()?
            .iter()
            .find(|template| &template.id == id)
    }

    pub(super) fn set_template(&mut self, id: String, cx: &mut Context<Self>) {
        if self.template.as_ref() != Some(&id) {
            self.template = Some(id);
            self.rerender(cx);
        }
    }

    pub(super) fn repo_template(&self) -> Option<&str> {
        self.config
            .review
            .repo_template(self.subject.host(), self.subject.repo())
    }

    pub(super) fn is_pinned_to_repo(&self) -> bool {
        self.template.is_some() && self.repo_template() == self.template.as_deref()
    }

    pub(super) fn toggle_repo_pin(&mut self, cx: &mut Context<Self>) {
        if self.kind() != SessionKind::Review {
            return;
        }
        let Some(template) = self.template.clone() else {
            return;
        };
        let key = repo_key(self.subject.host(), self.subject.repo());
        let pinned = self.is_pinned_to_repo();
        self.config = config_watch::update(cx, |config| {
            if pinned {
                config.review.repo_templates.remove(&key);
            } else {
                config.review.repo_templates.insert(key, template);
            }
        });
        cx.notify();
    }

    pub(super) fn set_include_reviews(&mut self, on: bool, cx: &mut Context<Self>) {
        self.include_reviews = on;
        self.rerender(cx);
    }

    pub(super) fn set_include_ci(&mut self, on: bool, cx: &mut Context<Self>) {
        self.include_ci = on;
        if on {
            self.load_ci(cx);
        }
        self.rerender(cx);
    }

    pub(super) fn set_refresh_project(&mut self, on: bool, cx: &mut Context<Self>) {
        if self.has_saved_overview() {
            self.refresh_project = on;
            self.rerender(cx);
        }
    }

    pub(super) fn saved_overview(&self) -> Option<&ProjectOverview> {
        match self.context.ready()? {
            Gathering::Summary(gathering) => gathering.overview.as_ref(),
            Gathering::Review(_) | Gathering::Plan(_) => None,
        }
    }

    fn has_saved_overview(&self) -> bool {
        self.saved_overview().is_some()
    }

    fn planned_related(
        &self,
        gathering: &SummaryGathering,
        cx: &Context<Self>,
    ) -> Vec<RelatedRepo> {
        let (Some(sessions), Ok(session)) = (Self::sessions(cx), &self.session_id) else {
            return planned_related(&gathering.related, |_| {
                Err(labels::SERVICES_UNAVAILABLE.to_owned())
            });
        };
        let worktrees = sessions.read(cx).worktrees();
        let host = self.subject.host();
        planned_related(&gathering.related, |repo| {
            worktrees
                .planned_related_path(host, repo, session)
                .map_err(|error| error.to_string())
        })
    }

    pub(super) fn ci_text(&self) -> Option<&str> {
        match &self.ci {
            Some(Load::Ready(text)) => Some(text),
            _ => None,
        }
    }

    pub(super) fn is_rendering(&self) -> bool {
        self.render_task.is_some()
    }

    pub(super) fn rerender(&mut self, cx: &mut Context<Self>) {
        self.render_epoch += 1;
        let epoch = self.render_epoch;
        let (Some(gathered), Some(template)) = (self.context.ready(), self.current_template())
        else {
            self.preview = None;
            self.render_task = None;
            cx.notify();
            return;
        };
        let template = match resolve_template(template, &self.all_templates) {
            Ok(resolved) => resolved,
            Err(error) => {
                self.preview = Some(preview(Err(render_error(&error)), None));
                self.render_task = None;
                cx.notify();
                return;
            }
        };
        let job: Task<Preview> = match gathered.clone() {
            Gathering::Review(gathered) => {
                let ci = self.ci_text().map(str::to_owned);
                let options = ContextOptions {
                    include_reviews: self.include_reviews,
                    include_ci: self.include_ci,
                };
                let max_diff_chars = self.config.review.max_diff_chars;
                cx.background_spawn(async move {
                    let ctx = prompt_context(&gathered, ci.as_deref(), options, max_diff_chars);
                    let inline = diff_inline(&ctx.diff, max_diff_chars);
                    let rendered = render_prompt(&template, &ctx, max_diff_chars)
                        .map_err(|error| render_error(&error));
                    preview(rendered, Some(inline))
                })
            }
            Gathering::Summary(gathering) => {
                let related = self.planned_related(&gathering, cx);
                let refresh_project = self.refresh_project;
                cx.background_spawn(async move {
                    let ctx = preview_context(&gathering, refresh_project, related);
                    let rendered = render_summary_prompt(&template, &ctx)
                        .map_err(|error| render_error(&error));
                    preview(rendered, None)
                })
            }
            Gathering::Plan(detail) => cx.background_spawn(async move {
                let ctx =
                    plan_context(&detail, labels::PLAN_BASE_PENDING, labels::PLAN_SHA_PENDING);
                let rendered =
                    render_plan_prompt(&template, &ctx).map_err(|error| render_error(&error));
                preview(rendered, None)
            }),
        };
        self.render_task = Some(cx.spawn(async move |this, cx| {
            let preview = job.await;
            _ = this.update(cx, |this, cx| {
                if this.render_epoch == epoch {
                    this.preview = Some(preview);
                    this.render_task = None;
                    cx.notify();
                }
            });
        }));
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template(id: &str, kind: TemplateKind) -> PromptTemplate {
        PromptTemplate {
            id: id.into(),
            name: id.into(),
            kind,
            ..PromptTemplate::default()
        }
    }

    fn ids(templates: &[PromptTemplate]) -> Vec<&str> {
        templates
            .iter()
            .map(|template| template.id.as_str())
            .collect()
    }

    fn all() -> Vec<PromptTemplate> {
        vec![
            template("review-padrao", TemplateKind::Review),
            template("plan-padrao", TemplateKind::Plan),
            template("seguranca", TemplateKind::Review),
            template("meu-plano", TemplateKind::Plan),
        ]
    }

    #[test]
    fn review_dialog_lists_only_review_templates() {
        let kind = template_kind(SessionKind::Review);
        assert_eq!(
            ids(&templates_of_kind(all(), kind)),
            ["review-padrao", "seguranca"]
        );
    }

    #[test]
    fn plan_dialog_lists_only_plan_templates() {
        let kind = template_kind(SessionKind::Plan);
        assert_eq!(
            ids(&templates_of_kind(all(), kind)),
            ["plan-padrao", "meu-plano"]
        );
    }

    #[test]
    fn plan_sessions_stay_supervised_whatever_is_asked() {
        for mode in [
            PermissionMode::Supervised,
            PermissionMode::AcceptEdits,
            PermissionMode::FullAccess,
        ] {
            assert_eq!(
                session_permission_mode(SessionKind::Plan, mode),
                PermissionMode::Supervised
            );
            assert_eq!(session_permission_mode(SessionKind::Review, mode), mode);
        }
    }
}

use argon_core::{AgentKind, PromptTemplate, render_prompt};
use gpui_kit::{AppContext as _, Context, SharedString, Window};

use super::{Load, Preview, StartDialog};
use crate::agents::choice::Choice;
use crate::agents::context::{ContextOptions, diff_inline, prompt_context};
use crate::agents::detection::{
    AgentOption, default_agent, default_model, default_template, model_choices,
};
use crate::agents::labels;

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
        let Some(option) = self
            .agents
            .ready()
            .and_then(|options| options.iter().find(|option| option.spec.kind == kind))
            .filter(|option| option.state.is_selectable())
        else {
            return;
        };
        let model = default_model(&option.spec);
        let choices: Vec<Choice> = model_choices(&option.spec)
            .into_iter()
            .map(|model| Choice::new(model.id, model.label))
            .collect();
        self.agent = Some(kind);
        self.model = model.clone();
        self.model_select.update(cx, |select, cx| {
            select.set_items(choices, window, cx);
            if let Some(model) = &model {
                select.set_selected_value(model, window, cx);
            }
        });
        self.clear_failure();
        cx.notify();
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

    pub(super) fn set_templates(
        &mut self,
        templates: Vec<PromptTemplate>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let chosen = default_template(&templates, &self.config.review.default_template);
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
        let gathered = gathered.clone();
        let template = template.clone();
        let ci = self.ci_text().map(str::to_owned);
        let options = ContextOptions {
            include_reviews: self.include_reviews,
            include_ci: self.include_ci,
        };
        let max_diff_chars = self.config.review.max_diff_chars;
        let job = cx.background_spawn(async move {
            let ctx = prompt_context(&gathered, ci.as_deref(), options, max_diff_chars);
            let inline = diff_inline(&ctx.diff, max_diff_chars);
            let rendered = render_prompt(&template, &ctx, max_diff_chars)
                .map_err(|error| SharedString::from(labels::render_failed(&error.to_string())));
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
                diff_inline: inline,
            }
        });
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

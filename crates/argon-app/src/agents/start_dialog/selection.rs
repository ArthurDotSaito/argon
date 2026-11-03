use argon_core::{AgentKind, PromptTemplate, render_prompt};
use gpui_kit::{Context, SharedString, Window};

use super::choice::Choice;
use super::{Load, StartDialog};
use crate::agents::context::{ContextOptions, diff_inline, prompt_context};
use crate::agents::detection::{AgentOption, default_agent, default_template, model_choices};
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
        let models = model_choices(&option.spec);
        let model = models
            .iter()
            .find(|model| model.id == option.spec.default_model)
            .or_else(|| models.first())
            .map(|model| model.id.clone());
        let choices: Vec<Choice> = models
            .iter()
            .map(|model| Choice::new(model.id.clone(), model.label.clone()))
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

    pub(super) fn rerender(&mut self, cx: &mut Context<Self>) {
        let (Some(gathered), Some(template)) = (self.context.ready(), self.current_template())
        else {
            self.rendered = None;
            self.preview.clear();
            self.preview_chars = 0;
            cx.notify();
            return;
        };
        let options = ContextOptions {
            include_reviews: self.include_reviews,
            include_ci: self.include_ci,
        };
        let ctx = prompt_context(gathered, self.ci_text(), options);
        let inline = diff_inline(&ctx.diff, self.config.review.max_diff_chars);
        let rendered = render_prompt(template, &ctx, self.config.review.max_diff_chars)
            .map_err(|error| SharedString::from(labels::render_failed(&error.to_string())));
        match &rendered {
            Ok(prompt) => {
                self.preview = prompt
                    .text
                    .lines()
                    .map(|line| line.to_owned().into())
                    .collect();
                self.preview_chars = prompt.text.chars().count();
            }
            Err(_) => {
                self.preview.clear();
                self.preview_chars = 0;
            }
        }
        self.diff_inline = inline;
        self.rendered = Some(rendered);
        cx.notify();
    }
}

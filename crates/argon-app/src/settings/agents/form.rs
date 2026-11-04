use std::path::PathBuf;
use std::time::Instant;

use argon_agents::{
    AgentSpec, Detection, ModelInfo, Support, agent_spec, list_models, lists_models, support_for,
};
use argon_core::{AgentCfg, AgentKind, Config, PermissionMode};
use gpui_kit::component::input::InputState;
use gpui_kit::component::select::{SelectEvent, SelectState};
use gpui_kit::{AppContext as _, Context, Entity, SharedString, Subscription, Task, Window};

use super::fields::{FieldError, join_args, parse_env, split_args};
use super::{DetectionResult, run_detection, store_cache};
use crate::agents::{AgentSessions, CardState, Choice, Trigger, card_state, permission_choices};
use crate::config_watch;
use crate::settings::labels;
use crate::settings::ui::services;

pub(super) struct EnvRow {
    pub key: Entity<InputState>,
    pub value: Entity<InputState>,
    _subscriptions: [Subscription; 2],
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Source {
    Saved(Trigger),
    Draft,
}

pub(super) enum Status {
    Detecting,
    Done(CardState),
    Failed(SharedString),
}

pub struct AgentForm {
    pub(super) kind: AgentKind,
    pub(super) label: &'static str,
    pub(super) support: Support,
    pub(super) status: Status,
    pub(super) detected: Option<AgentSpec>,
    draft_models: bool,
    draft_task: Option<Task<()>>,
    pub(super) saved: AgentCfg,
    pub(super) command: Entity<InputState>,
    pub(super) args: Entity<InputState>,
    pub(super) env: Vec<EnvRow>,
    pub(super) model_select: Entity<SelectState<Vec<Choice>>>,
    pub(super) model: Option<String>,
    pub(super) permission_select: Entity<SelectState<Vec<Choice>>>,
    pub(super) permission_mode: PermissionMode,
    pub(super) use_acp: bool,
    pub(super) error: Option<SharedString>,
    test_task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

fn text_input(
    placeholder: String,
    value: String,
    masked: bool,
    window: &mut Window,
    cx: &mut Context<AgentForm>,
) -> Entity<InputState> {
    cx.new(|cx| {
        let mut state = InputState::new(window, cx)
            .placeholder(placeholder)
            .masked(masked);
        state.set_value(value, window, cx);
        state
    })
}

fn normalized(cfg: AgentCfg) -> AgentCfg {
    let mode = cfg.permission_mode();
    cfg.with_permission_mode(mode)
}

impl AgentForm {
    pub fn new(
        kind: AgentKind,
        config: &Config,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let defaults = agent_spec(kind, &Config::default());
        let saved = normalized(config.agents.get(&kind).cloned().unwrap_or_default());
        let command = text_input(
            defaults.command.display().to_string(),
            saved
                .command
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            false,
            window,
            cx,
        );
        let args = text_input(
            join_args(&defaults.args),
            join_args(&saved.args),
            false,
            window,
            cx,
        );
        let model_select = cx.new(|cx| SelectState::new(Vec::<Choice>::new(), None, window, cx));
        let permission_mode = saved.permission_mode();
        let permission_select = cx.new(|cx| {
            let mut select = SelectState::new(permission_choices(), None, window, cx);
            select.set_selected_value(&permission_mode.as_str().to_owned(), window, cx);
            select
        });
        let subscriptions = vec![
            cx.observe(&command, |_, _, cx| cx.notify()),
            cx.observe(&args, |_, _, cx| cx.notify()),
            cx.subscribe_in(
                &model_select,
                window,
                |this, _, event: &SelectEvent<Vec<Choice>>, _, cx| {
                    if let SelectEvent::Confirm(Some(id)) = event {
                        this.model = Some(id.clone()).filter(|id| !id.is_empty());
                        cx.notify();
                    }
                },
            ),
            cx.subscribe_in(
                &permission_select,
                window,
                |this, _, event: &SelectEvent<Vec<Choice>>, _, cx| {
                    if let SelectEvent::Confirm(Some(id)) = event
                        && let Some(mode) = PermissionMode::parse(id)
                    {
                        this.permission_mode = mode;
                        cx.notify();
                    }
                },
            ),
        ];
        let mut this = Self {
            kind,
            label: defaults.label,
            support: support_for(kind),
            status: Status::Detecting,
            detected: None,
            draft_models: false,
            draft_task: None,
            model: saved.default_model.clone(),
            permission_select,
            permission_mode,
            use_acp: saved.use_acp,
            command,
            args,
            env: Vec::new(),
            model_select,
            error: None,
            test_task: None,
            _subscriptions: subscriptions,
            saved,
        };
        for (key, value) in this.saved.env.clone() {
            this.push_env(key, value, window, cx);
        }
        this.refresh_models(window, cx);
        this
    }

    pub(super) fn push_env(
        &mut self,
        key: String,
        value: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = text_input(labels::ENV_KEY.into(), key, false, window, cx);
        let value = text_input(labels::ENV_VALUE.into(), value, true, window, cx);
        let subscriptions = [
            cx.observe(&key, |_, _, cx| cx.notify()),
            cx.observe(&value, |_, _, cx| cx.notify()),
        ];
        self.env.push(EnvRow {
            key,
            value,
            _subscriptions: subscriptions,
        });
        cx.notify();
    }

    pub(super) fn remove_env(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix < self.env.len() {
            self.env.remove(ix);
            cx.notify();
        }
    }

    pub(super) fn is_supported(&self) -> bool {
        self.support == Support::Supported
    }

    pub(super) fn draft(&self, cx: &Context<Self>) -> Result<AgentCfg, FieldError> {
        let command = self.command.read(cx).value().trim().to_owned();
        let rows: Vec<(String, String)> = self
            .env
            .iter()
            .map(|row| {
                (
                    row.key.read(cx).value().to_string(),
                    row.value.read(cx).value().to_string(),
                )
            })
            .collect();
        Ok(AgentCfg {
            command: (!command.is_empty()).then(|| PathBuf::from(command)),
            args: split_args(&self.args.read(cx).value())?,
            env: parse_env(&rows)?,
            default_model: self.model.clone(),
            use_acp: self.use_acp,
            ..AgentCfg::default()
        }
        .with_permission_mode(self.permission_mode))
    }

    pub(super) fn is_dirty(&self, cx: &Context<Self>) -> bool {
        !self.draft(cx).is_ok_and(|draft| draft == self.saved)
    }

    pub(super) fn apply(
        &mut self,
        results: &[DetectionResult],
        source: Source,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((spec, support, detection)) =
            results.iter().find(|(spec, _, _)| spec.kind == self.kind)
        else {
            return;
        };
        self.status = Status::Done(card_state(support, detection));
        let mut spec = spec.clone();
        let found = matches!(detection, Detection::Found { .. });
        self.draft_models = source == Source::Draft;
        self.draft_task = None;
        match source {
            Source::Saved(trigger) => {
                if let Some(sessions) = AgentSessions::global(cx) {
                    if found {
                        let specs = vec![spec.clone()];
                        sessions.update(cx, |sessions, cx| {
                            sessions.refresh_models(specs, trigger, cx)
                        });
                    }
                    sessions.read(cx).apply_listed_models(&mut spec);
                }
            }
            Source::Draft if found => self.list_draft_models(spec.clone(), window, cx),
            Source::Draft => {}
        }
        self.detected = Some(spec);
        self.refresh_models(window, cx);
        cx.notify();
    }

    fn list_draft_models(&mut self, spec: AgentSpec, window: &mut Window, cx: &mut Context<Self>) {
        if !lists_models(spec.kind) {
            return;
        }
        let Some(services) = services(cx) else {
            return;
        };
        let label = spec.label;
        let job = services.spawn_runtime(async move { list_models(&spec).await });
        self.draft_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = job.await;
            _ = this.update_in(cx, |this, window, cx| match result {
                Ok(Some(Ok(models))) if !models.is_empty() => this.set_draft_models(models, window, cx),
                Ok(Some(Ok(_))) => {
                    tracing::warn!(agent = label, "the draft configuration listed no models");
                }
                Ok(Some(Err(error))) => {
                    tracing::warn!(agent = label, %error, "failed to list models for the draft configuration");
                }
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!(agent = label, %error, "failed to list models for the draft configuration");
                }
            });
        }));
    }

    fn set_draft_models(
        &mut self,
        models: Vec<ModelInfo>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.draft_models {
            return;
        }
        if let Some(spec) = self.detected.as_mut() {
            spec.models = models;
            self.refresh_models(window, cx);
            cx.notify();
        }
    }

    pub(super) fn apply_listed_models(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.draft_models {
            return;
        }
        let Some(sessions) = AgentSessions::global(cx) else {
            return;
        };
        let Some(spec) = self.detected.as_ref() else {
            return;
        };
        let listed = sessions
            .read(cx)
            .listed_models(self.kind)
            .filter(|listed| spec.models != *listed)
            .map(<[_]>::to_vec);
        if let Some(models) = listed {
            if let Some(spec) = self.detected.as_mut() {
                spec.models = models;
            }
            self.refresh_models(window, cx);
            cx.notify();
        }
    }

    fn refresh_models(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let fallback = agent_spec(self.kind, &Config::default());
        let models = self
            .detected
            .as_ref()
            .map_or(&fallback.models, |spec| &spec.models);
        let mut choices = vec![Choice::new(
            "",
            labels::agent_default_model(&fallback.default_model),
        )];
        choices.extend(
            models
                .iter()
                .map(|model| Choice::new(model.id.clone(), model.label.clone())),
        );
        if let Some(current) = &self.model
            && !choices.iter().any(|choice| &choice.id == current)
        {
            choices.push(Choice::new(current.clone(), current.clone()));
        }
        let selected = self.model.clone().unwrap_or_default();
        self.model_select.update(cx, |select, cx| {
            select.set_items(choices, window, cx);
            select.set_selected_value(&selected, window, cx);
        });
    }

    pub(super) fn test(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let draft = match self.draft(cx) {
            Ok(draft) => draft,
            Err(error) => {
                self.error = Some(error.message().into());
                cx.notify();
                return;
            }
        };
        let matches_saved = draft == self.saved;
        let mut config = config_watch::current(cx);
        config.agents.insert(self.kind, draft);
        let Some(task) = run_detection(config, cx) else {
            self.status = Status::Failed(labels::SERVICES_UNAVAILABLE.into());
            cx.notify();
            return;
        };
        self.status = Status::Detecting;
        self.error = None;
        self.test_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            _ = this.update_in(cx, |this, window, cx| match result {
                Ok(results) => {
                    if matches_saved {
                        store_cache(&results, Instant::now(), cx);
                    }
                    let source = if matches_saved {
                        Source::Saved(Trigger::Manual)
                    } else {
                        Source::Draft
                    };
                    this.apply(&results, source, window, cx);
                }
                Err(error) => {
                    this.status = Status::Failed(labels::detection_failed(&error).into());
                    cx.notify();
                }
            });
        }));
        cx.notify();
    }

    pub(super) fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let draft = match self.draft(cx) {
            Ok(draft) => draft,
            Err(error) => {
                self.error = Some(error.message().into());
                cx.notify();
                return;
            }
        };
        let kind = self.kind;
        let stored = draft.clone();
        config_watch::update(cx, move |config| {
            if stored == normalized(AgentCfg::default()) {
                config.agents.remove(&kind);
            } else {
                config.agents.insert(kind, stored);
            }
        });
        self.saved = draft;
        self.error = None;
        self.test(window, cx);
    }

    pub(super) fn discard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let saved = self.saved.clone();
        self.permission_mode = saved.permission_mode();
        let command = saved
            .command
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        self.command
            .update(cx, |input, cx| input.set_value(command, window, cx));
        let args = join_args(&saved.args);
        self.args
            .update(cx, |input, cx| input.set_value(args, window, cx));
        self.env.clear();
        for (key, value) in saved.env {
            self.push_env(key, value, window, cx);
        }
        self.model = saved.default_model;
        let mode = self.permission_mode.as_str().to_owned();
        self.permission_select.update(cx, |select, cx| {
            select.set_selected_value(&mode, window, cx)
        });
        self.use_acp = saved.use_acp;
        self.error = None;
        self.refresh_models(window, cx);
        cx.notify();
    }
}

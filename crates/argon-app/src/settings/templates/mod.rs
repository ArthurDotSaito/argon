mod actions;
mod files;
mod naming;
mod preview;
mod profile;
mod repos;
mod review_opts;
mod view;

use std::path::PathBuf;

use argon_core::{Config, InvalidTemplate, PromptTemplate};
use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::component::select::{SelectEvent, SelectState};
use gpui_kit::{AppContext as _, Context, Entity, SharedString, Subscription, Task, Window};

use self::preview::PreviewState;
use self::profile::ProfileEditor;
use self::repos::RepoPicker;
use super::labels;
use crate::agents::{AgentSessions, Choice};
use crate::config_watch;
use crate::review::{Confirm, confirm};

const EDITOR_ROWS: usize = 22;

enum Load<T> {
    Loading,
    Ready(T),
    Failed(SharedString),
}

pub struct TemplatesSection {
    dir: Option<PathBuf>,
    templates: Load<Vec<PromptTemplate>>,
    invalid: Vec<InvalidTemplate>,
    selected: Option<String>,
    name: Entity<InputState>,
    body: Entity<TextareaState>,
    error: Option<SharedString>,
    busy: bool,
    preview: PreviewState,
    profile: ProfileEditor,
    repo_picker: RepoPicker,
    default_select: Entity<SelectState<Vec<Choice>>>,
    diff_limit: Entity<InputState>,
    config: Config,
    io_task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl TemplatesSection {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let dir = AgentSessions::global(cx).map(|sessions| sessions.read(cx).paths().prompts_dir());
        let config = config_watch::current(cx);
        let name = cx.new(|cx| InputState::new(window, cx));
        let body = cx.new(|cx| TextareaState::new(window, cx).rows(EDITOR_ROWS));
        let default_select = cx.new(|cx| SelectState::new(Vec::<Choice>::new(), None, window, cx));
        let diff_limit = cx.new(|cx| InputState::new(window, cx));
        let preview = PreviewState::new(window, cx);
        let profile = ProfileEditor::new(window, cx);
        let repo_picker = RepoPicker::new(window, cx);
        let mut subscriptions = vec![
            cx.observe(&name, |_, _, cx| cx.notify()),
            cx.subscribe(&body, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.rerender(cx);
                }
            }),
            cx.subscribe_in(
                &default_select,
                window,
                |this, _, event: &SelectEvent<Vec<Choice>>, _, cx| {
                    if let SelectEvent::Confirm(Some(id)) = event {
                        this.set_default_template(id.clone(), cx);
                    }
                },
            ),
            cx.subscribe_in(&diff_limit, window, |this, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                    this.commit_diff_limit(cx);
                }
            }),
            cx.subscribe_in(
                &preview.target_select,
                window,
                |this, _, event: &SelectEvent<Vec<Choice>>, _, cx| {
                    if let SelectEvent::Confirm(Some(id)) = event {
                        this.set_target(id, cx);
                    }
                },
            ),
        ];
        subscriptions.extend(profile.subscriptions(window, cx));
        subscriptions.push(repo_picker.subscription(window, cx));
        let mut this = Self {
            dir,
            templates: Load::Loading,
            invalid: Vec::new(),
            selected: None,
            name,
            body,
            error: None,
            busy: false,
            preview,
            profile,
            repo_picker,
            default_select,
            diff_limit,
            config,
            io_task: None,
            _subscriptions: subscriptions,
        };
        this.sync_review_options(window, cx);
        this.reload(None, window, cx);
        this.refresh_targets(window, cx);
        this
    }

    pub fn shown(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.refresh_targets(window, cx);
        self.refresh_repos(window, cx);
    }

    pub fn config_changed(&mut self, config: &Config, window: &mut Window, cx: &mut Context<Self>) {
        let review_changed = config.review != self.config.review;
        self.config = config.clone();
        if review_changed {
            self.sync_review_options(window, cx);
            self.refresh_repos(window, cx);
            self.rerender(cx);
        }
    }

    fn saved_templates(&self) -> &[PromptTemplate] {
        match &self.templates {
            Load::Ready(templates) => templates,
            Load::Loading | Load::Failed(_) => &[],
        }
    }

    fn current(&self) -> Option<&PromptTemplate> {
        let id = self.selected.as_ref()?;
        self.saved_templates()
            .iter()
            .find(|template| &template.id == id)
    }

    fn draft(&self, cx: &Context<Self>) -> Option<PromptTemplate> {
        let current = self.current()?;
        let name = self.name.read(cx).value().to_string();
        if current.base.is_some() {
            return Some(self.profile_draft(current, name, cx));
        }
        Some(PromptTemplate {
            id: current.id.clone(),
            name,
            description: current.description.clone(),
            kind: current.kind,
            version: current.version,
            base: current.base.clone(),
            detached: current.detached,
            body: self.body.read(cx).value().to_string(),
        })
    }

    fn is_dirty(&self, cx: &Context<Self>) -> bool {
        match (self.current(), self.draft(cx)) {
            (Some(saved), Some(_)) if saved.base.is_some() => self.profile_dirty(saved, cx),
            (Some(saved), Some(draft)) => saved.name != draft.name || saved.body != draft.body,
            _ => false,
        }
    }

    fn reload(&mut self, select: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dir) = self.dir.clone() else {
            self.templates = Load::Failed(labels::SERVICES_UNAVAILABLE.into());
            return;
        };
        let job = cx.background_spawn(async move { files::load(&dir) });
        self.io_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = job.await;
            _ = this.update_in(cx, |this, window, cx| {
                this.busy = false;
                match result {
                    Ok(set) => {
                        this.invalid = set.invalid;
                        this.templates = Load::Ready(set.templates);
                        let wanted = select
                            .or_else(|| this.selected.clone())
                            .or_else(|| Some(this.config.review.default_template.clone()));
                        this.select(wanted, window, cx);
                        this.sync_review_options(window, cx);
                    }
                    Err(error) => {
                        this.templates =
                            Load::Failed(labels::templates_failed(&error.to_string()).into());
                        cx.notify();
                    }
                }
            });
        }));
    }

    fn select(&mut self, id: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let templates = self.saved_templates();
        let chosen = id
            .filter(|id| templates.iter().any(|template| &template.id == id))
            .or_else(|| templates.first().map(|template| template.id.clone()));
        self.selected = chosen;
        self.error = None;
        let (name, body) = self
            .current()
            .map(|template| (template.name.clone(), template.body.clone()))
            .unwrap_or_default();
        self.name
            .update(cx, |input, cx| input.set_value(name, window, cx));
        self.body
            .update(cx, |input, cx| input.set_value(body, window, cx));
        self.load_profile(window, cx);
        self.refresh_repos(window, cx);
        self.rerender(cx);
    }

    fn request_select(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected.as_ref() == Some(&id) {
            return;
        }
        if !self.is_dirty(cx) {
            self.select(Some(id), window, cx);
            return;
        }
        let name = self.current().map(|t| t.name.clone()).unwrap_or_default();
        let this = cx.entity().downgrade();
        confirm(
            Confirm {
                title: labels::DISCARD_TITLE,
                message: labels::discard_message(&name),
                confirm: labels::DISCARD,
            },
            window,
            cx,
            move |window, cx| {
                _ = this.update(cx, |this, cx| this.select(Some(id.clone()), window, cx));
            },
        );
    }
}

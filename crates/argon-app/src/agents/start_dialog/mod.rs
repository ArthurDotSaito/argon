mod cards;
mod choice;
mod fields;
mod launch;
mod preview;
mod render;
mod selection;

use std::time::Instant;

use argon_agents::{detect_all, new_session_id};
use argon_core::{
    AgentKind, Config, PromptTemplate, PullRequest, RenderedPrompt, ensure_default_templates,
    load_templates,
};
use argon_github::PrDetail;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::select::{SelectEvent, SelectState};
use gpui_kit::{
    App, AppContext as _, Context, Entity, ParentElement as _, SharedString, Subscription, Task,
    UniformListScrollHandle, Window, px,
};

use self::choice::Choice;
use super::context::{Gathered, gather};
use super::detection::AgentOption;
use super::labels;
use super::launcher::LaunchStage;
use super::sessions::AgentSessions;
use crate::services::{AppServices, Services};

const DIALOG_WIDTH: f32 = 980.;

type ChoiceSelect = Entity<SelectState<Vec<Choice>>>;

pub struct ReviewTarget {
    pub pr: PullRequest,
    pub detail: Option<PrDetail>,
}

enum Load<T> {
    Loading,
    Ready(T),
    Failed(SharedString),
}

impl<T> Load<T> {
    fn ready(&self) -> Option<&T> {
        match self {
            Self::Ready(value) => Some(value),
            Self::Loading | Self::Failed(_) => None,
        }
    }
}

enum Phase {
    Idle,
    Launching(LaunchStage),
    Failed(SharedString),
}

pub struct StartDialog {
    pr: PullRequest,
    config: Config,
    agents: Load<Vec<AgentOption>>,
    agent: Option<AgentKind>,
    model: Option<String>,
    templates: Load<Vec<PromptTemplate>>,
    template: Option<String>,
    model_select: ChoiceSelect,
    template_select: ChoiceSelect,
    include_reviews: bool,
    include_ci: bool,
    context: Load<Gathered>,
    ci: Option<Load<String>>,
    rendered: Option<Result<RenderedPrompt, SharedString>>,
    preview: Vec<SharedString>,
    preview_chars: usize,
    diff_inline: bool,
    preview_scroll: UniformListScrollHandle,
    session_id: Result<String, SharedString>,
    phase: Phase,
    tasks: Vec<Task<()>>,
    _subscriptions: [Subscription; 2],
}

pub fn open(target: ReviewTarget, window: &mut Window, cx: &mut App) {
    let dialog = cx.new(|cx| StartDialog::new(target, window, cx));
    window.open_dialog(cx, move |modal, _, cx| {
        let busy = dialog.read(cx).is_launching();
        let title = dialog.read(cx).title(cx);
        modal
            .w(px(DIALOG_WIDTH))
            .title(title)
            .overlay_closable(false)
            .keyboard(!busy)
            .close_button(!busy)
            .child(dialog.clone())
    });
}

fn app_services(cx: &App) -> Option<Services> {
    cx.try_global::<AppServices>()
        .map(|global| global.0.clone())
}

fn choice_select(window: &mut Window, cx: &mut Context<StartDialog>) -> ChoiceSelect {
    cx.new(|cx| SelectState::new(Vec::<Choice>::new(), None, window, cx))
}

impl StartDialog {
    fn new(target: ReviewTarget, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let config = app_services(cx)
            .map(|services| services.config())
            .unwrap_or_default();
        let model_select = choice_select(window, cx);
        let template_select = choice_select(window, cx);
        let subscriptions = [
            cx.subscribe_in(
                &model_select,
                window,
                |this, _, event: &SelectEvent<Vec<Choice>>, _, cx| {
                    let SelectEvent::Confirm(Some(id)) = event else {
                        return;
                    };
                    this.set_model(id.clone(), cx);
                },
            ),
            cx.subscribe_in(
                &template_select,
                window,
                |this, _, event: &SelectEvent<Vec<Choice>>, _, cx| {
                    let SelectEvent::Confirm(Some(id)) = event else {
                        return;
                    };
                    this.set_template(id.clone(), cx);
                },
            ),
        ];
        let mut this = Self {
            include_reviews: config.review.include_previous_reviews,
            include_ci: config.review.include_ci_failures,
            pr: target.pr.clone(),
            config,
            agents: Load::Loading,
            agent: None,
            model: None,
            templates: Load::Loading,
            template: None,
            model_select,
            template_select,
            context: Load::Loading,
            ci: None,
            rendered: None,
            preview: Vec::new(),
            preview_chars: 0,
            diff_inline: false,
            preview_scroll: UniformListScrollHandle::new(),
            session_id: fresh_session_id(),
            phase: Phase::Idle,
            tasks: Vec::new(),
            _subscriptions: subscriptions,
        };
        this.load_agents(window, cx);
        this.load_templates(window, cx);
        this.load_context(target, cx);
        this
    }

    fn is_launching(&self) -> bool {
        matches!(self.phase, Phase::Launching(_))
    }

    fn sessions(cx: &App) -> Option<Entity<AgentSessions>> {
        AgentSessions::global(cx)
    }

    fn load_agents(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let cached = Self::sessions(cx).and_then(|sessions| {
            sessions
                .read(cx)
                .detection()
                .fresh(Instant::now())
                .map(<[_]>::to_vec)
        });
        if let Some(options) = cached {
            self.set_agents(options, window, cx);
            return;
        }
        let Some(services) = app_services(cx) else {
            self.agents = Load::Failed(labels::SERVICES_UNAVAILABLE.into());
            return;
        };
        let config = self.config.clone();
        let job = services.spawn_runtime(async move { detect_all(&config).await });
        self.tasks.push(cx.spawn_in(window, async move |this, cx| {
            let result = job.await;
            _ = this.update_in(cx, |this, window, cx| match result {
                Ok(detected) => {
                    let options: Vec<AgentOption> = detected
                        .into_iter()
                        .map(|(spec, support, detection)| {
                            AgentOption::new(spec, &support, &detection)
                        })
                        .collect();
                    if let Some(sessions) = Self::sessions(cx) {
                        let cached = options.clone();
                        sessions.update(cx, |sessions, _| {
                            sessions.detection_mut().store(Instant::now(), cached)
                        });
                    }
                    this.set_agents(options, window, cx);
                }
                Err(error) => {
                    this.agents = Load::Failed(labels::detection_failed(&error.to_string()).into());
                    cx.notify();
                }
            });
        }));
    }

    fn load_templates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((services, dir)) = app_services(cx)
            .zip(Self::sessions(cx).map(|sessions| sessions.read(cx).paths().prompts_dir()))
        else {
            self.templates = Load::Failed(labels::SERVICES_UNAVAILABLE.into());
            return;
        };
        let job = services.spawn_runtime(async move {
            ensure_default_templates(&dir).and_then(|()| load_templates(&dir))
        });
        self.tasks.push(cx.spawn_in(window, async move |this, cx| {
            let result = job.await;
            _ = this.update_in(cx, |this, window, cx| {
                let result = result
                    .map_err(|error| error.to_string())
                    .and_then(|loaded| loaded.map_err(|error| error.to_string()));
                match result {
                    Ok(templates) => this.set_templates(templates, window, cx),
                    Err(error) => {
                        this.templates = Load::Failed(labels::templates_failed(&error).into());
                        cx.notify();
                    }
                }
            });
        }));
    }

    fn load_context(&mut self, target: ReviewTarget, cx: &mut Context<Self>) {
        let Some(services) = app_services(cx) else {
            self.context = Load::Failed(labels::SERVICES_UNAVAILABLE.into());
            return;
        };
        let task = gather(&services, &target.pr, target.detail, cx);
        self.tasks.push(cx.spawn(async move |this, cx| {
            let result = task.await;
            _ = this.update(cx, |this, cx| {
                match result {
                    Ok(gathered) => {
                        this.pr = gathered.detail.pr.clone();
                        this.context = Load::Ready(gathered);
                        if this.include_ci {
                            this.load_ci(cx);
                        }
                    }
                    Err(error) => {
                        this.context =
                            Load::Failed(labels::context_failed(&error.to_string()).into());
                    }
                }
                this.rerender(cx);
            });
        }));
    }

    fn load_ci(&mut self, cx: &mut Context<Self>) {
        let (Some(services), Some(gathered)) = (app_services(cx), self.context.ready()) else {
            return;
        };
        if self.ci.is_some() {
            return;
        }
        self.ci = Some(Load::Loading);
        let task = super::context::gather_ci(&services, &gathered.detail, cx);
        self.tasks.push(cx.spawn(async move |this, cx| {
            let result = task.await;
            _ = this.update(cx, |this, cx| {
                this.ci = Some(match result {
                    Ok(text) => Load::Ready(text),
                    Err(error) => Load::Failed(labels::ci_failed(&error.to_string()).into()),
                });
                this.rerender(cx);
            });
        }));
    }
}

fn fresh_session_id() -> Result<String, SharedString> {
    new_session_id().map_err(|error| SharedString::from(error.to_string()))
}

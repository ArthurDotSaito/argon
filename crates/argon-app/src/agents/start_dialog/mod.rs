mod cards;
mod fields;
mod launch;
mod loaders;
mod preview;
mod render;
mod selection;

use std::sync::Arc;

use argon_agents::new_session_id;
use argon_core::{
    AgentKind, Config, InvalidTemplate, PermissionMode, PromptTemplate, PullRequest, RenderedPrompt,
};
use argon_github::PrDetail;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::select::{SelectEvent, SelectState};
use gpui_kit::{
    App, AppContext as _, Context, Entity, ParentElement as _, SharedString, Subscription, Task,
    UniformListScrollHandle, Window, px,
};

use super::context::Gathered;
use super::detection::AgentOption;
use super::launcher::LaunchStage;
use super::sessions::AgentSessions;
use crate::actions::OpenSettings;
use crate::agents::choice::{Choice, permission_choices};
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
    Cancelling,
    Cancelled,
    Failed(SharedString),
}

struct Preview {
    rendered: Result<RenderedPrompt, SharedString>,
    lines: Vec<SharedString>,
    chars: usize,
    diff_inline: bool,
}

pub struct StartDialog {
    pr: PullRequest,
    config: Config,
    agents: Load<Vec<AgentOption>>,
    agent: Option<AgentKind>,
    model: Option<String>,
    permission_mode: PermissionMode,
    templates: Load<Vec<PromptTemplate>>,
    invalid_templates: Vec<InvalidTemplate>,
    template: Option<String>,
    model_select: ChoiceSelect,
    permission_select: ChoiceSelect,
    template_select: ChoiceSelect,
    include_reviews: bool,
    include_ci: bool,
    context: Load<Arc<Gathered>>,
    ci: Option<Load<String>>,
    preview: Option<Preview>,
    render_epoch: u64,
    render_task: Option<Task<()>>,
    preview_scroll: UniformListScrollHandle,
    session_id: Result<String, SharedString>,
    phase: Phase,
    tasks: Vec<Task<()>>,
    _subscriptions: Vec<Subscription>,
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

fn open_settings(window: &mut Window, cx: &mut App) {
    window.close_dialog(cx);
    window.defer(cx, |window, cx| {
        window.dispatch_action(Box::new(OpenSettings), cx)
    });
}

fn fresh_session_id() -> Result<String, SharedString> {
    new_session_id().map_err(|error| SharedString::from(error.to_string()))
}

impl StartDialog {
    fn new(target: ReviewTarget, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let config = app_services(cx)
            .map(|services| services.config())
            .unwrap_or_default();
        let model_select = choice_select(window, cx);
        let template_select = choice_select(window, cx);
        let permission_select =
            cx.new(|cx| SelectState::new(permission_choices(), None, window, cx));
        let mut subscriptions = vec![
            cx.subscribe_in(
                &model_select,
                window,
                |this, _, event: &SelectEvent<Vec<Choice>>, _, cx| {
                    if let SelectEvent::Confirm(Some(id)) = event {
                        this.set_model(id.clone(), cx);
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
                        this.set_permission_mode(mode, cx);
                    }
                },
            ),
            cx.subscribe_in(
                &template_select,
                window,
                |this, _, event: &SelectEvent<Vec<Choice>>, _, cx| {
                    if let SelectEvent::Confirm(Some(id)) = event {
                        this.set_template(id.clone(), cx);
                    }
                },
            ),
        ];
        if let Some(sessions) = Self::sessions(cx) {
            subscriptions.push(cx.observe_in(&sessions, window, |this, _, window, cx| {
                this.on_sessions_changed(window, cx)
            }));
        }
        let mut this = Self {
            include_reviews: config.review.include_previous_reviews,
            include_ci: config.review.include_ci_failures,
            pr: target.pr.clone(),
            config,
            agents: Load::Loading,
            agent: None,
            model: None,
            permission_mode: PermissionMode::default(),
            templates: Load::Loading,
            invalid_templates: Vec::new(),
            template: None,
            model_select,
            permission_select,
            template_select,
            context: Load::Loading,
            ci: None,
            preview: None,
            render_epoch: 0,
            render_task: None,
            preview_scroll: UniformListScrollHandle::new(),
            session_id: fresh_session_id(),
            phase: Phase::Idle,
            tasks: Vec::new(),
            _subscriptions: subscriptions,
        };
        super::warmup::now(cx);
        this.load_agents(window, cx);
        this.load_templates(window, cx);
        this.load_context(target, cx);
        this
    }

    fn is_launching(&self) -> bool {
        matches!(self.phase, Phase::Launching(_) | Phase::Cancelling)
    }

    fn sessions(cx: &App) -> Option<Entity<AgentSessions>> {
        AgentSessions::global(cx)
    }
}

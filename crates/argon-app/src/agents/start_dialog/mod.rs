mod cards;
mod fields;
mod launch;
mod loaders;
mod models;
mod preview;
mod render;
mod selection;
mod usage;

use std::sync::Arc;

use argon_agents::new_session_id;
use argon_core::{
    AgentKind, Config, InvalidTemplate, Issue, PermissionMode, PrKey, PromptTemplate, PullRequest,
    RenderedPrompt, SessionKind,
};
use argon_github::{IssueDetail, PrDetail};
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::select::{SelectEvent, SelectState};
use gpui_kit::{
    App, AppContext as _, Context, Entity, ParentElement as _, SharedString, Subscription, Task,
    UniformListScrollHandle, Window, px,
};

use super::context::{Gathered, SummaryGathering};
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

pub struct SummaryTarget {
    pub pr: PullRequest,
    pub detail: Option<PrDetail>,
}

pub struct PlanTarget {
    pub issue: Issue,
    pub detail: Option<IssueDetail>,
}

pub enum Target {
    Review(ReviewTarget),
    #[expect(dead_code)]
    Summary(SummaryTarget),
    Plan(PlanTarget),
}

enum Subject {
    Pr(PullRequest, SessionKind),
    Issue(Issue),
}

impl Subject {
    fn of(target: &Target) -> Self {
        match target {
            Target::Review(target) => Self::Pr(target.pr.clone(), SessionKind::Review),
            Target::Summary(target) => Self::Pr(target.pr.clone(), SessionKind::Summary),
            Target::Plan(target) => Self::Issue(target.issue.clone()),
        }
    }

    fn kind(&self) -> SessionKind {
        match self {
            Self::Pr(_, kind) => *kind,
            Self::Issue(_) => SessionKind::Plan,
        }
    }

    fn key(&self) -> PrKey {
        match self {
            Self::Pr(pr, _) => pr.key(),
            Self::Issue(issue) => issue.key(),
        }
    }

    fn repo(&self) -> &str {
        match self {
            Self::Pr(pr, _) => &pr.repo,
            Self::Issue(issue) => &issue.repo,
        }
    }

    fn host(&self) -> &str {
        match self {
            Self::Pr(pr, _) => &pr.host,
            Self::Issue(issue) => &issue.host,
        }
    }

    fn number(&self) -> u64 {
        match self {
            Self::Pr(pr, _) => pr.number,
            Self::Issue(issue) => issue.number,
        }
    }
}

#[derive(Clone)]
enum Gathering {
    Review(Arc<Gathered>),
    Summary(Arc<SummaryGathering>),
    Plan(Arc<IssueDetail>),
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
    diff_inline: Option<bool>,
}

pub struct StartDialog {
    subject: Subject,
    config: Config,
    agents: Load<Vec<AgentOption>>,
    agent: Option<AgentKind>,
    model: Option<String>,
    permission_mode: PermissionMode,
    templates: Load<Vec<PromptTemplate>>,
    all_templates: Vec<PromptTemplate>,
    invalid_templates: Vec<InvalidTemplate>,
    template: Option<String>,
    model_select: ChoiceSelect,
    permission_select: ChoiceSelect,
    template_select: ChoiceSelect,
    include_reviews: bool,
    include_ci: bool,
    refresh_project: bool,
    context: Load<Gathering>,
    ci: Option<Load<String>>,
    preview: Option<Preview>,
    render_epoch: u64,
    usage_revision: u64,
    render_task: Option<Task<()>>,
    preview_scroll: UniformListScrollHandle,
    session_id: Result<String, SharedString>,
    phase: Phase,
    tasks: Vec<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

pub fn open(target: Target, window: &mut Window, cx: &mut App) {
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
    fn new(target: Target, window: &mut Window, cx: &mut Context<Self>) -> Self {
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
        let subject = Subject::of(&target);
        let review = subject.kind() == SessionKind::Review;
        let mut this = Self {
            include_reviews: review && config.review.include_previous_reviews,
            include_ci: review && config.review.include_ci_failures,
            refresh_project: false,
            subject,
            config,
            agents: Load::Loading,
            agent: None,
            model: None,
            permission_mode: PermissionMode::default(),
            templates: Load::Loading,
            all_templates: Vec::new(),
            invalid_templates: Vec::new(),
            template: None,
            model_select,
            permission_select,
            template_select,
            context: Load::Loading,
            ci: None,
            preview: None,
            render_epoch: 0,
            usage_revision: 0,
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

    fn kind(&self) -> SessionKind {
        self.subject.kind()
    }

    fn sessions(cx: &App) -> Option<Entity<AgentSessions>> {
        AgentSessions::global(cx)
    }
}

mod ai;
mod body;
mod closing;
mod loading;
mod merge;
mod summary;

use std::rc::Rc;

use argon_core::{PrKey, PullRequest, Reason, RepoKey, Side};
use argon_github::{MergeMethod, PrDetail};
use argon_store::PrSummary;
use chrono::{DateTime, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, Styled as _, Subscription, Task, Window,
    div,
};

use super::actions::ActionsTab;
use super::commits::{CommitsEvent, CommitsTab};
use super::conversation::{Conversation, ConversationEvent};
use super::header::{HeaderProps, header};
use super::labels;
use super::properties;
use crate::inbox::InboxModel;
use crate::keymap::PR_VIEW_CONTEXT;
use crate::review::{
    DraftsBus, DraftsChanged, ProjectsChanged, ReviewScope, ReviewTab, ReviewTabEvent,
};
use crate::services::{AppServices, Services};

pub use body::{
    failure_banner, failure_panel, fallback_header, retry_button, skeleton, tab_button,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PrTab {
    #[default]
    Conversation,
    Commits,
    Actions,
    Review,
}

impl PrTab {
    pub const ALL: [Self; 4] = [
        Self::Conversation,
        Self::Commits,
        Self::Actions,
        Self::Review,
    ];

    fn title(self) -> &'static str {
        match self {
            Self::Conversation => labels::TAB_OVERVIEW,
            Self::Commits => labels::TAB_COMMITS,
            Self::Actions => labels::TAB_CHECKS,
            Self::Review => labels::TAB_FILES,
        }
    }

    fn icon(self) -> IconName {
        match self {
            Self::Conversation => IconName::MessageSquare,
            Self::Commits => IconName::GitCommitHorizontal,
            Self::Actions => IconName::Play,
            Self::Review => IconName::FileDiff,
        }
    }
}

enum Load {
    Loading,
    Failed(SharedString),
    Ready,
}

pub enum MergeState {
    Idle,
    Merging,
    Merged,
    Failed(SharedString),
}

impl MergeState {
    pub fn is_busy(&self) -> bool {
        matches!(self, Self::Merging | Self::Merged)
    }
}

pub struct PrView {
    key: PrKey,
    inbox: Entity<InboxModel>,
    tab: PrTab,
    detail: Option<Rc<PrDetail>>,
    store_pr: Option<Rc<PullRequest>>,
    viewer_login: Option<String>,
    load: Load,
    refreshing: bool,
    epoch: u64,
    applied_epoch: u64,
    seen_updated_at: Option<DateTime<Utc>>,
    merge_method: MergeMethod,
    merge_state: MergeState,
    conversation: Entity<Conversation>,
    commits: Entity<CommitsTab>,
    actions: Entity<ActionsTab>,
    review: Entity<ReviewTab>,
    focus_handle: FocusHandle,
    pending_ai_review: bool,
    pending_summarize: bool,
    summary: Option<PrSummary>,
    summary_task: Option<Task<()>>,
    overview_at: Option<DateTime<Utc>>,
    checks_running: bool,
    load_task: Option<Task<()>>,
    merge_task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl PrView {
    pub fn new(
        key: PrKey,
        inbox: Entity<InboxModel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let viewer_login = viewer_login(&key.host, cx);
        let conversation =
            cx.new(|cx| Conversation::new(key.clone(), viewer_login.clone(), window, cx));
        let commits = cx.new(|cx| CommitsTab::new(window, cx));
        let actions = cx.new(|cx| ActionsTab::new(window, cx));
        let review = cx.new(|cx| ReviewTab::new(window, cx));
        let drafts = DraftsBus::global(cx);
        let subscriptions = vec![
            cx.subscribe_in(
                &conversation,
                window,
                |this, _, event, window, cx| match event {
                    ConversationEvent::Commented => {
                        this.reload(cx);
                        request_sync(cx);
                    }
                    ConversationEvent::CommentConfirmed => request_sync(cx),
                    ConversationEvent::Recheck => this.reload(cx),
                    ConversationEvent::OpenOnGithub => this.open_on_github(cx),
                    ConversationEvent::OpenCommits => this.set_tab(PrTab::Commits, cx),
                    ConversationEvent::OpenChecks => {
                        this.set_tab(PrTab::Actions, cx);
                        this.actions
                            .update(cx, |actions, cx| actions.reopen_failed_log(cx));
                    }
                    ConversationEvent::RerunFailed => {
                        this.set_tab(PrTab::Actions, cx);
                        this.actions
                            .update(cx, |actions, cx| actions.confirm_rerun(window, cx));
                    }
                    ConversationEvent::StartReview => this.start_review(window, cx),
                    ConversationEvent::Reveal { path, line, side } => {
                        this.reveal(path, *line, *side, cx)
                    }
                },
            ),
            cx.observe(&commits, |_, _, cx| cx.notify()),
            cx.subscribe(&commits, |this, _, event, cx| match event {
                CommitsEvent::OpenCommit { oid, headline } => this.set_review_scope(
                    ReviewScope::Commit {
                        oid: oid.clone(),
                        headline: headline.clone(),
                    },
                    cx,
                ),
            }),
            cx.observe(&actions, |this, _, cx| {
                this.sync_checks(cx);
                cx.notify();
            }),
            cx.observe(&review, |_, _, cx| cx.notify()),
            cx.subscribe(&review, |this, _, event, cx| match event {
                ReviewTabEvent::ShowAll => this.set_review_scope(ReviewScope::All, cx),
                ReviewTabEvent::Reload => {
                    this.reload(cx);
                    request_sync(cx);
                }
            }),
            cx.observe(&inbox, |this, _, cx| this.on_inbox_changed(cx)),
            cx.subscribe(&drafts, |this, _, event: &DraftsChanged, cx| {
                if event.pr == this.key {
                    this.reload_summary(cx);
                }
            }),
            cx.subscribe(&drafts, |this, _, _: &ProjectsChanged, cx| {
                this.reload_summary(cx)
            }),
        ];
        let store_pr = inbox.read(cx).pr(&key).cloned().map(Rc::new);
        let seen_updated_at = store_pr.as_ref().map(|pr| pr.updated_at);
        let review_login = viewer_login.clone();
        review.update(cx, |tab, cx| tab.set_viewer_login(review_login, cx));
        let mut this = Self {
            key,
            inbox,
            tab: PrTab::default(),
            detail: None,
            store_pr,
            viewer_login,
            load: Load::Loading,
            refreshing: false,
            epoch: 0,
            applied_epoch: 0,
            seen_updated_at,
            merge_method: MergeMethod::Merge,
            merge_state: MergeState::Idle,
            conversation,
            commits,
            actions,
            review,
            focus_handle: cx.focus_handle(),
            pending_ai_review: false,
            pending_summarize: false,
            summary: None,
            summary_task: None,
            overview_at: None,
            checks_running: false,
            load_task: None,
            merge_task: None,
            _subscriptions: subscriptions,
        };
        this.reload(cx);
        this.reload_summary(cx);
        this
    }

    pub fn set_tab(&mut self, tab: PrTab, cx: &mut Context<Self>) {
        if self.tab != tab {
            self.tab = tab;
            self.actions.update(cx, |actions, cx| {
                actions.set_active(tab == PrTab::Actions, cx)
            });
            self.review
                .update(cx, |review, cx| review.set_active(tab == PrTab::Review, cx));
            cx.notify();
        }
    }

    fn sync_checks(&mut self, cx: &mut Context<Self>) {
        let (runs, error_line, running) = {
            let actions = self.actions.read(cx);
            (actions.runs(), actions.error_line(), actions.has_running())
        };
        self.conversation.update(cx, |conversation, cx| {
            conversation.set_checks(runs, error_line, cx)
        });
        let finished = self.checks_running && !running;
        self.checks_running = running;
        if finished {
            self.reload(cx);
        }
    }

    pub fn set_review_scope(&mut self, scope: ReviewScope, cx: &mut Context<Self>) {
        self.review.update(cx, |tab, cx| tab.set_scope(scope, cx));
        self.set_tab(PrTab::Review, cx);
    }

    pub fn reveal(&mut self, path: &str, line: u32, side: Side, cx: &mut Context<Self>) {
        self.set_review_scope(ReviewScope::All, cx);
        self.review
            .update(cx, |review, cx| review.reveal(path, line, side, cx));
    }

    pub fn selected_line(&self, cx: &App) -> Option<(String, u32)> {
        self.review.read(cx).selected_line(cx)
    }

    pub fn start_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_review_scope(ReviewScope::All, cx);
        self.review
            .update(cx, |review, cx| review.open_finish(window, cx));
    }

    pub fn open_on_github(&mut self, _: &mut Context<Self>) {
        let Some(url) = self.pr().map(|pr| pr.url.clone()) else {
            return;
        };
        if let Err(error) = open::that_detached(&url) {
            tracing::warn!(%error, url, "failed to open pr in browser");
        }
    }

    pub fn project_key(&self) -> Option<RepoKey> {
        self.pr()
            .map(|pr| RepoKey::new(pr.host.clone(), pr.repo.clone()))
    }

    fn inbox_reason(&self, cx: &App) -> Option<Reason> {
        self.inbox
            .read(cx)
            .item(&self.key)
            .map(|item| item.reason.clone())
    }
}

fn app_services(cx: &App) -> Option<Services> {
    cx.try_global::<AppServices>()
        .map(|global| global.0.clone())
}

fn request_sync(cx: &App) {
    if let Some(services) = app_services(cx) {
        services.sync_now();
    }
}

fn viewer_login(host: &str, cx: &App) -> Option<String> {
    app_services(cx)?
        .accounts()
        .into_iter()
        .find(|account| account.host.eq_ignore_ascii_case(host))
        .map(|account| account.login)
}

impl Focusable for PrView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for PrView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.settle_pending_ai_review(window, cx);
        self.settle_pending_summarize(window, cx);
        if self.tab == PrTab::Conversation {
            self.actions
                .update(cx, |actions, _| actions.mark_overview_visible());
        }
        let now = Utc::now();
        let detail = self.detail.clone();
        let store_pr = self.store_pr.clone();
        let pr = detail
            .as_ref()
            .map(|detail| &detail.pr)
            .or(store_pr.as_deref());
        let show_panel = detail.is_some() && self.tab != PrTab::Review;
        let reason = show_panel.then(|| self.inbox_reason(cx)).flatten();
        let viewer = self.viewer_login.clone();
        let (head, panel) = match pr {
            Some(pr) => {
                let props = HeaderProps {
                    pr,
                    detail: detail.as_deref(),
                    closing: self.closing_links(pr, cx),
                    can_merge: self.can_merge(),
                    can_review_with_ai: self.can_review_with_ai(),
                    merge_state: &self.merge_state,
                    refreshing: self.refreshing,
                    now,
                    overview_at: self.overview_at,
                    viewer: viewer.as_deref(),
                };
                let show_review = self.tab != PrTab::Review;
                let actions = properties::action_buttons(&props, show_review, cx);
                let panel = show_panel.then(|| properties::panel(&props, reason.as_ref(), cx));
                (header(&props, actions, cx), panel)
            }
            None => (body::pr_fallback_header(cx), None),
        };
        let banner = self.error_banner(cx);
        let summary_banner = (self.tab == PrTab::Conversation)
            .then(|| self.summary_banner(cx))
            .flatten();
        let tab_bar = self.tab_bar(cx);
        let body = self.body(cx);
        let theme = cx.theme();
        let main = v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(head)
            .children(banner)
            .child(
                div()
                    .flex_none()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(tab_bar),
            )
            .children(summary_banner)
            .child(div().flex_1().min_h_0().child(body));
        h_flex()
            .key_context(PR_VIEW_CONTEXT)
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(theme.background)
            .child(main)
            .children(panel)
    }
}

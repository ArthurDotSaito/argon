mod body;
mod loading;
mod merge;

use std::rc::Rc;

use argon_core::{PrKey, PullRequest, Side};
use argon_github::{MergeMethod, PrDetail};
use chrono::{DateTime, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme as _, v_flex};
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
use crate::inbox::InboxModel;
use crate::review::{ReviewScope, ReviewTab, ReviewTabEvent};
use crate::services::{AppServices, Services};

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
            Self::Conversation => labels::TAB_CONVERSATION,
            Self::Commits => labels::TAB_COMMITS,
            Self::Actions => labels::TAB_ACTIONS,
            Self::Review => labels::TAB_REVIEW,
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
        let conversation = cx.new(|cx| Conversation::new(key.clone(), window, cx));
        let commits = cx.new(|cx| CommitsTab::new(window, cx));
        let actions = cx.new(|cx| ActionsTab::new(window, cx));
        let review = cx.new(|cx| ReviewTab::new(window, cx));
        let subscriptions = vec![
            cx.subscribe(&conversation, |this, _, event, cx| match event {
                ConversationEvent::Commented => {
                    this.reload(cx);
                    request_sync(cx);
                }
                ConversationEvent::CommentConfirmed => request_sync(cx),
                ConversationEvent::Recheck => this.reload(cx),
                ConversationEvent::OpenOnGithub => this.open_on_github(cx),
            }),
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
            cx.observe(&actions, |_, _, cx| cx.notify()),
            cx.observe(&review, |_, _, cx| cx.notify()),
            cx.subscribe(&review, |this, _, event, cx| match event {
                ReviewTabEvent::ShowAll => this.set_review_scope(ReviewScope::All, cx),
            }),
            cx.observe(&inbox, |this, _, cx| this.on_inbox_changed(cx)),
        ];
        let store_pr = inbox.read(cx).pr(&key).cloned().map(Rc::new);
        let seen_updated_at = store_pr.as_ref().map(|pr| pr.updated_at);
        let viewer_login = viewer_login(&key.host, cx);
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
            load_task: None,
            merge_task: None,
            _subscriptions: subscriptions,
        };
        this.reload(cx);
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

    pub fn set_review_scope(&mut self, scope: ReviewScope, cx: &mut Context<Self>) {
        self.review.update(cx, |tab, cx| tab.set_scope(scope, cx));
        self.set_tab(PrTab::Review, cx);
    }

    #[expect(
        dead_code,
        reason = "called by review navigation from drafts and agent findings (Tasks 16 and 23)"
    )]
    pub fn reveal(&mut self, path: &str, line: u32, side: Side, cx: &mut Context<Self>) {
        self.set_review_scope(ReviewScope::All, cx);
        self.review
            .update(cx, |review, cx| review.reveal(path, line, side, cx));
    }

    pub fn open_on_github(&mut self, _: &mut Context<Self>) {
        let Some(url) = self.pr().map(|pr| pr.url.clone()) else {
            return;
        };
        if let Err(error) = open::that_detached(&url) {
            tracing::warn!(%error, url, "failed to open pr in browser");
        }
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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Utc::now();
        let detail = self.detail.clone();
        let store_pr = self.store_pr.clone();
        let pr = detail
            .as_ref()
            .map(|detail| &detail.pr)
            .or(store_pr.as_deref());
        let head = match pr {
            Some(pr) => {
                let can_merge = self.can_merge();
                header(
                    HeaderProps {
                        pr,
                        detail: detail.as_deref(),
                        can_merge,
                        merge_state: &self.merge_state,
                        refreshing: self.refreshing,
                        now,
                    },
                    cx,
                )
            }
            None => body::fallback_header(cx),
        };
        let banner = self.error_banner(cx);
        let tab_bar = self.tab_bar(cx);
        let body = self.body(cx);
        let theme = cx.theme();
        v_flex()
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(theme.background)
            .child(head)
            .children(banner)
            .child(
                div()
                    .flex_none()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(tab_bar),
            )
            .child(div().flex_1().min_h_0().child(body))
    }
}

use std::rc::Rc;

use argon_core::{PrKey, PrState, PullRequest, Side, is_ready};
use argon_github::{MergeMethod, PrDetail};
use chrono::{DateTime, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::Button;
use gpui_kit::component::skeleton::Skeleton;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Div, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString, Stateful,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window, div, px,
};

use super::actions::ActionsTab;
use super::commits::{CommitsEvent, CommitsTab};
use super::conversation::{self, CONTENT_MAX_WIDTH, Conversation, ConversationEvent};
use super::header::{HeaderProps, header};
use super::labels;
use super::merge_dialog::{self, MergeTarget};
use super::review_tab::{ReviewEvent, ReviewScope, ReviewTab};
use crate::inbox::{InboxModel, PR_FALLBACK_TITLE};
use crate::services::{AppServices, Services};

const TAB_HEIGHT: f32 = 34.;

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
                ReviewEvent::ShowAll => this.set_review_scope(ReviewScope::All, cx),
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
            cx.notify();
        }
    }

    pub fn set_review_scope(&mut self, scope: ReviewScope, cx: &mut Context<Self>) {
        self.review.update(cx, |tab, cx| tab.set_scope(scope, cx));
        self.set_tab(PrTab::Review, cx);
    }

    #[expect(
        dead_code,
        reason = "scroll-to-line lands with the diff view in Task 15"
    )]
    pub fn reveal(&mut self, _path: &str, _line: u32, _side: Side, _cx: &mut Context<Self>) {}

    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let Some(services) = app_services(cx) else {
            self.fail(labels::load_failed(labels::SERVICES_UNAVAILABLE), cx);
            return;
        };
        self.epoch += 1;
        let epoch = self.epoch;
        self.refreshing = true;
        if self.detail.is_none() {
            self.load = Load::Loading;
        }
        let id = self.key.id.clone();
        let task = services.spawn_github(cx, self.key.host.clone(), move |client| async move {
            client.pr_detail(&id).await
        });
        self.load_task = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            _ = this.update(cx, |this, cx| match result {
                Ok(detail) => this.apply(epoch, detail, cx),
                Err(error) => {
                    tracing::warn!(%error, "failed to load pr detail");
                    if epoch > this.applied_epoch {
                        this.applied_epoch = epoch;
                        this.fail(labels::load_failed(&error.to_string()), cx);
                    }
                }
            });
        }));
        cx.notify();
    }

    fn apply(&mut self, epoch: u64, detail: PrDetail, cx: &mut Context<Self>) {
        if epoch <= self.applied_epoch {
            return;
        }
        self.applied_epoch = epoch;
        self.refreshing = false;
        self.load = Load::Ready;
        self.seen_updated_at = self.seen_updated_at.max(Some(detail.pr.updated_at));
        if matches!(self.merge_state, MergeState::Merged) && detail.pr.state != PrState::Open {
            self.merge_state = MergeState::Idle;
        }
        let detail = Rc::new(detail);
        self.conversation
            .update(cx, |tab, cx| tab.set_detail(detail.clone(), cx));
        self.commits
            .update(cx, |tab, cx| tab.set_detail(detail.clone(), cx));
        self.actions
            .update(cx, |tab, cx| tab.set_detail(detail.clone(), cx));
        self.review
            .update(cx, |tab, cx| tab.set_detail(detail.clone(), cx));
        self.detail = Some(detail);
        cx.notify();
    }

    fn fail(&mut self, message: String, cx: &mut Context<Self>) {
        self.refreshing = false;
        self.load = Load::Failed(message.into());
        cx.notify();
    }

    fn on_inbox_changed(&mut self, cx: &mut Context<Self>) {
        let viewer_login = viewer_login(&self.key.host, cx);
        let changed_login = viewer_login != self.viewer_login;
        self.viewer_login = viewer_login;
        let current = self.store_pr.as_ref().map(|pr| pr.updated_at);
        let latest = self.inbox.read(cx).pr(&self.key).map(|pr| pr.updated_at);
        let changed_pr = latest != current;
        if changed_pr {
            self.store_pr = self.inbox.read(cx).pr(&self.key).cloned().map(Rc::new);
        }
        if latest.is_some() && latest > self.seen_updated_at {
            self.seen_updated_at = latest;
            self.reload(cx);
        } else if changed_login || (changed_pr && self.detail.is_none()) {
            cx.notify();
        }
    }

    fn pr(&self) -> Option<&PullRequest> {
        self.detail
            .as_ref()
            .map(|detail| &detail.pr)
            .or(self.store_pr.as_deref())
    }

    fn can_merge(&self) -> bool {
        let Some(pr) = self.pr() else {
            return false;
        };
        !self.merge_state.is_busy()
            && is_ready(pr)
            && self
                .viewer_login
                .as_deref()
                .is_some_and(|login| login.eq_ignore_ascii_case(&pr.author))
    }

    pub fn open_on_github(&mut self, _: &mut Context<Self>) {
        let Some(url) = self.pr().map(|pr| pr.url.clone()) else {
            return;
        };
        if let Err(error) = open::that_detached(&url) {
            tracing::warn!(%error, url, "failed to open pr in browser");
        }
    }

    pub fn merge_method(&self) -> MergeMethod {
        self.merge_method
    }

    pub fn set_merge_method(&mut self, method: MergeMethod, cx: &mut Context<Self>) {
        self.merge_method = method;
        cx.notify();
    }

    pub fn open_merge_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_merge() {
            return;
        }
        let Some(pr) = self.pr() else {
            return;
        };
        let target = MergeTarget {
            number: pr.number,
            head: pr.head.clone(),
            base: pr.base.clone(),
        };
        merge_dialog::open(cx.weak_entity(), target, window, cx);
    }

    pub fn merge(&mut self, cx: &mut Context<Self>) {
        if !self.can_merge() {
            return;
        }
        let Some(services) = app_services(cx) else {
            self.merge_state =
                MergeState::Failed(labels::merge_failed(labels::SERVICES_UNAVAILABLE).into());
            cx.notify();
            return;
        };
        self.merge_state = MergeState::Merging;
        let id = self.key.id.clone();
        let method = self.merge_method;
        let task = services.spawn_github(cx, self.key.host.clone(), move |client| async move {
            client.merge(&id, method).await
        });
        self.merge_task = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            _ = this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        tracing::info!("pull request merged");
                        this.merge_state = MergeState::Merged;
                        this.reload(cx);
                        request_sync(cx);
                    }
                    Err(error) => {
                        tracing::warn!(%error, "failed to merge pull request");
                        this.merge_state =
                            MergeState::Failed(labels::merge_failed(&error.to_string()).into());
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn tab_count(&self, tab: PrTab, cx: &App) -> Option<usize> {
        match tab {
            PrTab::Conversation => self.detail.as_deref().map(conversation::count),
            PrTab::Commits => self.commits.read(cx).count(),
            PrTab::Actions => self.actions.read(cx).count(),
            PrTab::Review => self.review.read(cx).count(),
        }
    }

    fn tab_bar(&self, cx: &mut Context<Self>) -> Div {
        let items: Vec<Stateful<Div>> = PrTab::ALL
            .iter()
            .enumerate()
            .map(|(ix, tab)| self.tab_item(ix, *tab, cx))
            .collect();
        h_flex().px_2().gap_1().items_end().children(items)
    }

    fn tab_item(&self, ix: usize, tab: PrTab, cx: &mut Context<Self>) -> Stateful<Div> {
        let selected = self.tab == tab;
        let count = self.tab_count(tab, cx);
        let theme = cx.theme();
        let count = count.map(|count| {
            div()
                .px_1p5()
                .rounded_full()
                .bg(theme.muted)
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(count.to_string())
        });
        h_flex()
            .id(("pr-tab", ix))
            .h(px(TAB_HEIGHT))
            .px_3()
            .gap_1p5()
            .items_center()
            .border_b_2()
            .border_color(if selected {
                theme.primary
            } else {
                gpui_kit::transparent_black()
            })
            .text_sm()
            .text_color(if selected {
                theme.foreground
            } else {
                theme.muted_foreground
            })
            .cursor_pointer()
            .hover(|style| style.text_color(theme.foreground))
            .child(Icon::new(tab.icon()).small())
            .child(tab.title())
            .children(count)
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.set_tab(tab, cx)))
    }

    fn body(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.detail.is_some() {
            return match self.tab {
                PrTab::Conversation => self.conversation.clone().into_any_element(),
                PrTab::Commits => self.commits.clone().into_any_element(),
                PrTab::Actions => self.actions.clone().into_any_element(),
                PrTab::Review => self.review.clone().into_any_element(),
            };
        }
        match &self.load {
            Load::Failed(message) => self.error_panel(message.clone(), cx).into_any_element(),
            Load::Loading | Load::Ready => skeleton().into_any_element(),
        }
    }

    fn error_panel(&self, message: SharedString, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_3()
            .child(
                Icon::new(IconName::CircleAlert)
                    .large()
                    .text_color(theme.danger),
            )
            .child(
                div()
                    .max_w(px(560.))
                    .text_sm()
                    .text_center()
                    .text_color(theme.foreground)
                    .child(message),
            )
            .child(retry_button("pr-retry", cx))
    }

    fn error_banner(&self, cx: &mut Context<Self>) -> Option<Div> {
        let Load::Failed(message) = &self.load else {
            return None;
        };
        self.detail.as_ref()?;
        let theme = cx.theme();
        Some(
            h_flex()
                .mx_4()
                .mb_2()
                .px_3()
                .py_1p5()
                .gap_2()
                .items_center()
                .rounded(theme.radius)
                .border_1()
                .border_color(theme.danger.opacity(0.5))
                .bg(theme.danger.opacity(0.08))
                .text_sm()
                .child(
                    Icon::new(IconName::CircleAlert)
                        .small()
                        .text_color(theme.danger),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(theme.foreground)
                        .child(message.clone()),
                )
                .child(retry_button("pr-retry-banner", cx)),
        )
    }

    fn fallback_header(&self, cx: &App) -> Div {
        let theme = cx.theme();
        v_flex()
            .px_4()
            .pt_3()
            .pb_3()
            .gap_1()
            .child(
                div()
                    .text_xl()
                    .font_semibold()
                    .text_color(theme.foreground)
                    .child(PR_FALLBACK_TITLE),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(labels::NOT_IN_INBOX),
            )
    }
}

fn retry_button(id: &'static str, cx: &mut Context<PrView>) -> Button {
    Button::new(id)
        .icon(Icon::new(IconName::RotateCw))
        .label(labels::RETRY)
        .small()
        .outline()
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.reload(cx)))
}

fn skeleton() -> Div {
    v_flex()
        .w_full()
        .max_w(px(CONTENT_MAX_WIDTH))
        .mx_auto()
        .p_4()
        .gap_3()
        .child(Skeleton::new().w(px(320.)).h(px(16.)))
        .child(Skeleton::new().w_full().h(px(96.)))
        .child(Skeleton::new().w(px(480.)).h(px(14.)))
        .child(Skeleton::new().w_full().h(px(72.)))
        .child(Skeleton::new().w(px(400.)).h(px(14.)))
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
            None => self.fallback_header(cx),
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

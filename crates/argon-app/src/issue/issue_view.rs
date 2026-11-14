use std::rc::Rc;

use argon_core::{Issue, PrKey, SubjectKind};
use argon_github::IssueDetail;
use argon_store::PlanDraft;
use chrono::{DateTime, Utc};
use gpui_kit::component::button::Button;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, v_flex};
use gpui_kit::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Div, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, ScrollHandle, SharedString,
    Stateful, StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window, div, px,
};

use super::actions::{ActionBar, ActionState};
use super::header::header;
use super::labels;
use super::tabs::IssueTab;
use super::timeline::Timeline;
use crate::inbox::{ISSUE_FALLBACK_TITLE, InboxModel};
use crate::keymap::ISSUE_VIEW_CONTEXT;
use crate::pr::{self, CONTENT_MAX_WIDTH, Composer, ComposerEvent};
use crate::review::{DraftsBus, DraftsChanged};

pub(super) enum Load {
    Loading,
    Failed(SharedString),
    Ready,
}

pub struct IssueView {
    pub(super) key: PrKey,
    pub(super) inbox: Entity<InboxModel>,
    pub(super) tab: IssueTab,
    pub(super) detail: Option<Rc<IssueDetail>>,
    pub(super) store_issue: Option<Rc<Issue>>,
    pub(super) timeline: Timeline,
    pub(super) composer: Entity<Composer>,
    pub(super) load: Load,
    pub(super) refreshing: bool,
    pub(super) epoch: u64,
    pub(super) applied_epoch: u64,
    pub(super) seen_updated_at: Option<DateTime<Utc>>,
    pub(super) read_after_epoch: Option<u64>,
    pub(super) load_task: Option<Task<()>>,
    pub(super) action: ActionState,
    pub(super) action_task: Option<Task<()>>,
    pub(super) viewer_id: Option<String>,
    pub(super) pending_plan: bool,
    pub(super) plan_drafts: Vec<PlanDraft>,
    pub(super) plans_task: Option<Task<()>>,
    pub(super) linked_scroll: ScrollHandle,
    pub(super) sub_scroll: ScrollHandle,
    scroll: ScrollHandle,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl IssueView {
    pub fn new(
        key: PrKey,
        inbox: Entity<InboxModel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let composer = cx.new(|cx| Composer::new(key.clone(), SubjectKind::Issue, window, cx));
        let drafts = DraftsBus::global(cx);
        let subscriptions = vec![
            cx.subscribe(&drafts, |this, _, event: &DraftsChanged, cx| {
                if event.pr == this.key {
                    this.reload_plans(cx);
                }
            }),
            cx.observe(&inbox, |this, _, cx| this.on_inbox_changed(cx)),
            cx.subscribe(&composer, |this, _, event, cx| match event {
                ComposerEvent::Commented => this.reload_and_mark_read(cx),
                ComposerEvent::CommentConfirmed => this.mark_read(cx),
                ComposerEvent::Recheck => this.reload(cx),
            }),
        ];
        let store_issue = inbox.read(cx).issue(&key).cloned().map(Rc::new);
        let seen_updated_at = store_issue.as_ref().map(|issue| issue.updated_at);
        let mut this = Self {
            key,
            inbox,
            tab: IssueTab::default(),
            detail: None,
            store_issue,
            timeline: Timeline::default(),
            composer,
            load: Load::Loading,
            refreshing: false,
            epoch: 0,
            applied_epoch: 0,
            seen_updated_at,
            read_after_epoch: None,
            load_task: None,
            action: ActionState::Idle,
            action_task: None,
            viewer_id: None,
            pending_plan: false,
            plan_drafts: Vec::new(),
            plans_task: None,
            linked_scroll: ScrollHandle::new(),
            sub_scroll: ScrollHandle::new(),
            scroll: ScrollHandle::new(),
            focus_handle: cx.focus_handle(),
            _subscriptions: subscriptions,
        };
        this.reload(cx);
        this.reload_plans(cx);
        this
    }

    pub(super) fn open_on_github(&self) {
        let Some(url) = self.issue().map(|issue| issue.url.clone()) else {
            return;
        };
        if let Err(error) = open::that_detached(&url) {
            tracing::warn!(%error, url, "failed to open issue in browser");
        }
    }

    fn retry_button(&self, id: &'static str, cx: &mut Context<Self>) -> Button {
        pr::retry_button(id).on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.reload(cx)))
    }

    pub(super) fn viewer_login(&self, cx: &App) -> Option<SharedString> {
        self.inbox
            .read(cx)
            .accounts()
            .iter()
            .find(|account| account.host.eq_ignore_ascii_case(&self.key.host))
            .map(|account| account.login.clone())
    }

    fn error_banner(&self, cx: &mut Context<Self>) -> Option<Div> {
        let Load::Failed(message) = &self.load else {
            return None;
        };
        self.detail.as_ref()?;
        let retry = self.retry_button("issue-retry-banner", cx);
        Some(pr::failure_banner(message.clone(), retry, cx))
    }

    fn action_banner(&self, cx: &mut Context<Self>) -> Option<Div> {
        let ActionState::Failed { action, message } = &self.action else {
            return None;
        };
        let action = *action;
        let retry = pr::retry_button("issue-action-retry")
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.run_action(action, cx)));
        Some(pr::failure_banner(message.clone(), retry, cx))
    }

    fn body(
        &self,
        detail: Option<&IssueDetail>,
        now: DateTime<Utc>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let Some(detail) = detail {
            return match self.visible_tab() {
                IssueTab::Conversation => self.conversation(detail, now, cx).into_any_element(),
                IssueTab::LinkedPrs => self.linked_prs(detail, now, cx).into_any_element(),
                IssueTab::SubIssues => self.sub_issues(detail, cx).into_any_element(),
            };
        }
        match &self.load {
            Load::Failed(message) => {
                let retry = self.retry_button("issue-retry", cx);
                pr::failure_panel(message.clone(), retry, cx).into_any_element()
            }
            Load::Loading | Load::Ready => pr::skeleton().into_any_element(),
        }
    }

    fn conversation(
        &self,
        detail: &IssueDetail,
        now: DateTime<Utc>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let notice = detail.truncated_timeline.then(|| {
            let open = cx.listener(|this, _: &ClickEvent, _, _| this.open_on_github());
            pr::truncated_notice("issue-timeline-open-github", open, cx)
        });
        let content = v_flex()
            .w_full()
            .min_w_0()
            .max_w(px(CONTENT_MAX_WIDTH))
            .mx_auto()
            .px_4()
            .py_4()
            .gap_4()
            .text_color(cx.theme().foreground)
            .child(self.timeline.description(&detail.issue, now, cx))
            .children(notice)
            .child(self.timeline.items(detail, now, cx))
            .child(self.composer.clone());
        div()
            .id("issue-conversation-scroll")
            .size_full()
            .track_scroll(&self.scroll)
            .overflow_y_scroll()
            .vertical_scrollbar(&self.scroll)
            .child(content)
    }
}

impl Focusable for IssueView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for IssueView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.settle_pending_plan(window, cx);
        let now = Utc::now();
        let detail = self.detail.clone();
        let store_issue = self.store_issue.clone();
        let issue = detail
            .as_ref()
            .map(|detail| &detail.issue)
            .or(store_issue.as_deref());
        let viewer_login = self.viewer_login(cx);
        let running = self.action.busy();
        let head = match issue {
            Some(issue) => {
                let bar = ActionBar::of(issue, viewer_login.as_deref(), running);
                header(issue, self.refreshing, now, bar, cx)
            }
            None => pr::fallback_header(ISSUE_FALLBACK_TITLE, labels::NOT_IN_INBOX, cx),
        };
        let banner = self.error_banner(cx);
        let action_banner = self.action_banner(cx);
        let plan_banner = self.plan_banner(cx);
        let tab_bar = self.tab_bar(cx);
        let body = self.body(detail.as_deref(), now, cx);
        let theme = cx.theme();
        v_flex()
            .key_context(ISSUE_VIEW_CONTEXT)
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(theme.background)
            .child(head)
            .children(plan_banner)
            .children(banner)
            .children(action_banner)
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

use argon_core::SubIssues;
use argon_github::{IssueDetail, SubIssueRow};
use chrono::{DateTime, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    App, ClickEvent, Context, Div, InteractiveElement as _, ParentElement as _, ScrollHandle,
    SharedString, Stateful, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, px,
};

use super::destination::{self, Destination};
use super::header::{IssueStatus, status_style};
use super::issue_view::IssueView;
use super::labels;
use crate::inbox::{pr_row, repo_ref};
use crate::pr::{self, CONTENT_MAX_WIDTH, labels as pr_labels};
use crate::workspace::Workspace;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IssueTab {
    #[default]
    Conversation,
    LinkedPrs,
    SubIssues,
}

impl IssueTab {
    fn id(self) -> &'static str {
        match self {
            Self::Conversation => "issue-tab-conversation",
            Self::LinkedPrs => "issue-tab-linked-prs",
            Self::SubIssues => "issue-tab-sub-issues",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Conversation => pr_labels::TAB_CONVERSATION,
            Self::LinkedPrs => labels::TAB_LINKED_PRS,
            Self::SubIssues => labels::TAB_SUB_ISSUES,
        }
    }

    fn icon(self) -> IconName {
        match self {
            Self::Conversation => IconName::MessageSquare,
            Self::LinkedPrs => IconName::GitPullRequest,
            Self::SubIssues => IconName::ListTree,
        }
    }
}

impl IssueView {
    pub(super) fn visible_tab(&self) -> IssueTab {
        match self.tab {
            IssueTab::SubIssues if self.sub_issue_progress().is_none() => IssueTab::Conversation,
            tab => tab,
        }
    }

    fn set_tab(&mut self, tab: IssueTab, cx: &mut Context<Self>) {
        if self.tab != tab {
            self.tab = tab;
            cx.notify();
        }
    }

    fn sub_issue_progress(&self) -> Option<SubIssues> {
        self.issue()?.sub_issues.filter(|sub| sub.total > 0)
    }

    fn tab_count(&self, tab: IssueTab) -> Option<SharedString> {
        match tab {
            IssueTab::Conversation => self.issue().map(|issue| issue.comments.to_string().into()),
            IssueTab::LinkedPrs => {
                let count = match &self.detail {
                    Some(detail) => detail.linked_prs.len(),
                    None => self.issue()?.linked_prs.len(),
                };
                Some(count.to_string().into())
            }
            IssueTab::SubIssues => self
                .sub_issue_progress()
                .map(|sub| labels::sub_issue_progress(sub).into()),
        }
    }

    pub(super) fn tab_bar(&self, cx: &mut Context<Self>) -> Div {
        let selected = self.visible_tab();
        let tabs = [IssueTab::Conversation, IssueTab::LinkedPrs]
            .into_iter()
            .chain(self.sub_issue_progress().map(|_| IssueTab::SubIssues));
        let items: Vec<Stateful<Div>> = tabs
            .map(|tab| {
                pr::tab_button(
                    tab.id(),
                    tab.icon(),
                    tab.title(),
                    self.tab_count(tab),
                    tab == selected,
                    cx,
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.set_tab(tab, cx)))
            })
            .collect();
        h_flex().px_2().gap_1().items_end().children(items)
    }

    pub(super) fn linked_prs(
        &self,
        detail: &IssueDetail,
        now: DateTime<Utc>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = cx.theme();
        let inbox = self.inbox.read(cx);
        let list = if detail.linked_prs.is_empty() {
            div()
                .py_6()
                .text_center()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(labels::NO_LINKED_PRS)
        } else {
            list_frame(cx).children(detail.linked_prs.iter().enumerate().map(|(ix, pr)| {
                let item = inbox.item(&pr.key());
                let target = destination::linked_pr(pr, item.is_some());
                let row = pr_row(("issue-linked-pr", ix), pr, item, now, cx).on_click(cx.listener(
                    move |this, _: &ClickEvent, window, cx| this.open(&target, window, cx),
                ));
                separated(ix, cx).child(row)
            }))
        };
        let help = div()
            .text_sm()
            .text_color(theme.muted_foreground)
            .child(labels::LINKED_PRS_HELP);
        scroller("issue-linked-prs-scroll", &self.linked_scroll)
            .child(content(cx).child(help).child(list))
    }

    pub(super) fn sub_issues(&self, detail: &IssueDetail, cx: &mut Context<Self>) -> Stateful<Div> {
        let inbox = self.inbox.read(cx);
        let host = detail.issue.host.as_str();
        let list =
            list_frame(cx).children(detail.sub_issues.iter().enumerate().map(|(ix, sub)| {
                let stored = inbox
                    .items()
                    .iter()
                    .filter_map(|item| item.subject.as_issue());
                let target = destination::sub_issue(host, sub, stored);
                let row = sub_issue_row(ix, sub, cx).on_click(cx.listener(
                    move |this, _: &ClickEvent, window, cx| this.open(&target, window, cx),
                ));
                separated(ix, cx).child(row)
            }));
        scroller("issue-sub-issues-scroll", &self.sub_scroll).child(content(cx).child(list))
    }

    fn open(&self, target: &Destination, window: &mut Window, cx: &mut Context<Self>) {
        match target {
            Destination::Argon(item) => {
                let Some(workspace) = Workspace::global(cx) else {
                    return;
                };
                let item = item.clone();
                _ = workspace.update(cx, |workspace, cx| {
                    workspace.open_item(item, false, window, cx)
                });
            }
            Destination::Browser(url) => {
                if let Err(error) = open::that_detached(url) {
                    tracing::warn!(%error, url, "failed to open link in browser");
                }
            }
        }
    }
}

fn scroller(id: &'static str, scroll: &ScrollHandle) -> Stateful<Div> {
    div()
        .id(id)
        .size_full()
        .track_scroll(scroll)
        .overflow_y_scroll()
        .vertical_scrollbar(scroll)
}

fn content(cx: &App) -> Div {
    v_flex()
        .w_full()
        .min_w_0()
        .max_w(px(CONTENT_MAX_WIDTH))
        .mx_auto()
        .px_4()
        .py_4()
        .gap_3()
        .text_color(cx.theme().foreground)
}

fn list_frame(cx: &App) -> Div {
    let theme = cx.theme();
    v_flex()
        .w_full()
        .overflow_hidden()
        .rounded(theme.radius)
        .border_1()
        .border_color(theme.border)
}

fn separated(ix: usize, cx: &App) -> Div {
    div().w_full().when(ix > 0, |this| {
        this.border_t_1().border_color(cx.theme().border)
    })
}

fn sub_issue_row(ix: usize, sub: &SubIssueRow, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    let (icon, color) = status_style(IssueStatus::of(sub.state, None), cx);
    h_flex()
        .id(("issue-sub-issue", ix))
        .w_full()
        .px_3()
        .py_2()
        .gap_2()
        .items_center()
        .cursor_pointer()
        .hover(|style| style.bg(theme.list_hover))
        .child(Icon::new(icon).small().text_color(color))
        .child(
            div()
                .flex_none()
                .font_family(theme.mono_font_family.clone())
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(repo_ref(&sub.repo, sub.number)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_sm()
                .child(sub.title.clone()),
        )
}

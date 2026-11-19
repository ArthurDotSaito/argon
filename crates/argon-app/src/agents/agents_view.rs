use std::collections::HashSet;
use std::time::{Duration, Instant};

use argon_core::{PrKey, SessionKind};
use chrono::Utc;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::{
    App, Context, Div, Entity, FocusHandle, Focusable, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, Stateful, StatefulInteractiveElement as _,
    Styled as _, Subscription, Task, Window, div, px,
};

use super::badges::{severity_pills, status_chip};
use super::card_status::SessionFilter;
use super::catalog::SessionCatalog;
use super::grouping::{PrGroup, group_by_pr};
use super::panel_labels as labels;
use super::session_actions::{resume_error, row_actions};
use super::session_tab::open_session_tab;
use super::sessions::AgentSessions;
use super::sessions_list::{SessionCard, subject_labels};
use crate::theme::density;
use crate::ui::columns;
use crate::workspace::{Workspace, subject_item};

const TICK: Duration = Duration::from_secs(1);
const STATUS_WIDTH: f32 = 168.;
const FINDINGS_WIDTH: f32 = 150.;
const TIME_WIDTH: f32 = 64.;
const ACTIONS_WIDTH: f32 = 150.;
const ROW_INDENT: f32 = 20.;
const HEADER_INDENT: f32 = 22.;
const GROUP_HEADER_HEIGHT: f32 = 34.;
const GROUP_TITLE_MAX_WIDTH: f32 = 420.;
const ARCHIVED_ROW_HEIGHT: f32 = 32.;

fn open_subject(key: PrKey, kind: SessionKind, window: &mut Window, cx: &mut App) {
    if let Some(workspace) = Workspace::global(cx) {
        _ = workspace.update(cx, |workspace, cx| {
            workspace.open_item(subject_item(key, kind), false, window, cx)
        });
    }
}

fn start_agent(key: PrKey, kind: SessionKind, window: &mut Window, cx: &mut App) {
    if let Some(workspace) = Workspace::global(cx) {
        _ = workspace.update(cx, |workspace, cx| match kind {
            SessionKind::Review | SessionKind::Summary => {
                workspace.review_pr_with_ai(key, window, cx)
            }
            SessionKind::Plan => workspace.plan_issue_with_agent(key, window, cx),
        });
    }
}

pub struct AgentsView {
    focus_handle: FocusHandle,
    sessions: Option<Entity<AgentSessions>>,
    catalog: Option<Entity<SessionCatalog>>,
    expanded: HashSet<PrKey>,
    last_render: Option<Instant>,
    _ticker: Task<()>,
    _subscription: Option<Subscription>,
}

impl AgentsView {
    pub fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
        let catalog = SessionCatalog::global(cx);
        let subscription = catalog
            .as_ref()
            .map(|catalog| cx.observe(catalog, |_, _, cx| cx.notify()));
        let ticker = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(TICK).await;
                let alive = this.update(cx, |this, cx| {
                    if this.is_visible() && this.has_active(cx) {
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    return;
                }
            }
        });
        Self {
            focus_handle: cx.focus_handle(),
            sessions: AgentSessions::global(cx),
            catalog,
            expanded: HashSet::new(),
            last_render: None,
            _ticker: ticker,
            _subscription: subscription,
        }
    }

    fn is_visible(&self) -> bool {
        self.last_render
            .is_some_and(|at| at.elapsed() < TICK + TICK)
    }

    fn has_active(&self, cx: &App) -> bool {
        self.sessions.as_ref().is_some_and(|sessions| {
            sessions
                .read(cx)
                .threads()
                .any(|thread| thread.read(cx).card_status().is_active())
        })
    }

    fn toggle_archived(&mut self, pr: &PrKey, cx: &mut Context<Self>) {
        if !self.expanded.remove(pr) {
            self.expanded.insert(pr.clone());
        }
        cx.notify();
    }

    fn header(&self, total: usize, filter: SessionFilter, cx: &App) -> Div {
        let theme = cx.theme();
        h_flex()
            .flex_none()
            .px_3()
            .py_2()
            .gap_2()
            .items_center()
            .border_b_1()
            .border_color(theme.border)
            .child(
                Icon::new(IconName::Bot)
                    .small()
                    .text_color(theme.muted_foreground),
            )
            .child(div().font_semibold().child(labels::PANEL_TITLE))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(total.to_string()),
            )
            .child(div().flex_1())
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(labels::filter_label(filter)),
            )
    }

    fn column_header(&self, cx: &App) -> Div {
        columns::column_header(cx)
            .pl(px(HEADER_INDENT))
            .child(
                div()
                    .w(px(STATUS_WIDTH))
                    .flex_none()
                    .child(labels::COLUMN_STATUS),
            )
            .child(div().flex_1().min_w_0().child(labels::COLUMN_AGENT))
            .child(
                div()
                    .w(px(FINDINGS_WIDTH))
                    .flex_none()
                    .child(labels::COLUMN_FINDINGS),
            )
            .child(
                h_flex()
                    .w(px(TIME_WIDTH))
                    .flex_none()
                    .justify_end()
                    .child(labels::COLUMN_TIME),
            )
            .child(div().w(px(ACTIONS_WIDTH)).flex_none())
    }

    fn session_row(&self, card: SessionCard, cx: &App) -> Stateful<Div> {
        let theme = cx.theme();
        let id = card.id.clone();
        let error = resume_error(&card.controls, cx);
        let remark = match (error, card.note) {
            (Some(error), _) => Some((SharedString::from(error), theme.danger)),
            (None, Some(note)) => Some((note, theme.warning)),
            (None, None) => None,
        };
        columns::list_row(
            SharedString::from(format!("agents-row-{}", card.id)),
            false,
            cx,
        )
        .pl(px(ROW_INDENT))
        .child(
            div()
                .w(px(STATUS_WIDTH))
                .flex_none()
                .child(status_chip(card.status, cx)),
        )
        .child(
            h_flex()
                .flex_1()
                .min_w_0()
                .items_center()
                .gap(px(10.))
                .child(div().flex_none().child(card.agent))
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(card.model),
                )
                .children(remark.map(|(text, color)| {
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .text_color(color)
                        .child(text)
                })),
        )
        .child(
            div()
                .w(px(FINDINGS_WIDTH))
                .flex_none()
                .child(severity_pills(card.counts, cx)),
        )
        .child(
            h_flex()
                .w(px(TIME_WIDTH))
                .flex_none()
                .justify_end()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(card.elapsed),
        )
        .child(
            row_actions(&card.controls, "agents-row", cx)
                .w(px(ACTIONS_WIDTH))
                .flex_none()
                .justify_end()
                .gap(px(6.)),
        )
        .on_click(move |_, window, cx| open_session_tab(&id, window, cx))
    }

    fn archived_toggle(
        &self,
        pr: &PrKey,
        count: usize,
        expanded: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = cx.theme();
        let target = pr.clone();
        let icon = if expanded {
            IconName::ChevronDown
        } else {
            IconName::ChevronRight
        };
        h_flex()
            .id(SharedString::from(format!(
                "agents-archived-{}-{}",
                pr.host, pr.id.0
            )))
            .h(px(density::scaled(ARCHIVED_ROW_HEIGHT, cx)))
            .flex_none()
            .w_full()
            .pl(px(HEADER_INDENT))
            .pr_3()
            .gap_1p5()
            .items_center()
            .text_xs()
            .text_color(theme.muted_foreground)
            .border_b_1()
            .border_color(theme.border.opacity(0.45))
            .cursor_pointer()
            .hover(|style| style.bg(theme.list_hover))
            .child(Icon::new(icon).xsmall())
            .child(labels::archived_more(count))
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_archived(&target, cx)))
    }

    fn group(&self, group: PrGroup<SessionCard>, cx: &mut Context<Self>) -> Div {
        let first = group.sessions.first().or(group.archived.first());
        let number = first.and_then(|card| card.number);
        let kind = first.map_or(SessionKind::Review, |card| card.kind);
        let (title, reference) = subject_labels(&group.pr, kind, number, cx);
        let expanded = self.expanded.contains(&group.pr);
        let key = format!("{}-{}", group.pr.host, group.pr.id.0);
        let (open_target, start_target) = (group.pr.clone(), group.pr.clone());
        let (icon, open_label, start_label) = match kind {
            SessionKind::Review => (IconName::GitPullRequest, labels::OPEN_PR, labels::REVIEW_PR),
            SessionKind::Plan => (IconName::CircleDot, labels::OPEN_ISSUE, labels::PLAN_ISSUE),
            SessionKind::Summary => (
                IconName::GitPullRequest,
                labels::OPEN_PR,
                labels::SUMMARIZE_PR,
            ),
        };
        let toggle = (!group.archived.is_empty())
            .then(|| self.archived_toggle(&group.pr, group.archived.len(), expanded, cx));
        let rows = group
            .sessions
            .into_iter()
            .chain(group.archived.into_iter().filter(|_| expanded))
            .map(|card| self.session_row(card, cx))
            .collect::<Vec<_>>();
        let theme = cx.theme();
        let header = columns::group_row(cx)
            .h(px(density::scaled(GROUP_HEADER_HEIGHT, cx)))
            .child(Icon::new(icon).small().text_color(theme.muted_foreground))
            .child(
                div()
                    .flex_none()
                    .max_w(px(GROUP_TITLE_MAX_WIDTH))
                    .truncate()
                    .text_sm()
                    .font_semibold()
                    .child(title),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_family(theme.mono_font_family.clone())
                    .text_xs()
                    .font_normal()
                    .text_color(theme.muted_foreground)
                    .child(reference),
            )
            .child(
                Button::new(SharedString::from(format!("agents-open-subject-{key}")))
                    .icon(Icon::new(IconName::ExternalLink))
                    .label(open_label)
                    .xsmall()
                    .ghost()
                    .on_click(move |_, window, cx| {
                        open_subject(open_target.clone(), kind, window, cx)
                    }),
            )
            .child(
                Button::new(SharedString::from(format!("agents-start-agent-{key}")))
                    .icon(Icon::new(IconName::Sparkles))
                    .label(start_label)
                    .xsmall()
                    .outline()
                    .on_click(move |_, window, cx| {
                        start_agent(start_target.clone(), kind, window, cx)
                    }),
            );
        v_flex()
            .w_full()
            .child(header)
            .children(rows)
            .children(toggle)
    }
}

impl Focusable for AgentsView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for AgentsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.last_render = Some(Instant::now());
        let (cards, filter, loaded) = match &self.catalog {
            Some(catalog) => {
                let catalog = catalog.read(cx);
                (
                    catalog.cards(Utc::now(), cx),
                    catalog.filter(),
                    catalog.is_loaded(),
                )
            }
            None => (Vec::new(), SessionFilter::All, false),
        };
        let total = cards.len();
        let visible = cards
            .iter()
            .filter(|card| card.status.matches(filter))
            .count();
        let groups = group_by_pr(cards, filter);
        let header = self.header(visible, filter, cx);
        let empty = groups.is_empty().then(|| {
            let text = match (loaded, total) {
                (false, 0) => labels::LOADING_SESSIONS,
                (_, 0) => labels::NO_SESSIONS,
                _ => labels::NO_SESSIONS_IN_FILTER,
            };
            div()
                .p_3()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(text)
        });
        let columns = (!groups.is_empty()).then(|| self.column_header(cx));
        let groups = groups
            .into_iter()
            .map(|group| self.group(group, cx))
            .collect::<Vec<_>>();
        v_flex()
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(cx.theme().background)
            .child(header)
            .children(columns)
            .child(
                v_flex()
                    .id("agents-view-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(groups)
                    .children(empty),
            )
    }
}

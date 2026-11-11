use std::collections::HashSet;
use std::time::{Duration, Instant};

use argon_core::PrKey;
use chrono::Utc;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::{
    App, Context, Div, Entity, FocusHandle, Focusable, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, Stateful, StatefulInteractiveElement as _,
    Styled as _, Subscription, Task, Window, div,
};

use super::badges::{severity_counts, status_chip};
use super::card_status::SessionFilter;
use super::catalog::SessionCatalog;
use super::grouping::{PrGroup, group_by_pr};
use super::panel_labels as labels;
use super::session_actions::controls;
use super::session_tab::open_session_tab;
use super::sessions::AgentSessions;
use super::sessions_list::{SessionCard, pr_labels};
use crate::center::CenterItem;
use crate::workspace::Workspace;

const TICK: Duration = Duration::from_secs(1);

fn open_pr(pr: PrKey, window: &mut Window, cx: &mut App) {
    if let Some(workspace) = Workspace::global(cx) {
        _ = workspace.update(cx, |workspace, cx| {
            workspace.open_item(CenterItem::Pr(pr), false, window, cx)
        });
    }
}

fn review_pr(pr: PrKey, window: &mut Window, cx: &mut App) {
    if let Some(workspace) = Workspace::global(cx) {
        _ = workspace.update(cx, |workspace, cx| {
            workspace.review_pr_with_ai(pr, window, cx)
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

    fn session_row(&self, card: SessionCard, cx: &App) -> Stateful<Div> {
        let theme = cx.theme();
        let id = card.id.clone();
        let actions = controls(&card.controls, "agents-row", cx);
        let detail = (card.counts.total() > 0 || card.note.is_some()).then(|| {
            h_flex()
                .gap_2()
                .items_center()
                .children((card.counts.total() > 0).then(|| severity_counts(card.counts, cx)))
                .children(card.note.clone().map(|note| {
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .text_color(theme.warning)
                        .child(note)
                }))
        });
        v_flex()
            .id(SharedString::from(format!("agents-row-{}", card.id)))
            .w_full()
            .pl_8()
            .pr_3()
            .py_1p5()
            .gap_1()
            .cursor_pointer()
            .hover(|style| style.bg(theme.list_hover))
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(status_chip(card.status, cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(card.agent_line),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(card.elapsed),
                    ),
            )
            .children(detail)
            .children(actions)
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
            .w_full()
            .pl_8()
            .pr_3()
            .py_1()
            .gap_1p5()
            .items_center()
            .text_xs()
            .text_color(theme.muted_foreground)
            .cursor_pointer()
            .hover(|style| style.bg(theme.list_hover))
            .child(Icon::new(icon).xsmall())
            .child(labels::archived_more(count))
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_archived(&target, cx)))
    }

    fn group(&self, group: PrGroup<SessionCard>, cx: &mut Context<Self>) -> Div {
        let number = group
            .sessions
            .first()
            .or(group.archived.first())
            .and_then(|card| card.number);
        let (title, reference) = pr_labels(&group.pr, number, cx);
        let expanded = self.expanded.contains(&group.pr);
        let key = format!("{}-{}", group.pr.host, group.pr.id.0);
        let (open_target, review_target) = (group.pr.clone(), group.pr.clone());
        let toggle = (!group.archived.is_empty())
            .then(|| self.archived_toggle(&group.pr, group.archived.len(), expanded, cx));
        let rows = group
            .sessions
            .into_iter()
            .chain(group.archived.into_iter().filter(|_| expanded))
            .map(|card| self.session_row(card, cx))
            .collect::<Vec<_>>();
        let theme = cx.theme();
        let header = h_flex()
            .gap_2()
            .px_3()
            .py_1p5()
            .items_center()
            .child(
                Icon::new(IconName::GitPullRequest)
                    .small()
                    .text_color(theme.muted_foreground),
            )
            .child(
                div()
                    .min_w_0()
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
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(reference),
            )
            .child(
                Button::new(SharedString::from(format!("agents-open-pr-{key}")))
                    .icon(Icon::new(IconName::ExternalLink))
                    .label(labels::OPEN_PR)
                    .xsmall()
                    .ghost()
                    .on_click(move |_, window, cx| open_pr(open_target.clone(), window, cx)),
            )
            .child(
                Button::new(SharedString::from(format!("agents-review-pr-{key}")))
                    .icon(Icon::new(IconName::Sparkles))
                    .label(labels::REVIEW_PR)
                    .xsmall()
                    .outline()
                    .on_click(move |_, window, cx| review_pr(review_target.clone(), window, cx)),
            );
        v_flex()
            .w_full()
            .pb_2()
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
        let groups = groups
            .into_iter()
            .map(|group| self.group(group, cx))
            .collect::<Vec<_>>();
        v_flex()
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(cx.theme().background)
            .child(header)
            .child(
                v_flex()
                    .id("agents-view-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .py_2()
                    .children(groups)
                    .children(empty),
            )
    }
}

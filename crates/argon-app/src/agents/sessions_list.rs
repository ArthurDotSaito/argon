use argon_core::PrKey;
use argon_store::SessionRow;
use chrono::{DateTime, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{
    ActiveTheme as _, Icon, Selectable as _, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::{
    App, Context, Div, InteractiveElement as _, ParentElement as _, SharedString, Stateful,
    StatefulInteractiveElement as _, Styled as _, div,
};

use super::badges::{severity_counts, status_chip};
use super::card_status::{CardStatus, SessionFilter, card_status, resume_action};
use super::finding_set::SeverityCounts;
use super::lifecycle::APP_CLOSED_REASON;
use super::panel::AgentPanel;
use super::panel_labels as labels;
use super::session_actions::{SessionControls, controls};
use super::thread_model::ThreadModel;
use super::transcript::{Item, PermissionState};
use crate::bottom::duration_label;
use crate::inbox::InboxModel;

pub struct SessionCard {
    pub id: String,
    pub title: SharedString,
    pub reference: SharedString,
    pub agent_line: SharedString,
    pub status: CardStatus,
    pub counts: SeverityCounts,
    pub elapsed: SharedString,
    pub created_at: DateTime<Utc>,
    pub note: Option<SharedString>,
    pub controls: SessionControls,
}

pub fn pr_labels(pr: &PrKey, number: Option<u64>, cx: &App) -> (SharedString, SharedString) {
    let found = InboxModel::global(cx).and_then(|inbox| inbox.read(cx).pr(pr).cloned());
    match found {
        Some(pr) => (
            pr.title.into(),
            format!("{} #{}", pr.repo, pr.number).into(),
        ),
        None => (
            labels::UNKNOWN_PR.into(),
            number.map_or_else(String::new, |n| format!("#{n}")).into(),
        ),
    }
}

impl SessionCard {
    pub fn from_thread(thread: &ThreadModel, now: DateTime<Utc>, cx: &App) -> Self {
        let info = thread.info();
        let (title, reference) = pr_labels(&info.pr, info.number, cx);
        let note = thread
            .transcript()
            .items()
            .iter()
            .find_map(|item| match item {
                Item::Permission(permission) if permission.state == PermissionState::Pending => {
                    Some(SharedString::from(permission.title.clone()))
                }
                _ => None,
            });
        Self {
            id: info.session_id.clone(),
            title,
            reference,
            agent_line: format!("{} · {}", info.agent_label, info.model_label).into(),
            status: thread.card_status(),
            counts: thread.counts(),
            elapsed: duration_label(thread.elapsed(now)).into(),
            created_at: info.created_at,
            note,
            controls: SessionControls {
                id: info.session_id.clone(),
                status: thread.card_status(),
                resume: thread.resume_action(),
                worktree: info.worktree.clone(),
            },
        }
    }

    pub fn from_row(
        row: &SessionRow,
        counts: SeverityCounts,
        worktree_present: bool,
        cx: &App,
    ) -> Self {
        let info = super::panel::session_info(row, worktree_present, cx);
        let status = card_status(row.status, false, 0);
        let app_closed = row.failure_reason.as_deref() == Some(APP_CLOSED_REASON);
        let resume = resume_action(
            status,
            info.resumable && info.worktree.is_some(),
            app_closed,
        );
        let (title, reference) = pr_labels(&row.pr, info.number, cx);
        Self {
            id: row.id.clone(),
            title,
            reference,
            agent_line: format!("{} · {}", info.agent_label, info.model_label).into(),
            status,
            counts,
            elapsed: match status {
                CardStatus::Archived => SharedString::default(),
                _ => duration_label(row.updated_at - row.created_at).into(),
            },
            created_at: row.created_at,
            note: row.failure_reason.clone().map(SharedString::from),
            controls: SessionControls {
                id: row.id.clone(),
                status,
                resume,
                worktree: info.worktree.clone(),
            },
        }
    }
}

fn card(card: SessionCard, cx: &mut Context<AgentPanel>) -> Stateful<Div> {
    let theme = cx.theme();
    let id = card.id.clone();
    let actions = controls(&card.controls, "card", cx);
    let foot = h_flex()
        .gap_2()
        .items_center()
        .children((card.counts.total() > 0).then(|| severity_counts(card.counts, cx)))
        .children(card.note.map(|note| {
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_xs()
                .text_color(theme.warning)
                .child(note)
        }));
    v_flex()
        .id(SharedString::from(format!("agent-session-{}", card.id)))
        .w_full()
        .p_2()
        .gap_1()
        .rounded(theme.radius)
        .border_1()
        .border_color(theme.border)
        .bg(theme.secondary.opacity(0.4))
        .cursor_pointer()
        .hover(|style| style.bg(theme.list_hover))
        .child(
            h_flex()
                .gap_1p5()
                .items_center()
                .child(status_chip(card.status, cx))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(card.reference),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(card.elapsed),
                ),
        )
        .child(
            div()
                .text_sm()
                .font_semibold()
                .line_clamp(2)
                .child(card.title),
        )
        .child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(card.agent_line),
        )
        .child(foot)
        .children(actions)
        .on_click(cx.listener(move |this, _, window, cx| this.open_thread(&id, window, cx)))
}

fn filter_bar(filter: SessionFilter, cards: &[SessionCard], cx: &mut Context<AgentPanel>) -> Div {
    let counts = SessionFilter::ALL.map(|option| {
        (
            option,
            cards
                .iter()
                .filter(|card| card.status.matches(option))
                .count(),
        )
    });
    h_flex()
        .gap_1()
        .children(counts.into_iter().map(|(option, count)| {
            Button::new(SharedString::from(format!("agents-filter-{option:?}")))
                .label(format!("{} {count}", labels::filter_label(option)))
                .xsmall()
                .ghost()
                .selected(option == filter)
                .on_click(cx.listener(move |this, _, _, cx| this.set_filter(option, cx)))
        }))
}

pub fn render_list(
    mut cards: Vec<SessionCard>,
    filter: SessionFilter,
    loaded: bool,
    cx: &mut Context<AgentPanel>,
) -> Div {
    cards.sort_by_key(|card| std::cmp::Reverse(card.created_at));
    let filters = filter_bar(filter, &cards, cx);
    let total = cards.len();
    let visible: Vec<SessionCard> = cards
        .into_iter()
        .filter(|card| card.status.matches(filter))
        .collect();
    let empty_state = visible.is_empty();
    let rendered: Vec<Stateful<Div>> = visible.into_iter().map(|item| card(item, cx)).collect();
    let theme = cx.theme();
    let empty = empty_state.then(|| {
        let text = match (loaded, total) {
            (false, 0) => labels::LOADING_SESSIONS,
            (_, 0) => labels::NO_SESSIONS,
            _ => labels::NO_SESSIONS_IN_FILTER,
        };
        div()
            .p_2()
            .text_sm()
            .text_color(theme.muted_foreground)
            .child(text)
    });
    let header = h_flex()
        .flex_none()
        .px_2()
        .py_1p5()
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
                .flex_1()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(total.to_string()),
        )
        .child(
            Button::new("agents-new-review-icon")
                .icon(Icon::new(IconName::Plus))
                .xsmall()
                .ghost()
                .tooltip(labels::NEW_REVIEW_TOOLTIP)
                .on_click(|_, window, cx| AgentPanel::new_review(window, cx)),
        );
    v_flex()
        .size_full()
        .child(header)
        .child(div().flex_none().px_2().py_1p5().child(filters))
        .child(
            v_flex()
                .id("agents-session-list")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .px_2()
                .pb_2()
                .gap_2()
                .children(rendered)
                .children(empty),
        )
        .child(
            div()
                .flex_none()
                .p_2()
                .border_t_1()
                .border_color(theme.border)
                .child(
                    Button::new("agents-new-review")
                        .icon(Icon::new(IconName::Sparkles))
                        .label(labels::NEW_REVIEW)
                        .small()
                        .outline()
                        .w_full()
                        .on_click(|_, window, cx| AgentPanel::new_review(window, cx)),
                ),
        )
}

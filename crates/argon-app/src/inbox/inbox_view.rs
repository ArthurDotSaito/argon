use std::time::Duration;

use chrono::Utc;
use gpui_kit::assets::IconName as AppIconName;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Div, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ListAlignment, ListOffset, ListState, ParentElement as _,
    Render, StatefulInteractiveElement as _, Styled as _, Subscription, Task, WeakEntity, Window,
    div, list, px,
};

use super::filter::{self, KindFilter};
use super::labels;
use super::model::InboxModel;
use super::row;
use super::sections::InboxRow;
use crate::actions::{
    ArchiveSelected, ClearFilter, FocusFilter, OpenSelected, SelectNext, SelectPrevious, ToggleRead,
};
use crate::keymap::{INBOX_CONTEXT, INBOX_LIST_CONTEXT};
use crate::theme::density;

const LIST_OVERDRAW: f32 = 260.;
const FILTER_WIDTH: f32 = 320.;
const KIND_SEGMENT_HEIGHT: f32 = 24.;
const KIND_OPTIONS: [(KindFilter, &str, Option<AppIconName>); 3] = [
    (KindFilter::All, labels::KIND_ALL, None),
    (
        KindFilter::Prs,
        labels::KIND_PRS,
        Some(AppIconName::GitPullRequest),
    ),
    (
        KindFilter::Issues,
        labels::KIND_ISSUES,
        Some(AppIconName::CircleDot),
    ),
];
const SINGLE_CLICK_DELAY: Duration = Duration::from_millis(300);

pub struct InboxPane {
    model: Entity<InboxModel>,
    filter: Entity<InputState>,
    list_state: ListState,
    focus_handle: FocusHandle,
    scroll_generation: u64,
    rows_revision: u64,
    selected_row: Option<usize>,
    pending_click: Option<Task<()>>,
    _subscriptions: [Subscription; 3],
}

impl InboxPane {
    pub fn new(model: Entity<InboxModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let filter =
            cx.new(|cx| InputState::new(window, cx).placeholder(labels::FILTER_PLACEHOLDER));
        let filter_subscription = cx.subscribe_in(&filter, window, Self::on_filter_event);
        let model_subscription = cx.observe(&model, |this, _, cx| this.sync_list(cx));
        let density_subscription = density::observe(cx, Self::on_density_change);
        let (rows, scroll_generation, rows_revision, selected_row) = {
            let model = model.read(cx);
            (
                model.rows().len(),
                model.scroll_generation(),
                model.rows_revision(),
                model.selected_row(),
            )
        };
        let list_state = ListState::new(0, ListAlignment::Top, px(LIST_OVERDRAW));
        list_state.reset_with_uniform_height(rows, px(row::pr_row_height(cx)));
        Self {
            model,
            filter,
            list_state,
            focus_handle: cx.focus_handle(),
            scroll_generation,
            rows_revision,
            selected_row,
            pending_click: None,
            _subscriptions: [
                filter_subscription,
                model_subscription,
                density_subscription,
            ],
        }
    }

    fn sync_list(&mut self, cx: &mut Context<Self>) {
        let model = self.model.read(cx);
        let rows = model.rows().len();
        let (scroll_generation, rows_revision) = (model.scroll_generation(), model.rows_revision());
        let selected_row = model.selected_row();
        let reset = scroll_generation != self.scroll_generation;
        if reset {
            self.pending_click = None;
            self.list_state
                .reset_with_uniform_height(rows, px(row::pr_row_height(cx)));
        } else if rows_revision != self.rows_revision {
            let top = self.list_state.logical_scroll_top();
            self.list_state
                .reset_with_uniform_height(rows, px(row::pr_row_height(cx)));
            self.list_state.scroll_to(ListOffset {
                item_ix: top.item_ix.min(rows.saturating_sub(1)),
                offset_in_item: top.offset_in_item,
            });
        }
        let laid_out = self.list_state.viewport_bounds().size.height > px(0.);
        if !reset
            && laid_out
            && self.selected_row.is_some()
            && selected_row != self.selected_row
            && let Some(row) = selected_row
        {
            self.list_state.scroll_to_reveal_item(row);
        }
        self.scroll_generation = scroll_generation;
        self.rows_revision = rows_revision;
        self.selected_row = selected_row;
        cx.notify();
    }

    fn on_density_change(&mut self, cx: &mut Context<Self>) {
        let rows = self.model.read(cx).rows().len();
        let top = self.list_state.logical_scroll_top();
        self.list_state
            .reset_with_uniform_height(rows, px(row::pr_row_height(cx)));
        self.list_state.scroll_to(ListOffset {
            item_ix: top.item_ix.min(rows.saturating_sub(1)),
            offset_in_item: px(0.),
        });
        cx.notify();
    }

    fn on_filter_event(
        &mut self,
        filter: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => {
                let query = filter.read(cx).value();
                self.model
                    .update(cx, |model, cx| model.set_query(&query, cx));
            }
            InputEvent::PressEnter { .. } => self.focus_handle.focus(window, cx),
            _ => {}
        }
    }

    fn on_row_click(
        &mut self,
        position: usize,
        click_count: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pending_click = None;
        self.focus_handle.focus(window, cx);
        self.model
            .update(cx, |model, cx| model.select(position, cx));
        if click_count >= 2 {
            self.pending_click = None;
            self.model
                .update(cx, |model, cx| model.open(position, false, cx));
            return;
        }
        let Some(id) = self
            .model
            .read(cx)
            .item_at(position)
            .map(|item| item.subject.key())
        else {
            return;
        };
        let model = self.model.downgrade();
        self.pending_click = Some(cx.spawn(async move |_, cx| {
            cx.background_executor().timer(SINGLE_CLICK_DELAY).await;
            _ = model.update(cx, |model, cx| model.open_subject(&id, true, cx));
        }));
    }

    fn on_select_next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        self.pending_click = None;
        self.model.update(cx, |model, cx| model.select_next(cx));
    }

    fn on_select_previous(&mut self, _: &SelectPrevious, _: &mut Window, cx: &mut Context<Self>) {
        self.pending_click = None;
        self.model.update(cx, |model, cx| model.select_previous(cx));
    }

    fn on_open_selected(&mut self, _: &OpenSelected, _: &mut Window, cx: &mut Context<Self>) {
        self.pending_click = None;
        self.model
            .update(cx, |model, cx| model.open_selected(true, cx));
    }

    fn on_archive_selected(&mut self, _: &ArchiveSelected, _: &mut Window, cx: &mut Context<Self>) {
        self.model
            .update(cx, |model, cx| model.archive_selected(cx));
    }

    fn on_toggle_read(&mut self, _: &ToggleRead, _: &mut Window, cx: &mut Context<Self>) {
        self.model
            .update(cx, |model, cx| model.toggle_read_selected(cx));
    }

    fn on_focus_filter(&mut self, _: &FocusFilter, window: &mut Window, cx: &mut Context<Self>) {
        self.filter
            .update(cx, |filter, cx| filter.focus(window, cx));
    }

    fn on_clear_filter(&mut self, _: &ClearFilter, window: &mut Window, cx: &mut Context<Self>) {
        self.filter
            .update(cx, |filter, cx| filter.set_value("", window, cx));
        self.model.update(cx, |model, cx| model.set_query("", cx));
        self.focus_handle.focus(window, cx);
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let view = self.model.read(cx).view();
        let kind_segment = filter::applies_to(view).then(|| self.kind_segment(cx));
        let theme = cx.theme();
        let model = self.model.read(cx);
        let org_chip = model.org_filter().map(|org| {
            let inbox = self.model.downgrade();
            h_flex()
                .id("inbox-org-chip")
                .gap_1()
                .px_1p5()
                .h(px(20.))
                .items_center()
                .rounded(theme.radius)
                .border_1()
                .border_color(theme.border)
                .text_xs()
                .text_color(theme.muted_foreground)
                .cursor_pointer()
                .hover(|style| style.text_color(theme.foreground))
                .child(org.name.clone())
                .child(Icon::new(IconName::Close).xsmall())
                .on_click(move |_, _, cx| {
                    _ = inbox.update(cx, |model, cx| model.clear_org(cx));
                })
        });
        h_flex()
            .h(px(40.))
            .px_3()
            .gap_2()
            .items_center()
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .text_base()
                    .font_semibold()
                    .text_color(theme.foreground)
                    .child(labels::view_title(view)),
            )
            .child(
                div()
                    .text_color(theme.muted_foreground)
                    .child(model.visible_count().to_string()),
            )
            .children(org_chip)
            .children(kind_segment)
            .child(div().flex_1())
            .child(
                div().w(px(FILTER_WIDTH)).child(
                    Input::new(&self.filter)
                        .small()
                        .prefix(Icon::new(IconName::Search).xsmall())
                        .suffix(
                            div()
                                .px_1()
                                .rounded(theme.radius)
                                .border_1()
                                .border_color(theme.border)
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(labels::FILTER_SHORTCUT),
                        ),
                ),
            )
    }

    fn kind_segment(&self, cx: &App) -> Div {
        let theme = cx.theme();
        let current = self.model.read(cx).kind_filter();
        h_flex()
            .h(px(KIND_SEGMENT_HEIGHT))
            .flex_none()
            .rounded(theme.radius)
            .border_1()
            .border_color(theme.border)
            .overflow_hidden()
            .children(
                KIND_OPTIONS
                    .into_iter()
                    .enumerate()
                    .map(|(ix, (kind, label, icon))| {
                        let selected = kind == current;
                        let model = self.model.downgrade();
                        h_flex()
                            .id(("inbox-kind", ix))
                            .px(px(9.))
                            .gap(px(5.))
                            .items_center()
                            .text_xs()
                            .cursor_pointer()
                            .when(ix > 0, |this| this.border_l_1().border_color(theme.border))
                            .map(|this| {
                                if selected {
                                    this.bg(theme.accent).text_color(theme.foreground)
                                } else {
                                    this.text_color(theme.muted_foreground)
                                        .hover(|style| style.text_color(theme.foreground))
                                }
                            })
                            .children(icon.map(|icon| Icon::new(icon).xsmall()))
                            .child(label)
                            .on_click(move |_, _, cx| {
                                _ = model.update(cx, |model, cx| model.set_kind_filter(kind, cx));
                            })
                    }),
            )
    }

    fn body(&self, cx: &mut Context<Self>) -> AnyElement {
        let model = self.model.read(cx);
        let message = if !model.is_loaded() && model.error().is_none() {
            Some(labels::LOADING)
        } else if model.rows().is_empty() && model.error().is_none() {
            Some(if model.has_query() {
                labels::EMPTY_FILTER
            } else {
                labels::empty_view(model.view(), model.kind_filter())
            })
        } else {
            None
        };
        if let Some(message) = message {
            return v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .text_color(cx.theme().muted_foreground)
                .child(message)
                .into_any_element();
        }
        let inbox = self.model.clone();
        let pane = cx.weak_entity();
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .child(
                list(self.list_state.clone(), move |ix, _, cx| {
                    render_list_row(&inbox, &pane, ix, cx)
                })
                .size_full(),
            )
            .vertical_scrollbar(&self.list_state)
            .into_any_element()
    }
}

fn render_list_row(
    inbox: &Entity<InboxModel>,
    pane: &WeakEntity<InboxPane>,
    ix: usize,
    cx: &mut App,
) -> AnyElement {
    let model = inbox.read(cx);
    match model.rows().get(ix) {
        Some(InboxRow::Header { title, count }) => {
            row::header(title.clone(), *count, cx).into_any_element()
        }
        Some(InboxRow::Item { position }) => {
            let position = *position;
            let Some(item) = model.item_at(position) else {
                return div().into_any_element();
            };
            let pane = pane.clone();
            row::item_row(position, item, model.is_selected(position), Utc::now(), cx)
                .on_click(move |event: &ClickEvent, window, cx| {
                    _ = pane.update(cx, |pane, cx| {
                        pane.on_row_click(position, event.click_count(), window, cx)
                    });
                })
                .into_any_element()
        }
        Some(InboxRow::LoadMore) => {
            let target = inbox.downgrade();
            row::load_more(
                model.is_loading_more(),
                move |_, _, cx| {
                    _ = target.update(cx, |model, cx| model.load_more(cx));
                },
                cx,
            )
            .into_any_element()
        }
        None => div().into_any_element(),
    }
}

impl Focusable for InboxPane {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for InboxPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let error = self.model.read(cx).error().cloned();
        let banner = error.map(|error| {
            div()
                .px_3()
                .py_1()
                .text_xs()
                .bg(theme.danger.opacity(0.15))
                .text_color(theme.danger)
                .child(error)
        });
        v_flex()
            .key_context(INBOX_CONTEXT)
            .size_full()
            .bg(theme.background)
            .on_action(cx.listener(Self::on_clear_filter))
            .child(self.header(cx))
            .children(banner)
            .child(
                v_flex()
                    .key_context(INBOX_LIST_CONTEXT)
                    .track_focus(&self.focus_handle)
                    .flex_1()
                    .min_h_0()
                    .on_action(cx.listener(Self::on_select_next))
                    .on_action(cx.listener(Self::on_select_previous))
                    .on_action(cx.listener(Self::on_open_selected))
                    .on_action(cx.listener(Self::on_archive_selected))
                    .on_action(cx.listener(Self::on_toggle_read))
                    .on_action(cx.listener(Self::on_focus_filter))
                    .child(self.body(cx)),
            )
    }
}

use std::time::Duration;

use chrono::Utc;
use gpui_kit::assets::IconName as AppIconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::resize_handle;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Side, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, Axis, Canvas, ClickEvent, Context, Div, DragMoveEvent, Empty,
    Entity, FocusHandle, Focusable, InteractiveElement as _, IntoElement, ListAlignment,
    ListOffset, ListState, MouseButton, MouseDownEvent, ParentElement as _, Pixels, Render,
    SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Task, WeakEntity,
    Window, canvas, div, list, px,
};

use super::browse::IssueScope;
use super::columns::{self, Column, ColumnLayout, ColumnWidths};
use super::columns_store::{self, ColumnsSaver};
use super::filter::{self, KindFilter};
use super::labels;
use super::model::InboxModel;
use super::row;
use super::sections::InboxRow;
use crate::actions::{
    ArchiveSelected, ClearFilter, FocusFilter, OpenSelected, SelectNext, SelectPrevious, ToggleRead,
};
use crate::keymap::{INBOX_CONTEXT, INBOX_LIST_CONTEXT};
use crate::pr::labels as pr_labels;
use crate::theme::density;
use crate::ui::columns::ROW_HORIZONTAL_PADDING;

const LIST_OVERDRAW: f32 = 260.;
const LIST_WIDTH_TOLERANCE: f32 = 0.5;
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

#[derive(Clone)]
struct ResizeColumn(Column);

impl Render for ResizeColumn {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

struct ResizeOrigin {
    column: Column,
    x: Pixels,
    width: f32,
}

pub struct InboxPane {
    model: Entity<InboxModel>,
    filter: Entity<InputState>,
    list_state: ListState,
    focus_handle: FocusHandle,
    scroll_generation: u64,
    rows_revision: u64,
    selected_row: Option<usize>,
    pending_click: Option<Task<()>>,
    column_widths: ColumnWidths,
    list_width: Option<f32>,
    resize_origin: Option<ResizeOrigin>,
    columns_saver: ColumnsSaver,
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
            column_widths: columns_store::load(),
            list_width: None,
            resize_origin: None,
            columns_saver: ColumnsSaver::default(),
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

    fn available_width(&self) -> Option<f32> {
        self.list_width.map(|width| width - ROW_HORIZONTAL_PADDING)
    }

    fn column_layout(&self) -> ColumnLayout {
        columns::fit(self.available_width(), &self.column_widths)
    }

    fn set_column_width(&mut self, column: Column, width: f32, cx: &mut Context<Self>) {
        let widths = columns::resized(self.column_widths, column, width, self.available_width());
        self.apply_column_widths(widths, cx);
    }

    fn apply_column_widths(&mut self, widths: ColumnWidths, cx: &mut Context<Self>) {
        if widths == self.column_widths {
            return;
        }
        self.column_widths = widths;
        self.columns_saver.schedule(widths, cx);
        cx.notify();
    }

    fn on_resize_press(&mut self, column: Column, event: &MouseDownEvent, cx: &mut Context<Self>) {
        if event.click_count >= 2 {
            self.resize_origin = None;
            let widths = self.column_widths.with(column, column.default_width());
            self.apply_column_widths(widths, cx);
            return;
        }
        self.resize_origin = self
            .column_layout()
            .width(column)
            .map(|width| ResizeOrigin {
                column,
                x: event.position.x,
                width,
            });
    }

    fn on_resize_drag(
        &mut self,
        event: &DragMoveEvent<ResizeColumn>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let column = event.drag(cx).0;
        let Some(origin) = self
            .resize_origin
            .as_ref()
            .filter(|origin| origin.column == column)
        else {
            return;
        };
        let moved = f32::from(event.event.position.x - origin.x);
        let width = origin.width - moved;
        self.set_column_width(column, width, cx);
    }

    fn resize_handle(&self, column: Column, cx: &Context<Self>) -> AnyElement {
        resize_handle::<ResizeColumn, ResizeColumn>(
            ("inbox-column-resize", column.index()),
            Axis::Horizontal,
        )
        .on_drag(ResizeColumn(column), |drag, _, _, cx| {
            cx.stop_propagation();
            cx.new(|_| drag.clone())
        })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                this.on_resize_press(column, event, cx);
            }),
        )
        .into_any_element()
    }

    fn width_probe(&self, cx: &Context<Self>) -> Canvas<()> {
        let pane = cx.weak_entity();
        let known = self.list_width;
        canvas(
            move |bounds, window, cx| {
                let width = f32::from(bounds.size.width);
                if known.is_some_and(|known| (known - width).abs() <= LIST_WIDTH_TOLERANCE) {
                    return;
                }
                window.defer(cx, move |_, cx| {
                    _ = pane.update(cx, |this, cx| {
                        this.list_width = Some(width);
                        cx.notify();
                    });
                });
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full()
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let model = self.model.read(cx);
        let view = model.view();
        let can_browse = model.can_browse();
        let browsing = can_browse && model.issue_scope() == IssueScope::AllOpen;
        let kind = model.kind_filter();
        let scope = model.issue_scope();
        let kind_segment = filter::applies_to(view).then(|| {
            self.segment(
                "inbox-kind",
                KIND_OPTIONS,
                kind,
                InboxModel::set_kind_filter,
                cx,
            )
        });
        let scope_menu = can_browse.then(|| self.scope_menu(scope, model.closed_window_days(), cx));
        let refresh = browsing.then(|| self.refresh_button(cx));
        let theme = cx.theme();
        let count = match model.browse_list() {
            Some(list) if list.loaded => labels::browse_count(model.visible_count(), list.total),
            _ => model.visible_count().to_string(),
        };
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
            .child(div().text_color(theme.muted_foreground).child(count))
            .children(scope_menu)
            .children(refresh)
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

    fn segment<T: Copy + PartialEq + 'static>(
        &self,
        id: &'static str,
        options: impl IntoIterator<Item = (T, &'static str, Option<AppIconName>)>,
        current: T,
        select: fn(&mut InboxModel, T, &mut Context<InboxModel>),
        cx: &Context<Self>,
    ) -> Div {
        let theme = cx.theme();
        h_flex()
            .h(px(KIND_SEGMENT_HEIGHT))
            .flex_none()
            .rounded(theme.radius)
            .border_1()
            .border_color(theme.border)
            .overflow_hidden()
            .children(
                options
                    .into_iter()
                    .enumerate()
                    .map(|(ix, (value, label, icon))| {
                        let selected = value == current;
                        h_flex()
                            .id((id, ix))
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
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.model.update(cx, |model, cx| select(model, value, cx));
                                this.focus_handle.focus(window, cx);
                            }))
                    }),
            )
    }

    fn scope_menu(
        &self,
        scope: IssueScope,
        window_days: i64,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let pane = cx.weak_entity();
        Button::new("inbox-scope-menu")
            .label(labels::scope_menu_label(scope))
            .child(Icon::new(IconName::ChevronDown).xsmall())
            .xsmall()
            .ghost()
            .text_color(cx.theme().muted_foreground)
            .dropdown_menu(move |menu, _, _| {
                menu.check_side(Side::Right)
                    .item(scope_item(
                        &pane,
                        IssueScope::Recent,
                        scope,
                        AppIconName::Clock,
                        labels::SCOPE_RECENT,
                        labels::scope_recent_hint(window_days),
                    ))
                    .item(scope_item(
                        &pane,
                        IssueScope::AllOpen,
                        scope,
                        AppIconName::List,
                        labels::SCOPE_ALL_OPEN,
                        labels::SCOPE_ALL_OPEN_HINT.to_string(),
                    ))
            })
    }

    fn refresh_button(&self, cx: &Context<Self>) -> Button {
        Button::new("inbox-browse-refresh")
            .icon(Icon::new(AppIconName::RefreshCw))
            .xsmall()
            .ghost()
            .loading(self.model.read(cx).is_browse_loading())
            .tooltip(pr_labels::REFRESH)
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                this.model.update(cx, |model, cx| model.browse_refresh(cx));
                this.focus_handle.focus(window, cx);
            }))
    }

    fn browse_banner(&self, error: SharedString, cx: &Context<Self>) -> Div {
        banner(cx)
            .child(div().flex_1().min_w_0().truncate().child(error))
            .child(
                Button::new("inbox-browse-retry")
                    .label(pr_labels::RETRY)
                    .xsmall()
                    .ghost()
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.model.update(cx, |model, cx| model.browse_retry(cx));
                        this.focus_handle.focus(window, cx);
                    })),
            )
    }

    fn body(&self, cx: &mut Context<Self>) -> AnyElement {
        let model = self.model.read(cx);
        let message = if let Some(list) = model.browse_list() {
            (model.rows().is_empty() && list.error.is_none()).then(|| {
                if list.loaded {
                    labels::empty_browse(model.has_query())
                } else {
                    labels::LOADING
                }
            })
        } else if !model.is_loaded() && model.error().is_none() {
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
        let kinds = filter::shown_kinds(model.view(), model.kind_filter());
        let inbox = self.model.clone();
        let pane = cx.weak_entity();
        let layout = self.column_layout();
        let column_header =
            row::column_header(kinds, &layout, |column| self.resize_handle(column, cx), cx);
        v_flex()
            .flex_1()
            .min_h_0()
            .child(column_header)
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(self.width_probe(cx))
                    .child(
                        list(self.list_state.clone(), move |ix, _, cx| {
                            render_list_row(&inbox, &pane, &layout, ix, cx)
                        })
                        .size_full(),
                    )
                    .vertical_scrollbar(&self.list_state),
            )
            .into_any_element()
    }
}

fn render_list_row(
    inbox: &Entity<InboxModel>,
    pane: &WeakEntity<InboxPane>,
    layout: &ColumnLayout,
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
            let selected = model.is_selected(position);
            row::item_row(position, item, selected, layout, Utc::now(), cx)
                .on_click(move |event: &ClickEvent, window, cx| {
                    _ = pane.update(cx, |pane, cx| {
                        pane.on_row_click(position, event.click_count(), window, cx)
                    });
                })
                .into_any_element()
        }
        Some(InboxRow::LoadMore) => {
            more_row(inbox, model.is_loading_more(), InboxModel::load_more, cx)
        }
        Some(InboxRow::BrowseMore) => more_row(
            inbox,
            model.is_browse_loading(),
            InboxModel::browse_more,
            cx,
        ),
        None => div().into_any_element(),
    }
}

fn more_row(
    inbox: &Entity<InboxModel>,
    loading: bool,
    load: fn(&mut InboxModel, &mut Context<InboxModel>),
    cx: &App,
) -> AnyElement {
    let target = inbox.downgrade();
    row::load_more(
        loading,
        move |_, _, cx| {
            _ = target.update(cx, load);
        },
        cx,
    )
    .into_any_element()
}

fn scope_item(
    pane: &WeakEntity<InboxPane>,
    target: IssueScope,
    current: IssueScope,
    icon: AppIconName,
    title: &'static str,
    hint: String,
) -> PopupMenuItem {
    let pane = pane.clone();
    PopupMenuItem::element(move |_, cx| {
        v_flex().child(title).child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(hint.clone()),
        )
    })
    .icon(Icon::new(icon))
    .checked(target == current)
    .on_click(move |_, window, cx| {
        _ = pane.update(cx, |this, cx| {
            this.model
                .update(cx, |model, cx| model.set_issue_scope(target, cx));
            this.focus_handle.focus(window, cx);
        });
    })
}

fn banner(cx: &App) -> Div {
    let theme = cx.theme();
    h_flex()
        .px_3()
        .py_1()
        .gap_2()
        .items_center()
        .text_xs()
        .bg(theme.danger.opacity(0.15))
        .text_color(theme.danger)
}

impl Focusable for InboxPane {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for InboxPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let model = self.model.read(cx);
        let error = model.error().cloned();
        let browse_error = model.browse_list().and_then(|list| list.error.clone());
        let error_banner = error.map(|error| banner(cx).child(error));
        let browse_banner = browse_error.map(|error| self.browse_banner(error, cx));
        let theme = cx.theme();
        v_flex()
            .key_context(INBOX_CONTEXT)
            .size_full()
            .bg(theme.background)
            .on_action(cx.listener(Self::on_clear_filter))
            .on_drag_move(cx.listener(Self::on_resize_drag))
            .child(self.header(cx))
            .children(error_banner)
            .children(browse_banner)
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

use std::ops::Range;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dock::{BasePanel, Panel, PanelControl, PanelEvent};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Div, Entity, EventEmitter, FocusHandle,
    Focusable, HighlightStyle, InteractiveElement as _, IntoElement, ListHorizontalSizingBehavior,
    ParentElement as _, Render, ScrollStrategy, SharedString, StatefulInteractiveElement as _,
    Styled as _, StyledText, Subscription, UniformListScrollHandle, Window, div, px, uniform_list,
};

use super::labels;
use super::log_lines::{LineKind, LogDoc};
use super::log_model::{JobLogModel, LogLoad, LogTarget};
use super::log_view::LogView;
use super::run_status::{run_status_icon, run_status_label};
use crate::placeholders::DockSlot;

gpui_kit::actions!(argon_log, [FocusLogSearch, NextLogMatch, PreviousLogMatch]);

pub const LOG_CONTEXT: &str = "JobLog";

const HEADER_HEIGHT: f32 = 34.;
const ROW_HEIGHT: f32 = 18.;
const GUTTER_WIDTH: f32 = 52.;
const GROUP_INDENT: f32 = 14.;
const SEARCH_WIDTH: f32 = 260.;

pub struct LogPanel {
    model: Entity<JobLogModel>,
    view: Option<LogView>,
    truncated: bool,
    search: Entity<InputState>,
    query: String,
    scroll: UniformListScrollHandle,
    focus_handle: FocusHandle,
    _subscriptions: [Subscription; 2],
}

impl LogPanel {
    pub fn new(model: Entity<JobLogModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder(labels::SEARCH_PLACEHOLDER));
        let subscriptions = [
            cx.subscribe_in(&search, window, Self::on_search_event),
            cx.observe(&model, |this, _, cx| this.sync_doc(cx)),
        ];
        let mut this = Self {
            model,
            view: None,
            truncated: false,
            search,
            query: String::new(),
            scroll: UniformListScrollHandle::new(),
            focus_handle: cx.focus_handle(),
            _subscriptions: subscriptions,
        };
        this.sync_doc(cx);
        this
    }

    fn sync_doc(&mut self, cx: &mut Context<Self>) {
        let (doc, truncated) = match self.model.read(cx).load() {
            LogLoad::Ready { doc, truncated } => (Some(doc.clone()), *truncated),
            LogLoad::Loading | LogLoad::Failed(_) => (None, false),
        };
        self.truncated = truncated;
        match doc {
            Some(doc) if !self.shows(&doc) => self.show_doc(doc),
            Some(_) => {}
            None => self.view = None,
        }
        cx.notify();
    }

    fn shows(&self, doc: &Rc<LogDoc>) -> bool {
        self.view
            .as_ref()
            .is_some_and(|view| Rc::ptr_eq(view.doc(), doc))
    }

    fn show_doc(&mut self, doc: Rc<LogDoc>) {
        let mut view = LogView::new(doc);
        view.search(&self.query);
        let target = view
            .first_error_row()
            .unwrap_or(view.row_count().saturating_sub(1));
        self.scroll.scroll_to_item(target, ScrollStrategy::Center);
        self.view = Some(view);
    }

    fn on_search_event(
        &mut self,
        search: &Entity<InputState>,
        event: &InputEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => {
                self.query = search.read(cx).value().to_string();
                if let Some(view) = self.view.as_mut() {
                    view.search(&self.query);
                }
                self.step_match(true, cx);
            }
            InputEvent::PressEnter { shift, .. } => self.step_match(!shift, cx),
            _ => {}
        }
    }

    fn step_match(&mut self, forward: bool, cx: &mut Context<Self>) {
        let top = self.scroll.0.borrow().base_handle.logical_scroll_top().0;
        if let Some(row) = self
            .view
            .as_mut()
            .and_then(|view| view.step_match(forward, top))
        {
            self.scroll.scroll_to_item(row, ScrollStrategy::Center);
        }
        cx.notify();
    }

    fn jump_to_error(&mut self, cx: &mut Context<Self>) {
        if let Some(row) = self.view.as_mut().and_then(LogView::first_error_row) {
            self.scroll.scroll_to_item(row, ScrollStrategy::Center);
        }
        cx.notify();
    }

    fn toggle_group(&mut self, line: usize, cx: &mut Context<Self>) {
        if let Some(view) = self.view.as_mut() {
            view.toggle_group(line);
        }
        cx.notify();
    }

    fn on_focus_search(&mut self, _: &FocusLogSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.search
            .update(cx, |search, cx| search.focus(window, cx));
    }

    fn on_next_match(&mut self, _: &NextLogMatch, _: &mut Window, cx: &mut Context<Self>) {
        self.step_match(true, cx);
    }

    fn on_previous_match(&mut self, _: &PreviousLogMatch, _: &mut Window, cx: &mut Context<Self>) {
        self.step_match(false, cx);
    }

    fn header(&self, target: &LogTarget, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let count = self.view.as_ref().map_or(0, LogView::match_count);
        let current = self.view.as_ref().and_then(LogView::current_match);
        let has_error = self
            .view
            .as_ref()
            .is_some_and(|view| view.doc().first_error.is_some());
        let counter = (!self.query.is_empty()).then(|| {
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(if count == 0 {
                    labels::NO_MATCHES.to_string()
                } else {
                    labels::match_position(current, count)
                })
        });
        let url = target.html_url.clone();
        h_flex()
            .flex_none()
            .h(px(HEADER_HEIGHT))
            .px_2()
            .gap_2()
            .items_center()
            .border_b_1()
            .border_color(theme.border)
            .child(run_status_icon(target.status, cx))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .font_semibold()
                    .text_color(theme.foreground)
                    .child(labels::title(&target.workflow, &target.job)),
            )
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(run_status_label(target.status)),
            )
            .child(div().flex_1())
            .child(
                div().w(px(SEARCH_WIDTH)).flex_none().child(
                    Input::new(&self.search)
                        .xsmall()
                        .prefix(Icon::new(IconName::Search).xsmall())
                        .suffix(div().children(counter)),
                ),
            )
            .child(
                Button::new("log-previous-match")
                    .icon(Icon::new(IconName::ArrowUp))
                    .xsmall()
                    .ghost()
                    .disabled(count == 0)
                    .tooltip(labels::PREVIOUS_MATCH)
                    .on_click(
                        cx.listener(|this, _: &ClickEvent, _, cx| this.step_match(false, cx)),
                    ),
            )
            .child(
                Button::new("log-next-match")
                    .icon(Icon::new(IconName::ArrowDown))
                    .xsmall()
                    .ghost()
                    .disabled(count == 0)
                    .tooltip(labels::NEXT_MATCH)
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.step_match(true, cx))),
            )
            .child(
                Button::new("log-jump-error")
                    .icon(Icon::new(IconName::CircleAlert))
                    .xsmall()
                    .ghost()
                    .disabled(!has_error)
                    .tooltip(labels::JUMP_TO_ERROR)
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.jump_to_error(cx))),
            )
            .child(
                Button::new("log-open-github")
                    .icon(Icon::new(IconName::ExternalLink))
                    .label(labels::OPEN_ON_GITHUB)
                    .xsmall()
                    .ghost()
                    .on_click(move |_, _, _| open_url(&url)),
            )
    }

    fn truncated_banner(&self, cx: &App) -> Option<Div> {
        let theme = cx.theme();
        self.truncated.then(|| {
            h_flex()
                .flex_none()
                .px_2()
                .py_0p5()
                .gap_1p5()
                .items_center()
                .bg(theme.warning.opacity(0.12))
                .text_xs()
                .text_color(theme.warning)
                .child(Icon::new(IconName::TriangleAlert).xsmall())
                .child(labels::TRUNCATED)
        })
    }

    fn body(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        match self.model.read(cx).load() {
            LogLoad::Loading => centered()
                .gap_2()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(Spinner::new().small())
                .child(labels::LOADING)
                .into_any_element(),
            LogLoad::Failed(message) => {
                let model = self.model.downgrade();
                centered()
                    .gap_2()
                    .text_sm()
                    .text_color(theme.foreground)
                    .child(
                        Icon::new(IconName::CircleAlert)
                            .small()
                            .text_color(theme.danger),
                    )
                    .child(message.clone())
                    .child(
                        Button::new("log-retry")
                            .icon(Icon::new(IconName::RotateCw))
                            .label(labels::RETRY)
                            .xsmall()
                            .outline()
                            .on_click(move |_, _, cx| {
                                _ = model.update(cx, |model, cx| model.retry(cx));
                            }),
                    )
                    .into_any_element()
            }
            LogLoad::Ready { .. } => self.lines(cx),
        }
    }

    fn lines(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(view) = self.view.as_ref().filter(|view| view.row_count() > 0) else {
            return centered()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(labels::EMPTY_LOG)
                .into_any_element();
        };
        let theme = cx.theme();
        let widest = view.widest_row();
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .font_family(theme.mono_font_family.clone())
            .text_xs()
            .child(
                uniform_list(
                    "job-log-lines",
                    view.row_count(),
                    cx.processor(|this, range: Range<usize>, _, cx| this.rows(range, cx)),
                )
                .track_scroll(&self.scroll)
                .with_horizontal_sizing_behavior(ListHorizontalSizingBehavior::Unconstrained)
                .with_width_from_item(widest)
                .size_full(),
            )
            .vertical_scrollbar(&self.scroll)
            .into_any_element()
    }

    fn rows(&self, range: Range<usize>, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(view) = self.view.as_ref() else {
            return Vec::new();
        };
        range
            .filter_map(|row| Some((row, view.line_at(row)?)))
            .map(|(row, line)| self.row(view, row, line, cx))
            .collect()
    }

    fn row(
        &self,
        view: &LogView,
        row: usize,
        line_ix: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let line = &view.doc().lines[line_ix];
        let current = view.current_match_ref();
        let highlights: Vec<(Range<usize>, HighlightStyle)> = view
            .matches_in(line_ix)
            .iter()
            .map(|found| {
                let strong = current.is_some_and(|current| current == found);
                let background = if strong {
                    theme.warning.opacity(0.7)
                } else {
                    theme.warning.opacity(0.28)
                };
                (
                    found.range.clone(),
                    HighlightStyle {
                        background_color: Some(background),
                        ..HighlightStyle::default()
                    },
                )
            })
            .collect();
        let color = match line.kind {
            LineKind::Error => theme.danger,
            LineKind::Warning => theme.warning,
            LineKind::Notice | LineKind::Command => theme.info,
            LineKind::Debug => theme.muted_foreground,
            LineKind::GroupStart | LineKind::GroupEnd | LineKind::Plain => theme.foreground,
        };
        let text =
            StyledText::new(SharedString::from(line.text.clone())).with_highlights(highlights);
        let indent = if line.group.is_some() {
            GROUP_INDENT
        } else {
            0.
        };
        let gutter = div()
            .flex_none()
            .w(px(GUTTER_WIDTH))
            .pr_2()
            .text_right()
            .text_color(theme.muted_foreground.opacity(0.7))
            .child((line_ix + 1).to_string());
        let base = h_flex()
            .id(("log-row", row))
            .h(px(ROW_HEIGHT))
            .min_w_full()
            .items_center()
            .whitespace_nowrap()
            .child(gutter);
        let base = match line.kind {
            LineKind::Error => base.bg(theme.danger.opacity(0.1)),
            LineKind::Warning => base.bg(theme.warning.opacity(0.08)),
            _ => base,
        };
        if line.kind == LineKind::GroupStart {
            let chevron = if view.is_collapsed(line_ix) {
                IconName::ChevronRight
            } else {
                IconName::ChevronDown
            };
            return base
                .cursor_pointer()
                .hover(|style| style.bg(theme.muted.opacity(0.5)))
                .child(
                    Icon::new(chevron)
                        .xsmall()
                        .text_color(theme.muted_foreground),
                )
                .child(div().pl_1().font_semibold().text_color(color).child(text))
                .on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| this.toggle_group(line_ix, cx)),
                )
                .into_any_element();
        }
        base.child(div().pl(px(indent)).text_color(color).child(text))
            .into_any_element()
    }

    fn empty(&self, cx: &App) -> AnyElement {
        centered()
            .gap_2()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(Icon::new(IconName::SquareTerminal).large())
            .child(labels::EMPTY_HINT)
            .into_any_element()
    }
}

fn centered() -> Div {
    v_flex()
        .flex_1()
        .size_full()
        .items_center()
        .justify_center()
}

fn open_url(url: &str) {
    if let Err(error) = open::that_detached(url) {
        tracing::warn!(%error, url, "failed to open job in browser");
    }
}

impl EventEmitter<PanelEvent> for LogPanel {}

impl Focusable for LogPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BasePanel for LogPanel {
    fn panel_name(&self) -> &'static str {
        DockSlot::Log.panel_name()
    }

    fn closable(&self, _: &App) -> bool {
        false
    }

    fn zoomable(&self, _: &App) -> bool {
        false
    }
}

impl Panel for LogPanel {
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        labels::PANEL_TITLE
    }

    fn zoom_control(&self, _: &App) -> Option<PanelControl> {
        None
    }
}

impl Render for LogPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let target = self.model.read(cx).target().cloned();
        let content = match target {
            Some(target) => v_flex()
                .size_full()
                .child(self.header(&target, cx))
                .children(self.truncated_banner(cx))
                .child(self.body(cx))
                .into_any_element(),
            None => self.empty(cx),
        };
        v_flex()
            .key_context(LOG_CONTEXT)
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(cx.theme().background)
            .on_action(cx.listener(Self::on_focus_search))
            .on_action(cx.listener(Self::on_next_match))
            .on_action(cx.listener(Self::on_previous_match))
            .child(content)
    }
}

mod header;
mod row;

use std::sync::Arc;
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::Button;
use gpui_kit::component::dock::{BasePanel, Panel, PanelControl, PanelEvent};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, v_flex};
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Div, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, ScrollStrategy, Styled as _,
    Subscription, Task, UniformListScrollHandle, Window,
};

use super::labels;
use super::log_lines::{LogDoc, SearchHits, search_lines};
use super::log_model::{JobLogModel, LogLoad};
use super::log_view::LogView;
use crate::placeholders::DockSlot;

gpui_kit::actions!(argon_log, [FocusLogSearch, NextLogMatch, PreviousLogMatch]);

pub const LOG_CONTEXT: &str = "JobLog";

const SEARCH_DEBOUNCE: Duration = Duration::from_millis(150);
const MIN_QUERY_CHARS: usize = 2;
const MATCH_LIMIT: usize = 10_000;

pub struct LogPanel {
    model: Entity<JobLogModel>,
    view: Option<LogView>,
    truncated: bool,
    search: Entity<InputState>,
    query: String,
    search_epoch: u64,
    search_task: Option<Task<()>>,
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
            search_epoch: 0,
            search_task: None,
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
            Some(doc) if !self.shows(&doc) => self.show_doc(doc, cx),
            Some(_) => {}
            None => self.view = None,
        }
        cx.notify();
    }

    fn shows(&self, doc: &Arc<LogDoc>) -> bool {
        self.view
            .as_ref()
            .is_some_and(|view| Arc::ptr_eq(view.doc(), doc))
    }

    fn show_doc(&mut self, doc: Arc<LogDoc>, cx: &mut Context<Self>) {
        let mut view = LogView::new(doc);
        let target = view
            .first_error_row()
            .unwrap_or(view.row_count().saturating_sub(1));
        self.scroll.scroll_to_item(target, ScrollStrategy::Center);
        self.view = Some(view);
        self.schedule_search(false, cx);
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
                let query = search.read(cx).value().to_string();
                if query != self.query {
                    self.query = query;
                    self.schedule_search(true, cx);
                }
            }
            InputEvent::PressEnter { shift, .. } => self.step_match(!shift, cx),
            _ => {}
        }
    }

    fn schedule_search(&mut self, jump: bool, cx: &mut Context<Self>) {
        self.search_epoch += 1;
        let epoch = self.search_epoch;
        let Some(doc) = self.view.as_ref().map(|view| view.doc().clone()) else {
            self.search_task = None;
            return;
        };
        let query = self.query.clone();
        self.search_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SEARCH_DEBOUNCE).await;
            let hits = if query.chars().count() < MIN_QUERY_CHARS {
                SearchHits::default()
            } else {
                let needle = query.clone();
                cx.background_spawn(async move { search_lines(&doc, &needle, MATCH_LIMIT) })
                    .await
            };
            _ = this.update(cx, |this, cx| {
                if epoch == this.search_epoch {
                    this.apply_hits(query, hits, jump, cx);
                }
            });
        }));
    }

    fn apply_hits(&mut self, query: String, hits: SearchHits, jump: bool, cx: &mut Context<Self>) {
        let Some(view) = self.view.as_mut() else {
            return;
        };
        let query = if hits.lines.is_empty() && query.chars().count() < MIN_QUERY_CHARS {
            String::new()
        } else {
            query
        };
        view.set_hits(query, hits);
        if jump {
            self.step_match(true, cx);
        }
        cx.notify();
    }

    pub(super) fn step_match(&mut self, forward: bool, cx: &mut Context<Self>) {
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

    pub(super) fn jump_to_error(&mut self, cx: &mut Context<Self>) {
        if let Some(row) = self.view.as_mut().and_then(LogView::first_error_row) {
            self.scroll.scroll_to_item(row, ScrollStrategy::Center);
        }
        cx.notify();
    }

    pub(super) fn toggle_group(&mut self, line: usize, cx: &mut Context<Self>) {
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

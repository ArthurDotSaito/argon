mod items;
mod plan;
mod properties;
mod tools;

use std::collections::HashSet;
use std::ops::Range;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{
    ActiveTheme as _, Icon, Selectable as _, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::{
    App, AppContext as _, Canvas, Context, Div, Entity, FocusHandle, FollowMode, IntoElement,
    ListAlignment, ListState, ParentElement as _, Render, SharedString, Styled as _, Subscription,
    Window, canvas, div, list, px,
};

use super::badges::status_chip;
use super::composer::Composer;
use super::panel_labels as labels;
use super::session_actions::{SessionControls, resume_error, row_actions};
use super::sessions_list::subject_labels;
use super::thread_model::ThreadModel;
use crate::theme::density;

const NARROW_WIDTH: f32 = 640.;
const TITLE_ERROR_MAX_WIDTH: f32 = 160.;
const LIST_OVERDRAW: f32 = 600.;
const TITLE_BAR_HEIGHT: f32 = 44.;
const TITLE_BAR_GAP: f32 = 12.;
const CONTENT_PADDING_X: f32 = 16.;
const TRANSCRIPT_PADDING_Y: f32 = 14.;
const TRANSCRIPT_MAX_WIDTH: f32 = 780.;
const ITEM_GAP: f32 = 12.;
const RADIUS: f32 = 4.;

pub struct ThreadView {
    model: Entity<ThreadModel>,
    composer: Option<Entity<Composer>>,
    expanded: HashSet<SharedString>,
    plan_editor: Option<plan::PlanEditor>,
    list: ListState,
    rows: Vec<Range<usize>>,
    narrow: bool,
    panel_override: Option<bool>,
    _subscriptions: Vec<Subscription>,
}

impl ThreadView {
    pub fn new(model: Entity<ThreadModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let composer = model
            .read(cx)
            .is_live()
            .then(|| cx.new(|cx| Composer::new(model.clone(), window, cx)));
        let subscriptions = vec![cx.observe(&model, |this, _, cx| this.sync_rows(cx))];
        let list = ListState::new(0, ListAlignment::Top, px(LIST_OVERDRAW));
        list.set_follow_mode(FollowMode::Tail);
        let mut this = Self {
            model,
            composer,
            expanded: HashSet::new(),
            plan_editor: None,
            list,
            rows: Vec::new(),
            narrow: false,
            panel_override: None,
            _subscriptions: subscriptions,
        };
        this.sync_rows(cx);
        this
    }

    fn sync_rows(&mut self, cx: &mut Context<Self>) {
        let rows = items::group_rows(self.model.read(cx).transcript().items());
        let count = rows.len() + 1;
        let shown = self.list.item_count();
        if count > shown {
            self.list.splice(shown..shown, count - shown);
        } else if count < shown {
            self.list.reset(count);
        }
        self.rows = rows;
        self.list.remeasure_items(0..count);
        cx.notify();
    }

    pub fn composer_focus_handle(&self, cx: &App) -> Option<FocusHandle> {
        self.composer
            .as_ref()
            .map(|composer| composer.read(cx).input_focus_handle(cx))
    }

    pub(super) fn is_expanded(&self, key: &str) -> bool {
        self.expanded.contains(key)
    }

    pub(super) fn toggle(&mut self, key: SharedString, row: usize, cx: &mut Context<Self>) {
        self.list.pause_following_tail();
        if !self.expanded.remove(&key) {
            self.expanded.insert(key);
        }
        self.list.remeasure_items(row..row + 1);
        cx.notify();
    }

    fn live_composer(&self, cx: &App) -> Option<&Entity<Composer>> {
        self.composer
            .as_ref()
            .filter(|_| self.model.read(cx).is_live())
    }

    fn panel_visible(&self) -> bool {
        self.panel_override.unwrap_or(!self.narrow)
    }

    fn set_narrow(&mut self, narrow: bool, cx: &mut Context<Self>) {
        if self.narrow == narrow {
            return;
        }
        self.narrow = narrow;
        self.panel_override = None;
        self.sync_composer(cx);
        cx.notify();
    }

    fn toggle_panel(&mut self, cx: &mut Context<Self>) {
        self.panel_override = Some(!self.panel_visible());
        self.sync_composer(cx);
        cx.notify();
    }

    fn sync_composer(&self, cx: &mut Context<Self>) {
        let inline = !self.panel_visible();
        if let Some(composer) = &self.composer {
            composer.update(cx, |composer, cx| composer.set_inline_selects(inline, cx));
        }
    }

    fn width_probe(&self, cx: &Context<Self>) -> Canvas<()> {
        let view = cx.weak_entity();
        let narrow = self.narrow;
        canvas(
            move |bounds, window, cx| {
                let measured = bounds.size.width < px(NARROW_WIDTH);
                if measured != narrow {
                    window.defer(cx, move |_, cx| {
                        let _ = view.update(cx, |this, cx| this.set_narrow(measured, cx));
                    });
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full()
    }

    fn session_controls(&self, cx: &App) -> SessionControls {
        let model = self.model.read(cx);
        let info = model.info();
        SessionControls {
            id: info.session_id.clone(),
            status: model.card_status(),
            resume: model.resume_action(),
            worktree: info.worktree.clone(),
        }
    }

    fn title_bar(&self, title: SharedString, panel_visible: bool, cx: &mut Context<Self>) -> Div {
        let toggle = Button::new("agent-thread-toggle-panel")
            .icon(Icon::new(IconName::PanelRight))
            .xsmall()
            .ghost()
            .selected(panel_visible)
            .tooltip(labels::SESSION_DETAILS)
            .on_click(cx.listener(|this, _, _, cx| this.toggle_panel(cx)));
        let controls = (!panel_visible).then(|| {
            let controls = self.session_controls(cx);
            let error = resume_error(&controls, cx).map(|error| {
                div()
                    .min_w_0()
                    .max_w(px(TITLE_ERROR_MAX_WIDTH))
                    .truncate()
                    .text_xs()
                    .text_color(cx.theme().danger)
                    .child(error)
            });
            h_flex()
                .flex_none()
                .gap_1p5()
                .items_center()
                .children(error)
                .child(row_actions(&controls, "thread-bar", cx))
        });
        let status = self.model.read(cx).card_status();
        h_flex()
            .flex_none()
            .h(px(density::scaled(TITLE_BAR_HEIGHT, cx)))
            .px(px(CONTENT_PADDING_X))
            .gap(px(TITLE_BAR_GAP))
            .items_center()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(status_chip(status, cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_semibold()
                    .child(title),
            )
            .children(controls)
            .child(toggle)
    }

    fn footer(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let error = self.model.read(cx).action_error().cloned().map(|error| {
            h_flex()
                .px(px(CONTENT_PADDING_X))
                .py_1()
                .gap_2()
                .items_center()
                .text_xs()
                .text_color(theme.danger)
                .child(div().flex_1().min_w_0().child(error))
                .child(
                    Button::new("agent-thread-dismiss-error")
                        .icon(Icon::new(IconName::X))
                        .xsmall()
                        .ghost()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.model.update(cx, |model, cx| model.dismiss_error(cx))
                        })),
                )
        });
        let body = match self.live_composer(cx) {
            Some(composer) => composer.clone().into_any_element(),
            None => div()
                .px(px(CONTENT_PADDING_X))
                .py_2p5()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(labels::SESSION_ENDED)
                .into_any_element(),
        };
        v_flex()
            .flex_none()
            .border_t_1()
            .border_color(theme.border)
            .children(error)
            .child(body)
    }
}

impl Render for ThreadView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let info = self.model.read(cx).info();
        let (title, reference) = subject_labels(&info.pr, info.kind, info.number, cx);
        let panel_visible = self.panel_visible();
        let title_bar = self.title_bar(title, panel_visible, cx);
        let properties = panel_visible.then(|| self.properties(reference, cx));
        let footer = self.footer(cx);
        let probe = self.width_probe(cx);
        let view = cx.weak_entity();
        let rows = list(self.list.clone(), move |ix, _, cx| {
            view.update(cx, |this, cx| this.render_row(ix, cx))
                .unwrap_or_else(|_| div().into_any_element())
        })
        .size_full();
        h_flex()
            .relative()
            .size_full()
            .child(probe)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(title_bar)
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .pt(px(TRANSCRIPT_PADDING_Y))
                            .child(rows),
                    )
                    .child(footer),
            )
            .children(properties)
    }
}

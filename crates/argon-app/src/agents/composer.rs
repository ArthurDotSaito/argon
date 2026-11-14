use argon_core::{PermissionMode, SessionKind};
use gpui_kit::assets::IconName;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::{App, FocusHandle, Focusable as _};
use gpui_kit::{
    AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _,
    Subscription, Window, div,
};

use super::choice::{Choice, permission_choices};
use super::labels::{FULL_ACCESS_RISK, PLAN_MODE_LOCKED, permission_mode_hint};
use super::panel_labels as labels;
use super::thread_model::ThreadModel;
use crate::review::{Confirm, confirm};
use crate::workspace::Workspace;

const MIN_ROWS: usize = 2;
const MAX_ROWS: usize = 8;

pub struct Composer {
    model: Entity<ThreadModel>,
    input: Entity<TextareaState>,
    model_select: Entity<SelectState<Vec<Choice>>>,
    mode_select: Entity<SelectState<Vec<Choice>>>,
    shown: (Vec<String>, String),
    shown_mode: Option<PermissionMode>,
    was_changing: bool,
    was_changing_mode: bool,
    _subscriptions: Vec<Subscription>,
}

impl Composer {
    pub fn input_focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.focus_handle(cx)
    }

    pub fn new(model: Entity<ThreadModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder(labels::COMPOSER_PLACEHOLDER)
                .auto_grow(MIN_ROWS, MAX_ROWS)
        });
        let model_select = cx.new(|cx| SelectState::new(Vec::<Choice>::new(), None, window, cx));
        let mode_select = cx.new(|cx| SelectState::new(permission_choices(), None, window, cx));
        let subscriptions = vec![
            cx.observe(&input, |_, _, cx| cx.notify()),
            cx.subscribe_in(&input, window, |this, _, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter {
                    secondary: true, ..
                } = event
                {
                    this.send(window, cx);
                }
            }),
            cx.subscribe_in(
                &model_select,
                window,
                |this, _, event: &SelectEvent<Vec<Choice>>, _, cx| {
                    if let SelectEvent::Confirm(Some(id)) = event {
                        let id = id.clone();
                        this.model
                            .update(cx, |model, cx| model.change_model(id, cx));
                    }
                },
            ),
            cx.subscribe_in(
                &mode_select,
                window,
                |this, _, event: &SelectEvent<Vec<Choice>>, window, cx| {
                    if let SelectEvent::Confirm(Some(id)) = event
                        && let Some(mode) = PermissionMode::parse(id)
                    {
                        this.request_mode(mode, window, cx);
                    }
                },
            ),
            cx.observe_in(&model, window, |this, _, window, cx| {
                this.sync_models(window, cx);
                this.sync_mode(false, window, cx);
            }),
        ];
        let mut this = Self {
            model,
            input,
            model_select,
            mode_select,
            shown: (Vec::new(), String::new()),
            shown_mode: None,
            was_changing: false,
            was_changing_mode: false,
            _subscriptions: subscriptions,
        };
        this.sync_models(window, cx);
        this.sync_mode(true, window, cx);
        this
    }

    fn sync_mode(&mut self, force: bool, window: &mut Window, cx: &mut Context<Self>) {
        let model = self.model.read(cx);
        let current = model.permission_mode();
        let changing = model.is_changing_permission_mode();
        let settled = std::mem::replace(&mut self.was_changing_mode, changing) && !changing;
        if !force && !settled && self.shown_mode == current {
            return;
        }
        self.shown_mode = current;
        if let Some(mode) = current {
            self.mode_select.update(cx, |select, cx| {
                select.set_selected_value(&mode.as_str().to_owned(), window, cx)
            });
        }
    }

    fn request_mode(&mut self, mode: PermissionMode, window: &mut Window, cx: &mut Context<Self>) {
        let model = self.model.clone();
        let widening_to_full =
            mode == PermissionMode::FullAccess && model.read(cx).permission_mode() != Some(mode);
        if widening_to_full {
            let request = Confirm {
                title: labels::FULL_ACCESS_TITLE,
                message: FULL_ACCESS_RISK.to_owned(),
                confirm: labels::FULL_ACCESS_CONFIRM,
            };
            confirm(request, window, cx, move |_, cx| {
                model.update(cx, |model, cx| model.change_permission_mode(mode, cx));
            });
        } else {
            model.update(cx, |model, cx| model.change_permission_mode(mode, cx));
        }
        self.sync_mode(true, window, cx);
    }

    fn sync_models(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let model = self.model.read(cx);
        let ids: Vec<String> = model.models().iter().map(|info| info.id.clone()).collect();
        let current = model.info().model.clone();
        let changing = model.is_changing_model();
        let settled = std::mem::replace(&mut self.was_changing, changing) && !changing;
        if !settled && self.shown.0 == ids && self.shown.1 == current {
            return;
        }
        let mut choices: Vec<Choice> = model
            .models()
            .iter()
            .map(|info| Choice::new(info.id.clone(), info.label.clone()))
            .collect();
        if !current.is_empty() && !ids.contains(&current) {
            choices.insert(
                0,
                Choice::new(current.clone(), model.info().model_label.clone()),
            );
        }
        self.model_select.update(cx, |select, cx| {
            select.set_items(choices, window, cx);
            if !current.is_empty() {
                select.set_selected_value(&current, window, cx);
            }
        });
        self.shown = (ids, current);
    }

    fn text(&self, cx: &Context<Self>) -> String {
        self.input.read(cx).value().trim().to_owned()
    }

    fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.text(cx);
        if text.is_empty() || self.model.read(cx).transcript().in_turn() {
            return;
        }
        let sent = self.model.update(cx, |model, cx| model.send(text, cx));
        if sent {
            self.input
                .update(cx, |input, cx| input.set_value("", window, cx));
        }
    }

    fn insert_reference(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pr = self.model.read(cx).info().pr.clone();
        let line = Workspace::global(cx)
            .and_then(|workspace| workspace.upgrade())
            .and_then(|workspace| workspace.read(cx).selected_diff_line(&pr, cx));
        let Some((path, line)) = line else {
            window.push_notification(Notification::info(labels::NO_LINE_SELECTED), cx);
            return;
        };
        self.input.update(cx, |input, cx| {
            input.insert(labels::reference(&path, line), window, cx);
            input.focus(window, cx);
        });
    }
}

impl Render for Composer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let model = self.model.read(cx);
        let in_turn = model.transcript().in_turn();
        let locked = in_turn || model.is_changing_model() || model.models().is_empty();
        let placeholder = model.info().model_label.clone();
        let empty = self.text(cx).is_empty();
        let theme = cx.theme();
        let select = Select::new(&self.model_select)
            .xsmall()
            .placeholder(placeholder)
            .disabled(locked);
        let mode = model.permission_mode();
        let mode_select = Select::new(&self.mode_select)
            .xsmall()
            .placeholder(labels::PERMISSION_MODE_TOOLTIP)
            .disabled(
                mode.is_none()
                    || model.is_changing_permission_mode()
                    || model.is_permission_mode_locked(),
            );
        let mode_hint = if model.is_permission_mode_locked() {
            Some(PLAN_MODE_LOCKED)
        } else {
            mode.map(permission_mode_hint)
        };
        let reference = (model.info().kind != SessionKind::Plan).then(|| {
            Button::new("composer-reference")
                .icon(Icon::new(IconName::AtSign))
                .xsmall()
                .ghost()
                .tooltip(labels::REFERENCE)
                .on_click(cx.listener(|this, _, window, cx| this.insert_reference(window, cx)))
        });
        let primary = if in_turn {
            Button::new("composer-stop")
                .icon(Icon::new(IconName::Square))
                .label(labels::STOP)
                .xsmall()
                .danger()
                .on_click(
                    cx.listener(|this, _, _, cx| this.model.update(cx, |model, cx| model.stop(cx))),
                )
        } else {
            Button::new("composer-send")
                .icon(Icon::new(IconName::ArrowUp))
                .label(labels::SEND)
                .xsmall()
                .primary()
                .disabled(empty)
                .tooltip("Ctrl+Enter")
                .on_click(cx.listener(|this, _, window, cx| this.send(window, cx)))
        };
        v_flex()
            .p_2()
            .gap_1p5()
            .child(Textarea::new(&self.input))
            .child(
                h_flex()
                    .gap_1p5()
                    .items_center()
                    .child(div().w_40().child(select))
                    .children(reference)
                    .child(div().flex_1())
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(Icon::new(IconName::Lock).xsmall())
                            .child(labels::DRAFTS_ONLY),
                    )
                    .child(primary),
            )
            .child(
                h_flex()
                    .gap_1p5()
                    .items_center()
                    .child(div().w_40().flex_none().child(mode_select))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .line_clamp(2)
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .children(mode_hint),
                    ),
            )
            .children(in_turn.then(|| {
                div()
                    .text_xs()
                    .text_color(theme.warning)
                    .child(labels::PERMISSION_MODE_TURN_HINT)
            }))
    }
}

use argon_core::ReviewEvent;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Textarea;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Selectable as _, Sizable as _, StyledExt as _,
    h_flex, v_flex,
};
use gpui_kit::{
    ClickEvent, Context, Div, Focusable as _, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, Stateful, StatefulInteractiveElement as _, Styled as _, Window,
    div, prelude::FluentBuilder as _, px,
};

use super::{ComposerEvent, ComposerTab, EVENTS, ReviewComposer};
use crate::actions::CollapseReview;
use crate::keymap::REVIEW_COMPOSER_CONTEXT;
use crate::pr::markdown;
use crate::review::drafts::{DraftSummary, SubmitState, summarize};
use crate::review::labels;

const SIDE_WIDTH: f32 = 280.;
const HEADER_HEIGHT: f32 = 40.;
const SELECTED_FILL: f32 = 0.12;

fn event_color(event: ReviewEvent, cx: &gpui_kit::App) -> Hsla {
    let theme = cx.theme();
    match event {
        ReviewEvent::Comment => theme.primary,
        ReviewEvent::Approve => theme.success,
        ReviewEvent::RequestChanges => theme.danger,
    }
}

fn event_icon(event: ReviewEvent) -> IconName {
    match event {
        ReviewEvent::Comment => IconName::MessageSquare,
        ReviewEvent::Approve => IconName::CircleCheck,
        ReviewEvent::RequestChanges => IconName::CircleX,
    }
}

fn event_label(event: ReviewEvent) -> &'static str {
    match event {
        ReviewEvent::Comment => labels::EVENT_COMMENT,
        ReviewEvent::Approve => labels::EVENT_APPROVE,
        ReviewEvent::RequestChanges => labels::EVENT_REQUEST_CHANGES,
    }
}

fn event_hint(event: ReviewEvent) -> &'static str {
    match event {
        ReviewEvent::Comment => labels::EVENT_COMMENT_HINT,
        ReviewEvent::Approve => labels::EVENT_APPROVE_HINT,
        ReviewEvent::RequestChanges => labels::EVENT_REQUEST_CHANGES_HINT,
    }
}

impl ReviewComposer {
    fn header(&self, count: usize, sending: bool, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        h_flex()
            .w_full()
            .flex_none()
            .h(px(HEADER_HEIGHT))
            .px_4()
            .gap_2()
            .items_center()
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .font_semibold()
                    .text_color(theme.foreground)
                    .child(labels::YOUR_REVIEW),
            )
            .when(count > 0, |header| {
                header
                    .child(
                        div()
                            .text_color(theme.muted_foreground)
                            .child(labels::SEPARATOR),
                    )
                    .child(
                        div()
                            .text_color(theme.muted_foreground)
                            .child(labels::line_comments(count)),
                    )
            })
            .child(div().flex_1())
            .child(
                Button::new("composer-discard")
                    .icon(Icon::new(IconName::Trash))
                    .label(labels::DISCARD)
                    .small()
                    .ghost()
                    .disabled(sending)
                    .on_click(
                        cx.listener(|this, _: &ClickEvent, window, cx| this.discard(window, cx)),
                    ),
            )
            .child(
                Button::new("composer-collapse")
                    .icon(Icon::new(IconName::ChevronDown))
                    .label(labels::COLLAPSE)
                    .small()
                    .ghost()
                    .on_click(
                        cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(ComposerEvent::Collapse)),
                    ),
            )
    }

    fn head_moved_warning(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        h_flex()
            .w_full()
            .flex_none()
            .px_4()
            .py_1p5()
            .gap_2()
            .items_center()
            .bg(theme.warning.opacity(0.08))
            .border_b_1()
            .border_color(theme.warning.opacity(0.4))
            .text_xs()
            .text_color(theme.foreground)
            .child(
                Icon::new(IconName::TriangleAlert)
                    .xsmall()
                    .text_color(theme.warning),
            )
            .child(labels::HEAD_MOVED)
    }

    fn tabs(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let tab = |id: &'static str, label: &'static str, target: ComposerTab| {
            Button::new(id)
                .label(label)
                .xsmall()
                .ghost()
                .selected(self.tab == target)
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.set_tab(target, cx)))
        };
        h_flex()
            .w_full()
            .flex_none()
            .px_2()
            .py_1()
            .gap_1()
            .items_center()
            .border_b_1()
            .border_color(theme.border)
            .child(tab("composer-write", labels::WRITE, ComposerTab::Write))
            .child(tab(
                "composer-preview",
                labels::PREVIEW,
                ComposerTab::Preview,
            ))
    }

    fn editor(&self, sending: bool, cx: &mut Context<Self>) -> Div {
        let body = match self.tab {
            ComposerTab::Write => div()
                .size_full()
                .child(Textarea::new(&self.input).h_full().disabled(sending)),
            ComposerTab::Preview => {
                let text = self.body(cx);
                let theme = cx.theme();
                let frame = div()
                    .size_full()
                    .rounded(theme.radius)
                    .border_1()
                    .border_color(theme.input)
                    .bg(theme.background);
                if text.trim().is_empty() {
                    frame
                        .px_3()
                        .py_2()
                        .text_color(theme.muted_foreground)
                        .child(labels::PREVIEW_EMPTY)
                } else {
                    frame.child(
                        v_flex()
                            .id("composer-preview-body")
                            .size_full()
                            .px_3()
                            .py_2()
                            .overflow_y_scroll()
                            .child(markdown("composer-preview-text".into(), text.into())),
                    )
                }
            }
        };
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(self.tabs(cx))
            .child(div().flex_1().min_h_0().p_2().child(body))
    }

    fn verdict(
        &self,
        ix: usize,
        event: ReviewEvent,
        sending: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let allowed = self.options.allows(event);
        let selected = self.event == event;
        let color = event_color(event, cx);
        let theme = cx.theme();
        let (border, fill, name_color) = if selected {
            (color, color.opacity(SELECTED_FILL), color)
        } else {
            (theme.border, theme.transparent, theme.foreground)
        };
        v_flex()
            .id(("composer-verdict", ix))
            .w_full()
            .px_3()
            .py_2()
            .gap_1()
            .rounded(theme.radius)
            .border_1()
            .border_color(border)
            .bg(fill)
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .text_color(name_color)
                    .child(Icon::new(event_icon(event)).small())
                    .child(div().font_semibold().child(event_label(event))),
            )
            .when(selected, |block| {
                block.child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(event_hint(event)),
                )
            })
            .when(!allowed || sending, |block| block.opacity(0.5))
            .when(!allowed, |block| {
                block.tooltip(|window, cx| Tooltip::new(labels::OWN_PR).build(window, cx))
            })
            .when(allowed && !sending, |block| {
                block.cursor_pointer().on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| this.set_event(event, cx)),
                )
            })
    }

    fn verdicts(&self, sending: bool, cx: &mut Context<Self>) -> Div {
        let blocks = EVENTS
            .iter()
            .enumerate()
            .map(|(ix, event)| self.verdict(ix, *event, sending, cx))
            .collect::<Vec<_>>();
        let theme = cx.theme();
        let own_pr = (!self.options.judge).then(|| {
            h_flex()
                .gap_1p5()
                .items_start()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(Icon::new(IconName::Info).xsmall().mt_0p5())
                .child(div().flex_1().min_w_0().child(labels::OWN_PR))
        });
        v_flex().gap_2().children(blocks).children(own_pr)
    }

    fn comment_rows(
        &self,
        items: Vec<DraftSummary>,
        suspects: &[i64],
        sending: bool,
        cx: &mut Context<Self>,
    ) -> Vec<Stateful<Div>> {
        let theme = cx.theme();
        let (link, danger, muted) = (theme.link, theme.danger, theme.muted_foreground);
        let font = theme.mono_font_family.clone();
        items
            .into_iter()
            .enumerate()
            .map(|(ix, item)| {
                let suspect = suspects.contains(&item.id);
                let (path, line, side) = (item.path, item.line, item.side);
                let label = if suspect {
                    format!("{} — {}", item.label, labels::SUSPECT_DRAFT)
                } else {
                    item.label
                };
                let color = if suspect { danger } else { link };
                v_flex()
                    .id(("composer-draft", ix))
                    .w_full()
                    .min_w_0()
                    .gap_0p5()
                    .px_1()
                    .py_1()
                    .when(!sending, |row| {
                        row.cursor_pointer().on_click(cx.listener(
                            move |_, _: &ClickEvent, _, cx| {
                                cx.emit(ComposerEvent::Reveal {
                                    path: path.clone(),
                                    line,
                                    side,
                                })
                            },
                        ))
                    })
                    .child(
                        h_flex()
                            .gap_1p5()
                            .items_center()
                            .text_xs()
                            .text_color(color)
                            .when(suspect, |head| {
                                head.child(Icon::new(IconName::TriangleAlert).xsmall())
                            })
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .font_family(font.clone())
                                    .hover(|style| style.underline())
                                    .child(label),
                            ),
                    )
                    .when(!item.excerpt.is_empty(), |row| {
                        row.child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_xs()
                                .text_color(muted)
                                .child(item.excerpt),
                        )
                    })
            })
            .collect()
    }

    fn comments(
        &self,
        items: Vec<DraftSummary>,
        suspects: &[i64],
        sending: bool,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        if items.is_empty() {
            return None;
        }
        let count = items.len();
        let open = self.comments_open;
        let rows = open.then(|| self.comment_rows(items, suspects, sending, cx));
        let theme = cx.theme();
        let chevron = if open {
            IconName::ChevronDown
        } else {
            IconName::ChevronRight
        };
        let toggle = h_flex()
            .id("composer-comments-toggle")
            .gap_1p5()
            .items_center()
            .cursor_pointer()
            .text_xs()
            .font_semibold()
            .text_color(theme.foreground)
            .child(Icon::new(chevron).xsmall())
            .child(labels::line_comments(count))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.comments_open = !this.comments_open;
                cx.notify();
            }));
        Some(
            v_flex()
                .gap_1()
                .child(toggle)
                .children(rows.into_iter().flatten()),
        )
    }

    fn stale_warning(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        h_flex()
            .gap_2()
            .items_center()
            .px_2()
            .py_1p5()
            .rounded(theme.radius)
            .bg(theme.warning.opacity(0.1))
            .text_xs()
            .text_color(theme.foreground)
            .child(
                Icon::new(IconName::TriangleAlert)
                    .small()
                    .text_color(theme.warning),
            )
            .child(div().flex_1().min_w_0().child(labels::DIFF_STALE))
            .child(
                Button::new("composer-reload")
                    .label(labels::RELOAD)
                    .xsmall()
                    .outline()
                    .on_click(
                        cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(ComposerEvent::Reload)),
                    ),
            )
    }

    fn submit_button(&self, sending: bool, stale: bool, cx: &mut Context<Self>) -> Button {
        let button = Button::new("composer-submit")
            .w_full()
            .icon(Icon::new(event_icon(self.event)))
            .label(event_label(self.event))
            .small()
            .loading(sending)
            .disabled(sending || stale)
            .tooltip(labels::SUBMIT_SHORTCUT)
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.submit(window, cx)));
        match self.event {
            ReviewEvent::Comment => button.primary(),
            ReviewEvent::Approve => button.success(),
            ReviewEvent::RequestChanges => button.danger(),
        }
    }
}

impl Render for ReviewComposer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.focus_pending) {
            let handle = match self.tab {
                ComposerTab::Write => self.input.focus_handle(cx),
                ComposerTab::Preview => self.focus_handle.clone(),
            };
            window.defer(cx, move |window, cx| window.focus(&handle, cx));
        }
        let drafts = self.drafts.read(cx);
        let state = drafts.submit_state().clone();
        let (stale, head_moved) = (drafts.stale(), drafts.head_moved());
        let suspects = drafts.suspects().to_vec();
        let mut items = summarize(drafts.drafts());
        items.sort_by_key(|item| !suspects.contains(&item.id));
        let count = items.len();
        let sending = state == SubmitState::Sending;

        let header = self.header(count, sending, cx);
        let warning = head_moved.then(|| self.head_moved_warning(cx));
        let editor = self.editor(sending, cx);
        let verdicts = self.verdicts(sending, cx);
        let comments = self.comments(items, &suspects, sending, cx);
        let stale_warning = stale.then(|| self.stale_warning(cx));
        let submit = self.submit_button(sending, stale, cx);
        let theme = cx.theme();
        let error = match state {
            SubmitState::Failed(message) => Some(
                h_flex()
                    .gap_2()
                    .items_start()
                    .text_xs()
                    .text_color(theme.danger)
                    .child(Icon::new(IconName::CircleAlert).small())
                    .child(div().flex_1().min_w_0().child(message)),
            ),
            SubmitState::Idle | SubmitState::Sending => None,
        };
        let side = v_flex()
            .w(px(SIDE_WIDTH))
            .flex_none()
            .h_full()
            .border_l_1()
            .border_color(theme.border)
            .child(
                v_flex()
                    .id("composer-side-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_3()
                    .gap_3()
                    .child(verdicts)
                    .children(comments),
            )
            .child(
                v_flex()
                    .flex_none()
                    .p_3()
                    .gap_2()
                    .children(error)
                    .children(stale_warning)
                    .child(submit),
            );
        v_flex()
            .key_context(REVIEW_COMPOSER_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|_, _: &CollapseReview, _, cx| cx.emit(ComposerEvent::Collapse)))
            .size_full()
            .border_t_1()
            .border_color(theme.primary)
            .bg(theme.popover)
            .text_sm()
            .child(header)
            .children(warning)
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_start()
                    .child(editor)
                    .child(side),
            )
    }
}

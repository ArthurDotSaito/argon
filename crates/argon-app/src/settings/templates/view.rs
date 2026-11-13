use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, Textarea};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    ClickEvent, Context, Div, ElementId, InteractiveElement as _, IntoElement, ParentElement as _,
    Render, StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _,
    px,
};

use argon_core::TemplateKind;

use super::naming::{is_built_in, variable_token, variables};
use super::{Load, TemplatesSection};
use crate::settings::labels;
use crate::settings::ui::{badge, card, field, note, section};

const LIST_WIDTH: f32 = 188.;

impl TemplatesSection {
    fn insert_variable(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let token = variable_token(name);
        self.body.update(cx, |input, cx| {
            input.insert(token, window, cx);
            input.focus(window, cx);
        });
        self.rerender(cx);
    }

    fn template_list(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let rows: Vec<_> = self
            .saved_templates()
            .iter()
            .enumerate()
            .map(|(ix, template)| {
                let selected = self.selected.as_ref() == Some(&template.id);
                let id = template.id.clone();
                let built_in = is_built_in(&template.id)
                    .then(|| badge(labels::BUILT_IN, theme.muted_foreground));
                h_flex()
                    .id(ElementId::NamedInteger(
                        "settings-template".into(),
                        ix as u64,
                    ))
                    .w_full()
                    .px_2()
                    .py_1()
                    .gap_1p5()
                    .items_center()
                    .rounded(theme.radius)
                    .cursor_pointer()
                    .when(selected, |row| row.bg(theme.primary.opacity(0.15)))
                    .hover(|style| style.bg(theme.muted.opacity(0.6)))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(div().text_sm().truncate().child(template.name.clone()))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .truncate()
                                    .child(template.id.clone()),
                            ),
                    )
                    .children(built_in)
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.request_select(id.clone(), window, cx)
                    }))
            })
            .collect();
        let invalid: Vec<Div> = self
            .invalid
            .iter()
            .map(|invalid| note(format!("{}: {}", invalid.id, invalid.error), theme.danger))
            .collect();
        let new = Button::new("settings-template-new")
            .icon(Icon::new(IconName::Plus))
            .label(labels::NEW)
            .xsmall()
            .ghost()
            .disabled(self.busy)
            .on_click(cx.listener(|this, _, window, cx| this.create(window, cx)));
        v_flex()
            .flex_none()
            .w(px(LIST_WIDTH))
            .gap_1()
            .child(h_flex().child(new))
            .children(rows)
            .children(invalid)
    }

    fn editor(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let Some(current) = self.current() else {
            return card(cx).child(note(labels::NO_TEMPLATE_SELECTED, theme.muted_foreground));
        };
        let built_in = is_built_in(&current.id);
        let dirty = self.is_dirty(cx);
        let uses_nonce = self.draft(cx).is_some_and(|draft| draft.uses_nonce());
        let no_nonce = match current.kind {
            TemplateKind::Review => labels::NO_NONCE,
            TemplateKind::Plan => labels::NO_NONCE_PLAN,
        };
        let chips: Vec<_> = variables(current.kind)
            .iter()
            .copied()
            .enumerate()
            .map(|(ix, name)| {
                Button::new(("settings-variable", ix))
                    .label(variable_token(name))
                    .xsmall()
                    .outline()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.insert_variable(name, window, cx)
                    }))
            })
            .collect();
        let remove = if built_in {
            Button::new("settings-template-restore")
                .icon(Icon::new(IconName::RotateCcw))
                .label(labels::RESTORE)
                .small()
                .ghost()
                .disabled(self.busy)
                .on_click(cx.listener(|this, _, window, cx| this.request_restore(window, cx)))
        } else {
            Button::new("settings-template-delete")
                .icon(Icon::new(IconName::Trash))
                .label(labels::DELETE)
                .small()
                .ghost()
                .disabled(self.busy)
                .on_click(cx.listener(|this, _, window, cx| this.request_delete(window, cx)))
        };
        let status = self
            .error
            .clone()
            .map(|error| note(error, theme.danger))
            .or_else(|| dirty.then(|| note(labels::UNSAVED, theme.warning)));
        card(cx)
            .child(field(labels::NAME, Input::new(&self.name).small(), cx))
            .child(
                div()
                    .w_full()
                    .font_family(theme.mono_font_family.clone())
                    .text_xs()
                    .child(Textarea::new(&self.body)),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(labels::VARIABLES),
                    )
                    .child(h_flex().flex_wrap().gap_1().children(chips)),
            )
            .children((!uses_nonce).then(|| note(no_nonce, theme.warning)))
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .items_center()
                    .child(
                        Button::new("settings-template-save")
                            .icon(Icon::new(IconName::Save))
                            .label(labels::SAVE)
                            .small()
                            .primary()
                            .loading(self.busy)
                            .disabled(!dirty || self.busy)
                            .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                    )
                    .child(
                        Button::new("settings-template-duplicate")
                            .icon(Icon::new(IconName::Copy))
                            .label(labels::DUPLICATE)
                            .small()
                            .ghost()
                            .disabled(self.busy)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.duplicate(window, cx)),
                            ),
                    )
                    .child(remove)
                    .children(status),
            )
    }
}

impl Render for TemplatesSection {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let content: Div = match &self.templates {
            Load::Loading => {
                card(cx).child(note(labels::LOADING_TEMPLATES, theme.muted_foreground))
            }
            Load::Failed(error) => card(cx).child(note(error.clone(), theme.danger)),
            Load::Ready(_) => h_flex()
                .w_full()
                .gap_3()
                .items_start()
                .child(self.template_list(cx))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_3()
                        .child(self.editor(cx))
                        .child(self.preview_pane(cx)),
                ),
        };
        section(labels::TEMPLATES, labels::TEMPLATES_HINT, cx)
            .child(self.review_options(cx))
            .child(content)
    }
}

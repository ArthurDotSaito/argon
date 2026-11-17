use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, Textarea, TextareaState};
use gpui_kit::component::select::Select;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, ClickEvent, Context, Div, ElementId, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Window,
    div, prelude::FluentBuilder as _, px,
};

use argon_core::{PROFILE_SLOT, PromptTemplate, TemplateKind};

use super::naming::{is_built_in, variable_token, variables};
use super::profile::{join_names, profiles_of};
use super::{Load, TemplatesSection};
use crate::settings::labels;
use crate::settings::ui::{badge, card, field, note, section};

const LIST_WIDTH: f32 = 188.;

enum ProfileMode {
    Plain,
    Following { base_edited: bool },
    Detached,
}

impl TemplatesSection {
    fn insert_variable(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let token = variable_token(name);
        let editor = if self.current().is_some_and(|t| t.base.is_some()) {
            self.profile.middle.clone()
        } else {
            self.body.clone()
        };
        editor.update(cx, |input, cx| {
            input.insert(token, window, cx);
            input.focus(window, cx);
        });
        self.rerender(cx);
    }

    fn template_name(&self, id: &str) -> String {
        self.saved_templates()
            .iter()
            .find(|template| template.id == id)
            .map_or_else(|| id.to_owned(), |template| template.name.clone())
    }

    fn row_detail(&self, template: &PromptTemplate) -> String {
        match &template.base {
            Some(base) => labels::profile_row(&self.template_name(base), template.detached),
            None => template.id.clone(),
        }
    }

    fn template_list(&self, cx: &Context<Self>) -> Div {
        let theme = cx.theme();
        let rows: Vec<_> = self
            .saved_templates()
            .iter()
            .enumerate()
            .map(|(ix, template)| {
                let selected = self.selected.as_ref() == Some(&template.id);
                let id = template.id.clone();
                let tag = if template.base.is_some() {
                    Some(badge(labels::PROFILE_BADGE, theme.primary))
                } else {
                    is_built_in(&template.id)
                        .then(|| badge(labels::BUILT_IN, theme.muted_foreground))
                };
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
                                    .child(self.row_detail(template)),
                            ),
                    )
                    .children(tag)
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
        let new_profile = Button::new("settings-template-new-profile")
            .icon(Icon::new(IconName::Plus))
            .label(labels::NEW_PROFILE)
            .xsmall()
            .ghost()
            .disabled(self.busy)
            .on_click(cx.listener(|this, _, window, cx| this.create_profile(window, cx)));
        v_flex()
            .flex_none()
            .w(px(LIST_WIDTH))
            .gap_1()
            .child(h_flex().flex_wrap().gap_1().child(new).child(new_profile))
            .children(rows)
            .children(invalid)
    }

    fn profile_mode(&self, current: &PromptTemplate, cx: &Context<Self>) -> ProfileMode {
        match (&current.base, current.detached) {
            (None, _) => ProfileMode::Plain,
            (Some(_), true) => ProfileMode::Detached,
            (Some(_), false) => ProfileMode::Following {
                base_edited: self.is_base_edited(cx),
            },
        }
    }

    fn text_block(
        &self,
        label: impl Into<SharedString>,
        editor: &Entity<TextareaState>,
        cx: &Context<Self>,
    ) -> Div {
        let theme = cx.theme();
        v_flex()
            .w_full()
            .gap_1()
            .child(note(label, theme.muted_foreground))
            .child(
                div()
                    .w_full()
                    .font_family(theme.mono_font_family.clone())
                    .text_xs()
                    .child(Textarea::new(editor)),
            )
    }

    fn body_editor(&self, current: &PromptTemplate, cx: &Context<Self>) -> Div {
        let theme = cx.theme();
        let profiles = join_names(profiles_of(self.saved_templates(), &current.id));
        let has_slot = self.body.read(cx).value().contains(PROFILE_SLOT);
        v_flex()
            .w_full()
            .gap_2()
            .child(
                div()
                    .w_full()
                    .font_family(theme.mono_font_family.clone())
                    .text_xs()
                    .child(Textarea::new(&self.body)),
            )
            .children((!profiles.is_empty()).then(|| {
                note(
                    labels::used_as_base(&profiles, has_slot),
                    theme.muted_foreground,
                )
            }))
    }

    fn profile_editor(&self, detached: bool, cx: &Context<Self>) -> Div {
        let theme = cx.theme();
        let base_name = self.base_label();
        let (start, end): (SharedString, SharedString) = if detached {
            (labels::OWN_TEXT_START.into(), labels::OWN_TEXT_END.into())
        } else {
            (
                labels::from_base_start(&base_name).into(),
                labels::from_base_end(&base_name).into(),
            )
        };
        let error = self.profile.error.clone().filter(|_| !detached);
        let show_base = error.is_none();
        let middle = v_flex()
            .w_full()
            .gap_1()
            .child(note(labels::PROFILE_TEXT, theme.primary))
            .child(
                div()
                    .w_full()
                    .p_1()
                    .rounded(theme.radius)
                    .border_1()
                    .border_color(theme.primary.opacity(0.45))
                    .bg(theme.primary.opacity(0.06))
                    .font_family(theme.mono_font_family.clone())
                    .text_xs()
                    .child(Textarea::new(&self.profile.middle)),
            );
        v_flex()
            .w_full()
            .gap_2()
            .child(field(
                labels::BASE,
                div()
                    .w_72()
                    .child(Select::new(&self.profile.base_select).small()),
                cx,
            ))
            .children(error.map(|error| note(error, theme.danger)))
            .children(show_base.then(|| self.text_block(start, &self.profile.head, cx)))
            .child(middle)
            .children(show_base.then(|| self.text_block(end, &self.profile.tail, cx)))
    }

    fn primary_button(
        &self,
        id: &'static str,
        label: impl Into<SharedString>,
        enabled: bool,
        cx: &Context<Self>,
        on_click: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        Button::new(id)
            .icon(Icon::new(IconName::Save))
            .label(label)
            .small()
            .primary()
            .loading(self.busy)
            .disabled(!enabled || self.busy)
            .on_click(cx.listener(move |this, _, window, cx| on_click(this, window, cx)))
            .into_any_element()
    }

    fn ghost_button(
        &self,
        id: &'static str,
        icon: IconName,
        label: impl Into<SharedString>,
        enabled: bool,
        cx: &Context<Self>,
        on_click: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        Button::new(id)
            .icon(Icon::new(icon))
            .label(label)
            .small()
            .ghost()
            .disabled(!enabled || self.busy)
            .on_click(cx.listener(move |this, _, window, cx| on_click(this, window, cx)))
            .into_any_element()
    }

    fn save_buttons(&self, mode: &ProfileMode, dirty: bool, cx: &Context<Self>) -> Vec<AnyElement> {
        match mode {
            ProfileMode::Plain => vec![self.primary_button(
                "settings-template-save",
                labels::SAVE,
                dirty,
                cx,
                |this, window, cx| this.save(window, cx),
            )],
            ProfileMode::Following { base_edited: false } => vec![self.primary_button(
                "settings-template-save",
                labels::SAVE,
                dirty,
                cx,
                |this, window, cx| this.save_profile(false, window, cx),
            )],
            ProfileMode::Following { base_edited: true } => {
                let base_name = self.base_label();
                vec![
                    self.primary_button(
                        "settings-profile-save-here",
                        labels::SAVE_HERE_ONLY,
                        true,
                        cx,
                        |this, window, cx| this.save_profile(true, window, cx),
                    ),
                    self.ghost_button(
                        "settings-profile-save-base",
                        IconName::Save,
                        labels::save_in_base(&base_name),
                        true,
                        cx,
                        |this, window, cx| this.save_in_base(window, cx),
                    ),
                    self.ghost_button(
                        "settings-profile-discard",
                        IconName::RotateCcw,
                        labels::DISCARD,
                        true,
                        cx,
                        |this, window, cx| this.discard_profile(window, cx),
                    ),
                ]
            }
            ProfileMode::Detached => {
                let can_follow = self.can_follow_again();
                vec![
                    self.primary_button(
                        "settings-template-save",
                        labels::SAVE,
                        dirty,
                        cx,
                        |this, window, cx| this.save_profile(true, window, cx),
                    ),
                    self.ghost_button(
                        "settings-profile-follow",
                        IconName::RotateCcw,
                        labels::FOLLOW_AGAIN,
                        can_follow,
                        cx,
                        |this, window, cx| this.request_follow_again(window, cx),
                    ),
                ]
            }
        }
    }

    fn editor(&self, cx: &Context<Self>) -> Div {
        let theme = cx.theme();
        let Some(current) = self.current() else {
            return card(cx).child(note(labels::NO_TEMPLATE_SELECTED, theme.muted_foreground));
        };
        let built_in = is_built_in(&current.id);
        let dirty = self.is_dirty(cx);
        let mode = self.profile_mode(current, cx);
        let no_nonce = matches!(
            self.resolved_draft(cx),
            Some(Ok(resolved)) if !resolved.uses_nonce()
        );
        let no_nonce_text = match current.kind {
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
            self.ghost_button(
                "settings-template-restore",
                IconName::RotateCcw,
                labels::RESTORE,
                true,
                cx,
                |this, window, cx| this.request_restore(window, cx),
            )
        } else {
            self.ghost_button(
                "settings-template-delete",
                IconName::Trash,
                labels::DELETE,
                true,
                cx,
                |this, window, cx| this.request_delete(window, cx),
            )
        };
        let duplicate = self.ghost_button(
            "settings-template-duplicate",
            IconName::Copy,
            labels::DUPLICATE,
            true,
            cx,
            |this, window, cx| this.duplicate(window, cx),
        );
        let status = self
            .error
            .clone()
            .map(|error| note(error, theme.danger))
            .or_else(|| dirty.then(|| note(labels::UNSAVED, theme.warning)));
        let base_edited = matches!(mode, ProfileMode::Following { base_edited: true });
        let edited_warning =
            base_edited.then(|| note(labels::base_edited(&self.base_label()), theme.warning));
        let edited_hint = base_edited.then(|| {
            note(
                labels::save_in_base_hint(&self.sibling_profiles()),
                theme.muted_foreground,
            )
        });
        let text = match mode {
            ProfileMode::Plain => self.body_editor(current, cx),
            ProfileMode::Following { .. } => self.profile_editor(false, cx),
            ProfileMode::Detached => self.profile_editor(true, cx),
        };
        let saves = self.save_buttons(&mode, dirty, cx);
        card(cx)
            .child(field(labels::NAME, Input::new(&self.name).small(), cx))
            .child(text)
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
            .children(no_nonce.then(|| note(no_nonce_text, theme.warning)))
            .children(
                self.base_lacks_slot()
                    .then(|| note(labels::base_without_slot(&self.base_label()), theme.warning)),
            )
            .children(edited_warning)
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .items_center()
                    .children(saves)
                    .child(duplicate)
                    .child(remove)
                    .children(status),
            )
            .children(edited_hint)
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
                        .children(self.repos_card(cx))
                        .child(self.preview_pane(cx)),
                ),
        };
        section(labels::TEMPLATES, labels::TEMPLATES_HINT, cx)
            .child(self.review_options(cx))
            .child(content)
    }
}

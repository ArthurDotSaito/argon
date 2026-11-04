use argon_core::PermissionMode;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::select::Select;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::{App, Context, Div, ParentElement as _, SharedString, Styled as _, div, px};

use super::{Load, StartDialog, open_settings};
use crate::agents::labels;

const FIELD_LABEL_WIDTH: f32 = 72.;

fn section_label(text: &'static str, cx: &App) -> Div {
    div()
        .flex_none()
        .w(px(FIELD_LABEL_WIDTH))
        .text_xs()
        .font_semibold()
        .text_color(cx.theme().muted_foreground)
        .child(text)
}

fn note(text: impl Into<SharedString>, color: gpui_kit::Hsla) -> Div {
    div().text_xs().text_color(color).child(text.into())
}

fn locked_switch(id: &'static str, label: &'static str, hint: &'static str, cx: &App) -> Div {
    v_flex()
        .gap_0p5()
        .child(
            h_flex()
                .gap_1p5()
                .items_center()
                .child(
                    Switch::new(id)
                        .checked(true)
                        .disabled(true)
                        .small()
                        .label(label),
                )
                .child(
                    Icon::new(IconName::Lock)
                        .xsmall()
                        .text_color(cx.theme().muted_foreground),
                ),
        )
        .child(note(hint, cx.theme().muted_foreground))
}

impl StartDialog {
    pub(super) fn selects(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let option = self.selected_option();
        let has_models = option.is_some_and(|option| !option.spec.models.is_empty());
        let model_placeholder = match option {
            Some(_) => labels::NO_MODEL_LISTED,
            None => labels::NO_MODELS,
        };
        let model = Select::new(&self.model_select)
            .small()
            .placeholder(model_placeholder)
            .disabled(!has_models || self.is_launching());
        let permissions = Select::new(&self.permission_select)
            .small()
            .disabled(self.selected_option().is_none() || self.is_launching());
        let risk_note = (self.permission_mode == PermissionMode::FullAccess)
            .then(|| note(labels::FULL_ACCESS_RISK, theme.warning).pl(px(FIELD_LABEL_WIDTH + 8.)));
        let template_placeholder = match &self.templates {
            Load::Loading => labels::LOADING_TEMPLATES,
            Load::Ready(_) | Load::Failed(_) => labels::NO_TEMPLATES,
        };
        let template = Select::new(&self.template_select)
            .small()
            .placeholder(template_placeholder)
            .disabled(self.templates.ready().is_none() || self.is_launching());
        let edit = Button::new("ai-edit-templates")
            .icon(Icon::new(IconName::Pencil))
            .label(labels::EDIT_IN_SETTINGS)
            .xsmall()
            .ghost()
            .disabled(self.is_launching())
            .on_click(|_, window, cx| open_settings(window, cx));
        let mut notes: Vec<Div> = Vec::new();
        if let Load::Failed(error) = &self.templates {
            notes.push(note(error.clone(), theme.danger));
        }
        notes.extend(self.invalid_templates.iter().map(|invalid| {
            note(
                labels::invalid_template(&invalid.id, &invalid.error),
                theme.warning,
            )
        }));
        match self.preview.as_ref().map(|preview| &preview.rendered) {
            Some(Ok(prompt)) => notes.extend(
                prompt
                    .warnings
                    .iter()
                    .map(|warning| note(warning.clone(), theme.warning)),
            ),
            Some(Err(error)) => notes.push(note(error.clone(), theme.danger)),
            None => {}
        }
        h_flex()
            .w_full()
            .gap_4()
            .items_start()
            .child(
                h_flex()
                    .flex_1()
                    .gap_2()
                    .items_center()
                    .child(section_label(labels::MODEL, cx))
                    .child(div().flex_1().child(model)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .gap_1()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(section_label(labels::PERMISSIONS, cx))
                            .child(div().flex_1().child(permissions)),
                    )
                    .children(risk_note),
            )
            .child(
                v_flex()
                    .flex_1()
                    .gap_1()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(section_label(labels::TEMPLATE, cx))
                            .child(div().flex_1().child(template))
                            .child(edit),
                    )
                    .child(
                        v_flex()
                            .pl(px(FIELD_LABEL_WIDTH + 8.))
                            .gap_0p5()
                            .children(notes),
                    ),
            )
    }

    pub(super) fn options(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let busy = self.is_launching();
        let entity = cx.entity().downgrade();
        let reviews = Switch::new("ai-include-reviews")
            .checked(self.include_reviews)
            .disabled(busy)
            .small()
            .label(labels::INCLUDE_REVIEWS)
            .on_click(move |on, _, cx| {
                _ = entity.update(cx, |this, cx| this.set_include_reviews(*on, cx));
            });
        let entity = cx.entity().downgrade();
        let ci = Switch::new("ai-include-ci")
            .checked(self.include_ci)
            .disabled(busy)
            .small()
            .label(labels::INCLUDE_CI)
            .on_click(move |on, _, cx| {
                _ = entity.update(cx, |this, cx| this.set_include_ci(*on, cx));
            });
        let ci_note = match &self.ci {
            Some(Load::Failed(error)) if self.include_ci => Some(note(error.clone(), theme.danger)),
            Some(Load::Loading) if self.include_ci => {
                Some(note(labels::LOADING_CI, theme.muted_foreground))
            }
            _ => None,
        };
        h_flex()
            .w_full()
            .gap_2()
            .items_start()
            .child(section_label(labels::OPTIONS, cx))
            .child(
                div()
                    .flex_1()
                    .grid()
                    .grid_cols(2)
                    .gap_x_6()
                    .gap_y_2()
                    .child(reviews)
                    .child(v_flex().gap_0p5().child(ci).children(ci_note))
                    .child(locked_switch(
                        "ai-worktree",
                        labels::ISOLATED_WORKTREE,
                        labels::ISOLATED_WORKTREE_HINT,
                        cx,
                    ))
                    .child(locked_switch(
                        "ai-drafts-only",
                        labels::DRAFTS_ONLY,
                        labels::DRAFTS_ONLY_HINT,
                        cx,
                    )),
            )
    }

    pub(super) fn permissions_warning(&self, cx: &App) -> Div {
        let theme = cx.theme();
        let agent = self
            .selected_option()
            .map_or(labels::GENERIC_AGENT, |option| option.spec.label);
        h_flex()
            .w_full()
            .gap_2()
            .items_start()
            .px_3()
            .py_2()
            .rounded(theme.radius)
            .border_1()
            .border_color(theme.warning.opacity(0.5))
            .bg(theme.warning.opacity(0.08))
            .child(
                Icon::new(IconName::ShieldAlert)
                    .small()
                    .text_color(theme.warning),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_xs()
                    .text_color(theme.foreground)
                    .child(labels::permissions_warning(agent)),
            )
    }
}

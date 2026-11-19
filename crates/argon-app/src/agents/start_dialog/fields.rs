use argon_core::{PermissionMode, SessionKind};
use chrono::Local;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::select::Select;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Selectable as _, Sizable as _, StyledExt as _,
    h_flex, v_flex,
};
use gpui_kit::{App, Context, Div, ParentElement as _, SharedString, Styled as _, div, px};

use super::{Gathering, Load, StartDialog, open_settings};
use crate::agents::labels;

const FIELD_LABEL_WIDTH: f32 = 72.;
const CHECKBOX_INDENT: f32 = 22.;

fn block(title: &'static str, trailing: Option<String>, body: Div, cx: &App) -> Div {
    let theme = cx.theme();
    v_flex()
        .w_full()
        .gap_1()
        .child(
            h_flex()
                .w_full()
                .gap_2()
                .text_xs()
                .font_semibold()
                .text_color(theme.muted_foreground)
                .child(title)
                .child(div().flex_1())
                .children(trailing.map(|text| div().font_normal().child(text))),
        )
        .child(
            body.w_full()
                .p_2()
                .rounded(theme.radius)
                .border_1()
                .border_color(theme.border),
        )
}

fn related_block(related: &[String], cx: &App) -> Div {
    let theme = cx.theme();
    let rows = related.iter().map(|repo| {
        h_flex()
            .w_full()
            .gap_2()
            .text_xs()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_family(theme.mono_font_family.clone())
                    .child(repo.clone()),
            )
            .child(
                div()
                    .flex_none()
                    .text_color(theme.muted_foreground)
                    .child(labels::PLAN_BASE_PENDING),
            )
    });
    let hint = if related.is_empty() {
        labels::NO_RELATED
    } else {
        labels::RELATED_HINT
    };
    block(
        labels::RELATED_PROJECTS,
        Some(labels::related_count(related.len())),
        v_flex()
            .gap_1()
            .children(rows)
            .child(note(hint, theme.muted_foreground)),
        cx,
    )
}

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
        let review = self.kind() == SessionKind::Review;
        let permissions = Select::new(&self.permission_select)
            .small()
            .disabled(!review || self.selected_option().is_none() || self.is_launching());
        let risk_note = match self.kind() {
            SessionKind::Plan => Some(note(labels::PLAN_MODE_LOCKED, theme.muted_foreground)),
            SessionKind::Summary => Some(note(labels::SUMMARY_MODE_LOCKED, theme.muted_foreground)),
            SessionKind::Review => (self.permission_mode == PermissionMode::FullAccess)
                .then(|| note(labels::FULL_ACCESS_RISK, theme.warning)),
        }
        .map(|note| note.w_full().min_w_0());
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
        let pin = (review && self.template.is_some()).then(|| {
            let pinned = self.is_pinned_to_repo();
            let repo = self.subject.repo();
            let (icon, label) = if pinned {
                (IconName::Check, labels::pinned_to_repo(repo))
            } else {
                (IconName::Tag, labels::pin_to_repo(repo))
            };
            Button::new("ai-pin-template")
                .icon(Icon::new(icon))
                .label(label)
                .xsmall()
                .ghost()
                .selected(pinned)
                .disabled(self.is_launching())
                .on_click(cx.listener(|this, _, _, cx| this.toggle_repo_pin(cx)))
        });
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
        let fields = h_flex()
            .w_full()
            .gap_4()
            .items_start()
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_2()
                    .items_center()
                    .child(section_label(labels::MODEL, cx))
                    .child(div().flex_1().min_w_0().child(model)),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_2()
                    .items_center()
                    .child(section_label(labels::PERMISSIONS, cx))
                    .child(div().flex_1().min_w_0().child(permissions)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(section_label(labels::TEMPLATE, cx))
                            .child(div().flex_1().min_w_0().child(template)),
                    )
                    .child(
                        v_flex()
                            .pl(px(FIELD_LABEL_WIDTH + 8.))
                            .gap_0p5()
                            .child(h_flex().child(edit))
                            .children(pin.map(|pin| h_flex().child(pin)))
                            .children(notes),
                    ),
            );
        v_flex().w_full().gap_1().child(fields).children(risk_note)
    }

    pub(super) fn options(&self, cx: &mut Context<Self>) -> Div {
        match self.kind() {
            SessionKind::Plan => return self.plan_options(cx),
            SessionKind::Summary => return self.summary_options(cx),
            SessionKind::Review => {}
        }
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

    fn summary_options(&self, cx: &mut Context<Self>) -> Div {
        let Some(Gathering::Summary(gathering)) = self.context.ready() else {
            return v_flex();
        };
        v_flex()
            .w_full()
            .gap_3()
            .child(self.project_block(cx))
            .child(related_block(&gathering.related, cx))
    }

    fn project_block(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let (status, hint, locked) = match self.saved_overview() {
            Some(overview) => {
                let saved = overview.updated_at.with_timezone(&Local).date_naive();
                let today = Local::now().date_naive();
                (
                    labels::overview_saved(saved, today, overview.source_pr),
                    labels::REFRESH_SAVED_HINT,
                    false,
                )
            }
            None => (
                labels::NO_OVERVIEW.to_owned(),
                labels::REFRESH_FIRST_HINT,
                true,
            ),
        };
        let entity = cx.entity().downgrade();
        let refresh = Checkbox::new("ai-refresh-project")
            .label(labels::REFRESH_PROJECT)
            .checked(self.refresh_project)
            .disabled(locked || self.is_launching())
            .small()
            .on_click(move |on, _, cx| {
                _ = entity.update(cx, |this, cx| this.set_refresh_project(*on, cx));
            });
        block(
            labels::PROJECT,
            None,
            v_flex()
                .gap_1p5()
                .child(div().text_xs().child(status))
                .child(
                    v_flex()
                        .gap_0p5()
                        .child(refresh)
                        .child(note(hint, theme.muted_foreground).pl(px(CHECKBOX_INDENT))),
                ),
            cx,
        )
    }

    fn plan_options(&self, cx: &App) -> Div {
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
                    .child(locked_switch(
                        "ai-read-only",
                        labels::READ_ONLY,
                        labels::READ_ONLY_HINT,
                        cx,
                    ))
                    .child(locked_switch(
                        "ai-worktree",
                        labels::ISOLATED_WORKTREE,
                        labels::ISOLATED_WORKTREE_HINT,
                        cx,
                    ))
                    .child(locked_switch(
                        "ai-drafts-only",
                        labels::PLAN_DRAFT_ONLY,
                        labels::PLAN_DRAFT_ONLY_HINT,
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
                    .child(labels::permissions_warning(agent, self.kind())),
            )
    }
}

use argon_core::{ReviewCfg, TemplateKind};
use gpui_kit::component::input::Input;
use gpui_kit::component::select::Select;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};
use gpui_kit::{Context, Div, ParentElement as _, Styled as _, Window, div};

use super::{TemplatesSection, templates_of};
use crate::agents::Choice;
use crate::config_watch;
use crate::settings::labels;
use crate::settings::ui::{card, field, note};

pub const DEFAULT_DIFF_LIMIT: usize = 60_000;

pub fn parse_limit(text: &str) -> Option<usize> {
    let digits: String = text
        .chars()
        .filter(|ch| !matches!(ch, '.' | '_' | ' ' | '\u{a0}'))
        .collect();
    digits.parse::<usize>().ok().filter(|limit| *limit > 0)
}

pub fn default_of(review: &ReviewCfg, kind: TemplateKind) -> &str {
    match kind {
        TemplateKind::Review => &review.default_template,
        TemplateKind::Plan => &review.default_plan_template,
        TemplateKind::Summary => &review.default_summary_template,
    }
}

fn set_default_of(review: &mut ReviewCfg, kind: TemplateKind, id: String) {
    match kind {
        TemplateKind::Review => review.default_template = id,
        TemplateKind::Plan => review.default_plan_template = id,
        TemplateKind::Summary => review.default_summary_template = id,
    }
}

impl TemplatesSection {
    pub(super) fn sync_review_options(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let choices: Vec<Choice> = templates_of(self.kind, self.saved_templates())
            .into_iter()
            .map(|template| Choice::new(template.id.clone(), template.name.clone()))
            .collect();
        let selected = default_of(&self.config.review, self.kind).to_owned();
        self.default_select.update(cx, |select, cx| {
            select.set_items(choices, window, cx);
            select.set_selected_value(&selected, window, cx);
        });
        let limit = match self.config.review.max_diff_chars {
            0 => DEFAULT_DIFF_LIMIT,
            limit => limit,
        };
        let current = parse_limit(&self.diff_limit.read(cx).value());
        if current != Some(limit) {
            self.diff_limit.update(cx, |input, cx| {
                input.set_value(limit.to_string(), window, cx)
            });
        }
        cx.notify();
    }

    pub(super) fn set_default_template(&mut self, id: String, cx: &mut Context<Self>) {
        let kind = self.kind;
        if default_of(&self.config.review, kind) != id {
            self.config =
                config_watch::update(cx, |config| set_default_of(&mut config.review, kind, id));
            cx.notify();
        }
    }

    fn set_include_diff(&mut self, on: bool, cx: &mut Context<Self>) {
        let limit = if on {
            parse_limit(&self.diff_limit.read(cx).value()).unwrap_or(DEFAULT_DIFF_LIMIT)
        } else {
            0
        };
        self.config = config_watch::update(cx, |config| config.review.max_diff_chars = limit);
        self.rerender(cx);
    }

    pub(super) fn commit_diff_limit(&mut self, cx: &mut Context<Self>) {
        if self.config.review.max_diff_chars == 0 {
            return;
        }
        let Some(limit) = parse_limit(&self.diff_limit.read(cx).value()) else {
            return;
        };
        if limit != self.config.review.max_diff_chars {
            self.config = config_watch::update(cx, |config| config.review.max_diff_chars = limit);
            self.rerender(cx);
        }
    }

    pub(super) fn review_options(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let include = self.config.review.max_diff_chars > 0;
        let entity = cx.entity().downgrade();
        let switch = Switch::new("settings-include-diff")
            .checked(include)
            .small()
            .label(labels::INCLUDE_DIFF)
            .on_click(move |on, _, cx| {
                _ = entity.update(cx, |this, cx| this.set_include_diff(*on, cx));
            });
        let limit = include.then(|| {
            h_flex()
                .gap_2()
                .items_center()
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(labels::DIFF_LIMIT),
                )
                .child(div().w_32().child(Input::new(&self.diff_limit).small()))
        });
        let diff = (self.kind == TemplateKind::Review).then(|| {
            v_flex()
                .gap_1()
                .child(
                    h_flex()
                        .gap_4()
                        .items_center()
                        .child(switch)
                        .children(limit),
                )
                .child(note(labels::INCLUDE_DIFF_HINT, theme.muted_foreground))
        });
        card(cx)
            .child(field(
                labels::DEFAULT_TEMPLATE,
                div()
                    .w_72()
                    .child(Select::new(&self.default_select).small()),
                cx,
            ))
            .children(diff)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_of_reads_and_writes_the_default_of_each_kind() {
        let mut review = ReviewCfg::default();
        for (kind, id) in [
            (TemplateKind::Review, "a"),
            (TemplateKind::Plan, "b"),
            (TemplateKind::Summary, "c"),
        ] {
            set_default_of(&mut review, kind, id.into());
            assert_eq!(default_of(&review, kind), id);
        }
        assert_eq!(review.default_template, "a");
        assert_eq!(review.default_plan_template, "b");
        assert_eq!(review.default_summary_template, "c");
    }

    #[test]
    fn parse_limit_accepts_grouped_digits() {
        assert_eq!(parse_limit("60000"), Some(60_000));
        assert_eq!(parse_limit(" 60.000 "), Some(60_000));
        assert_eq!(parse_limit("1_000"), Some(1_000));
        assert_eq!(parse_limit("0"), None);
        assert_eq!(parse_limit("abc"), None);
        assert_eq!(parse_limit(""), None);
    }
}

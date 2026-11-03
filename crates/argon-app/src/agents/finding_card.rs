use argon_core::{PrKey, Side};
use argon_store::FindingState;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{App, Div, ParentElement as _, SharedString, Styled as _, WeakEntity, Window, div};

use super::badges::{severity_counts, severity_pill};
use super::finding_set::{FindingEntry, SeverityCounts};
use super::panel_labels as labels;
use super::safe_markdown::agent_markdown;
use super::thread_model::ThreadModel;
use crate::workspace::Workspace;

#[derive(Clone)]
struct Target {
    pr: PrKey,
    path: String,
    line: u32,
    side: Side,
}

impl Target {
    fn reveal(&self, window: &mut Window, cx: &mut App) {
        let Some(workspace) = Workspace::global(cx) else {
            return;
        };
        _ = workspace.update(cx, |workspace, cx| {
            workspace.reveal_in_diff(
                self.pr.clone(),
                &self.path,
                self.line,
                self.side,
                window,
                cx,
            )
        });
    }
}

fn reveal_button(id: String, target: Target) -> Button {
    Button::new(SharedString::from(id))
        .icon(Icon::new(IconName::Crosshair))
        .label(labels::SHOW_IN_DIFF)
        .xsmall()
        .ghost()
        .on_click(move |_, window, cx| target.reveal(window, cx))
}

#[derive(Clone, Copy)]
struct CardState {
    busy: bool,
    in_diff: Option<bool>,
}

fn actions(
    model: WeakEntity<ThreadModel>,
    id: i64,
    entry: &FindingEntry,
    state: CardState,
    target: Target,
    cx: &App,
) -> Div {
    let theme = cx.theme();
    let busy = state.busy;
    let outside = state.in_diff == Some(false);
    let reveal = reveal_button(format!("finding-reveal-{id}"), target);
    let row = h_flex().gap_1p5().items_center().flex_wrap();
    match entry.state {
        FindingState::New => {
            let add_model = model.clone();
            row.child(
                Button::new(SharedString::from(format!("finding-add-{id}")))
                    .icon(Icon::new(IconName::MessageSquarePlus))
                    .label(labels::ADD_AS_DRAFT)
                    .xsmall()
                    .primary()
                    .loading(busy)
                    .disabled(busy || outside)
                    .on_click(move |_, _, cx| {
                        _ = add_model.update(cx, |model, cx| model.add_finding(id, cx));
                    }),
            )
            .child(
                Button::new(SharedString::from(format!("finding-ignore-{id}")))
                    .label(labels::IGNORE)
                    .xsmall()
                    .ghost()
                    .disabled(busy)
                    .on_click(move |_, _, cx| {
                        _ = model.update(cx, |model, cx| model.ignore_finding(id, cx));
                    }),
            )
            .children(outside.then(|| {
                div()
                    .text_xs()
                    .text_color(theme.warning)
                    .child(labels::OUT_OF_DIFF)
            }))
            .child(div().flex_1())
            .child(reveal)
        }
        FindingState::Added => row
            .child(
                div()
                    .text_xs()
                    .text_color(theme.success)
                    .child(labels::ADDED),
            )
            .child(div().flex_1())
            .child(reveal),
        FindingState::Ignored => row
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(labels::IGNORED),
            )
            .child(div().flex_1())
            .child(reveal),
    }
}

fn card(
    model: WeakEntity<ThreadModel>,
    id: i64,
    entry: &FindingEntry,
    state: CardState,
    pr: &PrKey,
    cx: &App,
) -> Div {
    let theme = cx.theme();
    let finding = &entry.finding;
    let location = labels::location(&finding.file, finding.line);
    let target = Target {
        pr: pr.clone(),
        path: finding.file.clone(),
        line: finding.line,
        side: finding.side,
    };
    let ignored = entry.state == FindingState::Ignored;
    let body = (!ignored && !finding.body.trim().is_empty()).then(|| {
        div().text_sm().child(agent_markdown(
            SharedString::from(format!("finding-body-{id}")),
            &finding.body,
        ))
    });
    let suggestion = finding
        .suggestion
        .as_ref()
        .filter(|code| !ignored && !code.trim().is_empty())
        .map(|code| {
            v_flex()
                .rounded(theme.radius)
                .border_1()
                .border_color(theme.border)
                .child(
                    div()
                        .px_1p5()
                        .py_0p5()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .border_b_1()
                        .border_color(theme.border)
                        .child(format!("{} · {location}", labels::SUGGESTION)),
                )
                .child(
                    div()
                        .p_1p5()
                        .bg(theme.success.opacity(0.08))
                        .text_xs()
                        .font_family(theme.mono_font_family.clone())
                        .child(code.trim_end().to_owned()),
                )
        });
    v_flex()
        .p_2()
        .gap_1p5()
        .rounded(theme.radius)
        .border_1()
        .border_color(theme.border)
        .when(ignored, |this| this.opacity(0.6))
        .child(
            h_flex()
                .gap_1p5()
                .items_start()
                .child(severity_pill(finding.severity, cx))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_sm()
                        .font_semibold()
                        .child(finding.title.clone()),
                )
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .font_family(theme.mono_font_family.clone())
                        .text_color(theme.link)
                        .child(location.clone()),
                ),
        )
        .children(body)
        .children(suggestion)
        .child(actions(model, id, entry, state, target, cx))
}

pub fn findings_block(
    model: WeakEntity<ThreadModel>,
    ids: &[i64],
    skipped: (usize, usize),
    thread: &ThreadModel,
    cx: &App,
) -> Div {
    let theme = cx.theme();
    let findings = thread.findings();
    let pr = thread.info().pr.clone();
    let entries: Vec<(i64, &FindingEntry)> = ids
        .iter()
        .filter_map(|id| findings.get(*id).map(|entry| (*id, entry)))
        .collect();
    let counts = SeverityCounts::tally(
        entries
            .iter()
            .map(|(_, entry)| (&entry.finding, entry.state)),
    );
    let heading = h_flex()
        .gap_2()
        .items_center()
        .text_xs()
        .font_semibold()
        .child(labels::findings_heading(entries.len()))
        .child(div().flex_1())
        .children((counts.total() > 0).then(|| severity_counts(counts, cx)));
    let empty = entries.is_empty().then(|| {
        div()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(labels::NO_FINDINGS)
    });
    let (outside, unreadable) = skipped;
    let notes = [
        (outside > 0).then(|| labels::skipped_outside(outside)),
        (unreadable > 0).then(|| labels::skipped_unreadable(unreadable)),
    ]
    .into_iter()
    .flatten()
    .map(|text| {
        div()
            .text_xs()
            .italic()
            .text_color(theme.muted_foreground)
            .child(text)
    });
    let cards: Vec<Div> = entries
        .iter()
        .map(|(id, entry)| {
            let state = CardState {
                busy: thread.is_finding_busy(*id),
                in_diff: thread.finding_in_diff(&entry.finding),
            };
            card(model.clone(), *id, entry, state, &pr, cx)
        })
        .collect();
    v_flex()
        .gap_2()
        .child(heading)
        .children(empty)
        .children(cards)
        .children(notes)
}

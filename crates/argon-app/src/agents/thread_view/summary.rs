use argon_core::RepoKey;
use chrono::Utc;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, ClipboardItem, Context, IntoElement, ParentElement as _, SharedString, Styled as _,
    div,
};

use super::ThreadView;
use crate::agents::panel_labels as labels;
use crate::agents::safe_markdown::agent_markdown;
use crate::agents::summary_card::{SummaryPart, summary_card_view};

fn part(
    id: SharedString,
    icon: IconName,
    content: SummaryPart,
    top_border: bool,
    cx: &Context<ThreadView>,
) -> gpui_kit::Div {
    let theme = cx.theme();
    let block = v_flex()
        .px_3p5()
        .py_3()
        .gap_2p5()
        .child(
            h_flex()
                .gap_1p5()
                .items_center()
                .text_xs()
                .font_semibold()
                .text_color(theme.muted_foreground)
                .child(Icon::new(icon).xsmall())
                .child(content.title),
        )
        .child(div().text_sm().child(agent_markdown(id, &content.body)));
    if top_border {
        block.border_t_1().border_color(theme.border)
    } else {
        block
    }
}

impl ThreadView {
    pub(super) fn summary_card(
        &self,
        id: i64,
        project_updated: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        let model = self.model.read(cx);
        let Some(summary) = model.summary(id) else {
            return div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(labels::SUMMARY_LOADING)
                .into_any_element();
        };
        let info = model.info();
        let view = summary_card_view(
            summary,
            model.project_overview(),
            info.number,
            project_updated,
            Utc::now(),
        );
        let project_key = model
            .project_repo()
            .map(|repo| RepoKey::new(info.pr.host.clone(), repo));
        let copy = view.copy;
        let theme = cx.theme();
        let note = view.note.map(|note| {
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(note)
        });
        let open_project = project_key.map(|key| {
            Button::new(SharedString::from(format!("summary-project-{id}")))
                .icon(Icon::new(IconName::BookOpen))
                .label(labels::VIEW_PROJECT)
                .xsmall()
                .outline()
                .on_click(move |_, window, cx| crate::project::open(key.clone(), window, cx))
        });
        let header = h_flex()
            .px_3()
            .py_2()
            .gap_2()
            .items_center()
            .bg(theme.muted)
            .border_b_1()
            .border_color(theme.border)
            .child(
                Icon::new(IconName::FileText)
                    .xsmall()
                    .text_color(theme.primary),
            )
            .child(div().text_sm().font_semibold().child(labels::SUMMARY))
            .children(note)
            .child(div().flex_1())
            .child(
                Button::new(SharedString::from(format!("summary-copy-{id}")))
                    .icon(Icon::new(IconName::Copy))
                    .label(labels::COPY_SUMMARY)
                    .xsmall()
                    .ghost()
                    .on_click(move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()))
                    }),
            )
            .children(open_project);
        let has_project = view.project.is_some();
        let project = view.project.map(|project| {
            part(
                SharedString::from(format!("summary-project-body-{id}")),
                IconName::BookOpen,
                project,
                false,
                cx,
            )
        });
        let pr = part(
            SharedString::from(format!("summary-pr-body-{id}")),
            IconName::GitPullRequest,
            view.pr,
            has_project,
            cx,
        );
        v_flex()
            .rounded(theme.radius)
            .border_1()
            .border_color(theme.border)
            .overflow_hidden()
            .child(header)
            .children(project)
            .child(pr)
            .into_any_element()
    }
}

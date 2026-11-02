use std::collections::HashMap;
use std::rc::Rc;

use argon_core::{DraftComment, DraftSource};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, Div, Hsla, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
};

use crate::pr::markdown;
use crate::review::diff_view::{BlockRender, InlineBlock};
use crate::review::labels;
use crate::ui;

const AVATAR_SIZE: f32 = 20.;

pub type DraftCallback = Rc<dyn Fn(i64, &mut Window, &mut App)>;

#[derive(Clone)]
pub struct DraftActions {
    pub edit: DraftCallback,
    pub delete: DraftCallback,
}

pub fn draft_blocks(
    drafts: &[DraftComment],
    hidden: Option<i64>,
    viewer: Option<&str>,
    actions: &DraftActions,
) -> HashMap<String, Vec<InlineBlock>> {
    let author: SharedString = viewer.unwrap_or(labels::YOU).to_string().into();
    let mut out: HashMap<String, Vec<InlineBlock>> = HashMap::new();
    for draft in drafts.iter().filter(|draft| Some(draft.id) != hidden) {
        let draft = Rc::new(draft.clone());
        let render_draft = draft.clone();
        let render_author = author.clone();
        let render_actions = actions.clone();
        let render: BlockRender =
            Rc::new(move |_, cx| draft_card(&render_draft, &render_author, &render_actions, cx));
        let start = draft
            .start_line
            .map(|line| (line, draft.start_side.unwrap_or(draft.side)));
        out.entry(draft.path.clone()).or_default().push(
            InlineBlock::line(
                (draft.line, draft.side),
                format!("draft-{}", draft.id),
                render,
            )
            .with_start(start),
        );
    }
    out
}

fn draft_card(
    draft: &DraftComment,
    author: &SharedString,
    actions: &DraftActions,
    cx: &mut App,
) -> AnyElement {
    let theme = cx.theme();
    let id = draft.id;
    let edit = actions.edit.clone();
    let delete = actions.delete.clone();
    let ai = (draft.source == DraftSource::Agent).then(|| {
        pill(labels::AI_BADGE, theme.info)
            .id(("draft-ai", id as u64))
            .tooltip(|window, cx| Tooltip::new(labels::AI_BADGE_TOOLTIP).build(window, cx))
    });
    let header = h_flex()
        .gap_2()
        .items_center()
        .child(ui::avatar(author, AVATAR_SIZE))
        .child(
            div()
                .font_semibold()
                .text_color(theme.foreground)
                .child(author.clone()),
        )
        .child(pill(labels::PENDING, theme.warning))
        .children(ai)
        .child(div().flex_1())
        .child(
            Button::new(("draft-edit", id as u64))
                .icon(Icon::new(IconName::Pencil))
                .label(labels::EDIT)
                .xsmall()
                .ghost()
                .on_click(move |_, window, cx| edit(id, window, cx)),
        )
        .child(
            Button::new(("draft-delete", id as u64))
                .icon(Icon::new(IconName::Trash))
                .label(labels::DELETE)
                .xsmall()
                .ghost()
                .on_click(move |_, window, cx| delete(id, window, cx)),
        );
    v_flex()
        .w_full()
        .px_3()
        .py_2()
        .gap_1()
        .rounded(theme.radius)
        .border_1()
        .border_color(theme.warning.opacity(0.5))
        .bg(theme.background)
        .text_sm()
        .font_family(theme.font_family.clone())
        .child(header)
        .child(div().pl_7().text_color(theme.foreground).child(markdown(
            format!("draft-body-{id}").into(),
            draft.body.clone().into(),
        )))
        .into_any_element()
}

fn pill(label: &'static str, color: Hsla) -> Div {
    div()
        .flex_none()
        .px_1p5()
        .rounded_full()
        .border_1()
        .border_color(color.opacity(0.6))
        .bg(color.opacity(0.12))
        .text_xs()
        .font_semibold()
        .text_color(color)
        .child(label)
}

#[cfg(test)]
mod tests {
    use argon_core::{PrKey, Side};

    use super::*;
    use crate::review::diff_view::BlockTarget;

    fn draft(id: i64, path: &str, start: Option<u32>) -> DraftComment {
        DraftComment {
            id,
            pr: PrKey::new("github.com", "PR_1"),
            path: path.to_string(),
            line: 10,
            side: Side::Right,
            start_line: start,
            start_side: None,
            body: "x".to_string(),
            source: DraftSource::User,
            session_id: None,
        }
    }

    fn actions() -> DraftActions {
        DraftActions {
            edit: Rc::new(|_, _, _| {}),
            delete: Rc::new(|_, _, _| {}),
        }
    }

    #[test]
    fn blocks_group_by_path_skip_the_edited_draft_and_keep_ranges() {
        let drafts = vec![
            draft(1, "a.ts", Some(7)),
            draft(2, "a.ts", None),
            draft(3, "b.ts", None),
        ];
        let blocks = draft_blocks(&drafts, Some(2), None, &actions());
        let a = &blocks["a.ts"];
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].key.as_ref(), "draft-1");
        assert_eq!(
            a[0].target,
            BlockTarget::Line {
                anchor: (10, Side::Right),
                start: Some((7, Side::Right))
            }
        );
        assert_eq!(blocks["b.ts"].len(), 1);
    }
}

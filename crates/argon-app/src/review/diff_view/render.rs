use std::collections::HashMap;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::diff::{Diff, DiffAnnotation, DiffDecoration, DiffHoverHighlight};
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, ElementId, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Window,
    div,
};

use super::DiffView;
use super::blocks::{BlockRender, place};
use super::convert::{position, range};
use super::header::viewed_toggle;
use crate::actions::ToggleDiffMode;
use crate::keymap::DIFF_VIEW_CONTEXT;
use crate::review::file_diff::FileBody;
use crate::review::labels;

const RANGE_ALPHA: f32 = 0.12;
const FLASH_ALPHA: f32 = 0.45;

type Renders = HashMap<ElementId, BlockRender>;

impl DiffView {
    fn layers(&self, cx: &App) -> (Vec<DiffAnnotation>, Renders, Vec<DiffDecoration>) {
        let theme = cx.theme();
        let mut annotations = Vec::new();
        let mut renders: Renders = HashMap::new();
        let mut decorations = Vec::new();
        for file in &self.files {
            let path = file.meta.path.as_str();
            if let Some(notice) = self.notice(file.body) {
                let id = ElementId::Name(format!("notice-{path}").into());
                annotations.push(DiffAnnotation::file(id.clone(), path.to_string()));
                renders.insert(id, notice);
            }
            let Some(set) = self.blocks.get(path) else {
                continue;
            };
            let placed = place(set, |anchor| file.has_line(anchor));
            for (block, placed) in set.iter().zip(placed) {
                let id = ElementId::Name(format!("{path}::{}", block.key).into());
                annotations.push(match placed.line {
                    Some(anchor) => DiffAnnotation::line(id.clone(), position(path, anchor)),
                    None => DiffAnnotation::file(id.clone(), path.to_string()),
                });
                if let (Some(end), Some(start)) = (placed.line, placed.start) {
                    decorations.push(
                        DiffDecoration::new(range(path, start, end))
                            .with_background(theme.primary.opacity(RANGE_ALPHA)),
                    );
                }
                renders.insert(id, block.render.clone());
            }
        }
        if let Some((path, anchor)) = &self.flash {
            decorations.push(
                DiffDecoration::new(range(path, *anchor, *anchor))
                    .with_background(theme.warning.opacity(FLASH_ALPHA)),
            );
        }
        (annotations, renders, decorations)
    }

    fn notice(&self, body: FileBody) -> Option<BlockRender> {
        let message: SharedString = match body {
            FileBody::Missing => labels::DIFF_TOO_LARGE.into(),
            FileBody::Invalid => labels::DIFF_INVALID.into(),
            FileBody::Ready | FileBody::Empty => return None,
        };
        let url = self.github_url.clone();
        Some(Rc::new(move |_, cx| {
            notice(message.clone(), url.clone(), cx)
        }))
    }
}

fn notice(message: SharedString, url: Option<String>, cx: &mut App) -> AnyElement {
    let theme = cx.theme();
    let link = url.map(|url| {
        div()
            .id("diff-notice-open")
            .text_color(theme.link)
            .cursor_pointer()
            .hover(|style| style.underline())
            .child(labels::OPEN_ON_GITHUB)
            .on_click(move |_: &ClickEvent, _, _| {
                if let Err(error) = open::that_detached(&url) {
                    tracing::warn!(%error, url, "failed to open diff in browser");
                }
            })
    });
    h_flex()
        .gap_2()
        .items_center()
        .text_sm()
        .text_color(theme.muted_foreground)
        .child(Icon::new(IconName::TriangleAlert).small())
        .child(message)
        .children(link)
        .into_any_element()
}

impl Render for DiffView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (annotations, renders, decorations) = self.layers(cx);
        let renders = Rc::new(renders);
        let header_view = cx.entity();
        let diff = Diff::new(&self.state)
            .hover_highlight(DiffHoverHighlight::LineNumber)
            .soft_wrap(true)
            .annotations(annotations)
            .render_annotation(
                move |annotation, window, cx| match renders.get(annotation.id()) {
                    Some(render) => render(window, cx),
                    None => div().into_any_element(),
                },
            )
            .decorations(decorations)
            .render_header_suffix(move |document, _, cx| viewed_toggle(&header_view, document, cx));
        let diff = diff.flex_1().min_h_0().border_0();
        v_flex()
            .size_full()
            .key_context(DIFF_VIEW_CONTEXT)
            .on_action(cx.listener(|this, _: &ToggleDiffMode, _, cx| this.toggle_mode(cx)))
            .child(diff)
    }
}

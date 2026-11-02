use argon_core::Side;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, h_flex};
use gpui_kit::{
    AnyElement, App, Context, ElementId, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, Window, div, list, px,
};

use super::header::{file_gap, file_header, hunk_header, notice, outdated_header};
use super::items::Item;
use super::row::{NUMBER_WIDTH, RowCx, render_row};
use super::{BlockRender, DiffMode, DiffView};
use crate::actions::ToggleDiffMode;
use crate::keymap::DIFF_VIEW_CONTEXT;

const BLOCK_PADDING: f32 = 6.;

impl DiffView {
    fn sticky_file(&self) -> Option<usize> {
        let top = self.list.logical_scroll_top();
        match self.items.get(top.item_ix)? {
            Item::FileHeader { .. } if top.offset_in_item <= px(0.) => None,
            Item::FileEnd { .. } => None,
            item => Some(item.file()),
        }
    }
}

impl Render for DiffView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.flush_dirty();
        if let Some(target) = self.pending_scroll.take() {
            self.apply_scroll(target);
        }
        let view = cx.entity();
        let sticky = self
            .sticky_file()
            .map(|file| file_header(&view, self, file, true, cx));
        let list_view = view.clone();
        div()
            .relative()
            .size_full()
            .key_context(DIFF_VIEW_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &ToggleDiffMode, _, cx| this.toggle_mode(cx)))
            .on_any_mouse_down(cx.listener(|this, _, window, cx| {
                window.focus(&this.focus_handle, cx);
            }))
            .child(
                list(self.list.clone(), move |ix, window, cx| {
                    render_item(&list_view, ix, window, cx)
                })
                .size_full(),
            )
            .vertical_scrollbar(&self.list)
            .children(sticky.map(|header| {
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .shadow_sm()
                    .child(header)
            }))
    }
}

fn render_item(
    view: &Entity<DiffView>,
    ix: usize,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let this = view.read(cx);
    let Some(item) = this.items.get(ix).copied() else {
        return div().into_any_element();
    };
    let mode = this.mode;
    match item {
        Item::FileHeader { file } => file_header(view, this, file, false, cx),
        Item::Notice { file } => notice(this, file, cx),
        Item::OutdatedHeader { .. } => outdated_header(cx),
        Item::FileEnd { .. } => file_gap(),
        Item::HunkHeader { file, hunk } => match this.files[file].body.diff() {
            Some(diff) => hunk_header(&diff.hunks[hunk], mode, cx),
            None => div().into_any_element(),
        },
        Item::Row {
            file,
            hunk,
            row,
            marked,
        } => {
            let entry = &this.files[file];
            let Some(diff) = entry.body.diff() else {
                return div().into_any_element();
            };
            let rcx = RowCx {
                view,
                this,
                file_ix: file,
                file: entry,
                diff,
            };
            render_row(&rcx, hunk, row, marked, cx)
        }
        Item::Outdated { file, block } => {
            let block = this.block(file, block, true);
            block_element(block, None, mode, window, cx)
        }
        Item::Floating { file, block } => {
            let block = this.block(file, block, false);
            block_element(block, None, mode, window, cx)
        }
        Item::Block { file, block, side } => {
            let block = this.block(file, block, false);
            block_element(block, Some(side), mode, window, cx)
        }
    }
}

fn block_element(
    block: Option<(SharedString, BlockRender)>,
    side: Option<Side>,
    mode: DiffMode,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let Some((key, render)) = block else {
        return div().into_any_element();
    };
    let element = render(window, cx);
    let theme = cx.theme();
    let content = div().flex_1().min_w_0().child(element);
    let spacer = || div().flex_1().min_w_0();
    let row = h_flex()
        .id(ElementId::Name(key))
        .w_full()
        .py(px(BLOCK_PADDING))
        .pr_3()
        .items_start()
        .border_y_1()
        .border_color(theme.border.opacity(0.5));
    match (mode, side) {
        (DiffMode::Split, Some(Side::Left)) => {
            row.child(content.pl(px(NUMBER_WIDTH))).child(spacer())
        }
        (DiffMode::Split, Some(Side::Right)) => {
            row.child(spacer()).child(content.pl(px(NUMBER_WIDTH)))
        }
        (DiffMode::Inline, Some(_)) => row.child(content.pl(px(NUMBER_WIDTH * 2.))),
        (_, None) => row.pl_4().child(content),
    }
    .into_any_element()
}

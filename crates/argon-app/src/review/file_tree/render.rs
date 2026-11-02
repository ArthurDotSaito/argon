use std::ops::Range;

use gpui_kit::assets::IconName;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::Input;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, ClickEvent, Context, ElementId, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _, Window, div, px,
    relative, uniform_list,
};

use super::model::TreeRow;
use super::{FileTree, FileTreeEvent, TreeFile};
use crate::review::labels;
use crate::review::status_badge::status_badge;
use crate::ui;

const ROW_HEIGHT: f32 = 26.;
const INDENT: f32 = 12.;
const ROW_PADDING: f32 = 8.;
const PROGRESS_HEIGHT: f32 = 3.;

impl FileTree {
    fn head(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let additions = self.files.iter().map(|file| file.additions).sum();
        let deletions = self.files.iter().map(|file| file.deletions).sum();
        let total = self.files.len();
        let viewed = self.viewed_count();
        let summary = if self.viewed_enabled {
            labels::viewed_count(viewed, total)
        } else {
            labels::file_count(total)
        };
        let progress = self.viewed_enabled.then(|| {
            let fraction = if total == 0 {
                0.
            } else {
                viewed as f32 / total as f32
            };
            div()
                .w_full()
                .h(px(PROGRESS_HEIGHT))
                .rounded_full()
                .bg(theme.muted)
                .child(
                    div()
                        .h_full()
                        .w(relative(fraction))
                        .rounded_full()
                        .bg(theme.success),
                )
        });
        v_flex()
            .flex_none()
            .p_2()
            .gap_2()
            .border_b_1()
            .border_color(theme.border)
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(summary)
                    .child(ui::diff_stat(additions, deletions, cx)),
            )
            .children(progress)
            .child(
                Input::new(&self.filter)
                    .xsmall()
                    .cleanable(true)
                    .prefix(Icon::new(IconName::Search).xsmall()),
            )
            .into_any_element()
    }

    fn render_rows(&self, range: Range<usize>, cx: &mut Context<Self>) -> Vec<AnyElement> {
        range
            .filter_map(|ix| Some((ix, self.rows.get(ix)?)))
            .map(|(ix, row)| match row {
                TreeRow::Dir {
                    depth,
                    key,
                    label,
                    collapsed,
                } => self.dir_row(ix, *depth, key, label, *collapsed, cx),
                TreeRow::File { depth, file, name } => match self.files.get(*file) {
                    Some(entry) => self.file_row(ix, *depth, entry, name, cx),
                    None => div().into_any_element(),
                },
            })
            .collect()
    }

    fn dir_row(
        &self,
        ix: usize,
        depth: usize,
        key: &str,
        label: &str,
        collapsed: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let (chevron, folder) = if collapsed {
            (IconName::ChevronRight, IconName::Folder)
        } else {
            (IconName::ChevronDown, IconName::FolderOpen)
        };
        let key = key.to_string();
        h_flex()
            .id(("review-tree-dir", ix))
            .w_full()
            .h(px(ROW_HEIGHT))
            .pl(px(ROW_PADDING + depth as f32 * INDENT))
            .pr_2()
            .gap_1()
            .items_center()
            .text_xs()
            .text_color(theme.muted_foreground)
            .cursor_pointer()
            .hover(|style| style.bg(theme.muted.opacity(0.5)))
            .child(Icon::new(chevron).xsmall())
            .child(Icon::new(folder).xsmall())
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .font_family(theme.mono_font_family.clone())
                    .child(label.to_string()),
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.toggle_dir(&key, cx)))
            .into_any_element()
    }

    fn file_row(
        &self,
        ix: usize,
        depth: usize,
        file: &TreeFile,
        name: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let active = self.active.as_deref() == Some(file.path.as_str());
        let viewed = self.viewed_enabled.then(|| {
            let path = file.path.clone();
            Checkbox::new(ElementId::NamedInteger(
                "review-tree-viewed".into(),
                ix as u64,
            ))
            .checked(file.viewed)
            .disabled(self.saving.contains(&file.path))
            .xsmall()
            .on_click(cx.listener(move |_, checked: &bool, _, cx| {
                cx.emit(FileTreeEvent::ToggleViewed {
                    path: path.clone(),
                    viewed: *checked,
                });
            }))
        });
        let open_path = file.path.clone();
        let name_color = if file.viewed {
            theme.muted_foreground
        } else {
            theme.foreground
        };
        let row = h_flex()
            .id(("review-tree-file", ix))
            .w_full()
            .h(px(ROW_HEIGHT))
            .pl(px(ROW_PADDING + depth as f32 * INDENT))
            .pr_2()
            .gap_1p5()
            .items_center()
            .text_xs()
            .cursor_pointer()
            .hover(|style| style.bg(theme.muted.opacity(0.5)))
            .child(status_badge(file.status, cx))
            .children(viewed)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_family(theme.mono_font_family.clone())
                    .text_color(name_color)
                    .child(name.to_string()),
            )
            .child(
                div()
                    .flex_none()
                    .child(ui::diff_stat(file.additions, file.deletions, cx)),
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.set_active(Some(open_path.clone()), cx);
                cx.emit(FileTreeEvent::Open(open_path.clone()));
            }));
        let row = if active {
            row.bg(theme.accent)
                .border_l_2()
                .border_color(theme.primary)
        } else {
            row
        };
        row.into_any_element()
    }
}

impl Render for FileTree {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let head = self.head(cx);
        let theme = cx.theme();
        let body = if self.rows.is_empty() {
            div()
                .p_3()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(if self.files.is_empty() {
                    labels::NO_FILES
                } else {
                    labels::NO_FILTER_MATCH
                })
                .into_any_element()
        } else {
            div()
                .relative()
                .flex_1()
                .min_h_0()
                .child(
                    uniform_list(
                        "review-file-tree",
                        self.rows.len(),
                        cx.processor(|this, range: Range<usize>, _, cx| {
                            this.render_rows(range, cx)
                        }),
                    )
                    .track_scroll(&self.scroll)
                    .size_full(),
                )
                .vertical_scrollbar(&self.scroll)
                .into_any_element()
        };
        v_flex()
            .size_full()
            .overflow_hidden()
            .bg(theme.sidebar)
            .child(head)
            .child(body)
    }
}

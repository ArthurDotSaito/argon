use std::rc::Rc;

use argon_core::{ArchGraph, ArchNode, RepoKey};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::scroll::{ScrollableElement as _, ScrollbarAxis};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Div, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, Stateful, StatefulInteractiveElement as _, Styled as _,
    Window, div, px,
};

use super::labels;
use super::project_view::ProjectView;
use crate::ui::arch_diagram::{
    self, DiagramOptions, SelectHandler, can_zoom_in, can_zoom_out, dot_grid, fit_step, kind_label,
    node_links, step_zoom, zoom_in, zoom_out, zoom_percent,
};
use crate::ui::properties;

const TOOLBAR_HEIGHT: f32 = 40.;
const BOARD_PADDING: f32 = 20.;
const ZOOM_VALUE_WIDTH: f32 = 44.;

impl ProjectView {
    pub(super) fn architecture_tab(
        &self,
        graph: Option<&ArchGraph>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(graph) = graph else {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(labels::NO_DIAGRAM)
                .into_any_element();
        };
        let toolbar = self.architecture_toolbar(graph, cx);
        let board = self.architecture_board(graph, cx);
        let inspector = self
            .arch_selected
            .as_deref()
            .and_then(|id| graph.nodes.iter().find(|node| node.id == id))
            .map(|node| self.node_inspector(graph, node, cx));
        v_flex()
            .size_full()
            .child(toolbar)
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(board)
                    .children(inspector),
            )
            .into_any_element()
    }

    fn set_arch_zoom(&mut self, step: usize, cx: &mut Context<Self>) {
        if self.arch_zoom != step {
            self.arch_zoom = step;
            cx.notify();
        }
    }

    fn fit_architecture(&mut self, base_width: f32, cx: &mut Context<Self>) {
        let visible = f32::from(self.arch_scroll.bounds().size.width) - 2. * BOARD_PADDING;
        self.set_arch_zoom(fit_step(base_width, visible), cx);
    }

    fn select_node(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        if self.arch_selected != id {
            self.arch_selected = id;
            cx.notify();
        }
    }

    fn architecture_toolbar(&self, graph: &ArchGraph, cx: &mut Context<Self>) -> Div {
        let step = self.arch_zoom;
        let base_width = arch_diagram::layout(graph, 1.).width;
        let zoom_out_button = Button::new("project-arch-zoom-out")
            .icon(Icon::new(IconName::Minus))
            .xsmall()
            .ghost()
            .tooltip(labels::ZOOM_OUT)
            .disabled(!can_zoom_out(step))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.set_arch_zoom(zoom_out(step), cx)
            }));
        let zoom_in_button = Button::new("project-arch-zoom-in")
            .icon(Icon::new(IconName::Plus))
            .xsmall()
            .ghost()
            .tooltip(labels::ZOOM_IN)
            .disabled(!can_zoom_in(step))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.set_arch_zoom(zoom_in(step), cx)
            }));
        let fit = Button::new("project-arch-fit")
            .label(labels::ZOOM_FIT)
            .xsmall()
            .outline()
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.fit_architecture(base_width, cx)
            }));
        let theme = cx.theme();
        let zoom = h_flex()
            .flex_none()
            .items_center()
            .rounded(theme.radius)
            .border_1()
            .border_color(theme.border)
            .text_color(theme.foreground)
            .child(zoom_out_button)
            .child(
                div()
                    .w(px(ZOOM_VALUE_WIDTH))
                    .text_center()
                    .font_family(theme.mono_font_family.clone())
                    .child(zoom_percent(step)),
            )
            .child(zoom_in_button);
        let strong = |text: String| div().font_medium().text_color(theme.foreground).child(text);
        let nodes = graph.nodes.len();
        let edges = graph.edges.len();
        let counts = h_flex()
            .flex_none()
            .gap_1()
            .child(strong(nodes.to_string()))
            .child(arch_diagram::labels::nodes_noun(nodes))
            .child("·")
            .child(strong(edges.to_string()))
            .child(arch_diagram::labels::edges_noun(edges));
        h_flex()
            .h(px(TOOLBAR_HEIGHT))
            .flex_none()
            .w_full()
            .px_4()
            .gap_4()
            .items_center()
            .overflow_hidden()
            .border_b_1()
            .border_color(theme.border)
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(counts)
            .children(arch_diagram::legend(cx))
            .child(div().flex_1())
            .child(zoom)
            .child(fit)
    }

    fn architecture_board(&self, graph: &ArchGraph, cx: &mut Context<Self>) -> Div {
        let view = cx.weak_entity();
        let on_select: SelectHandler = Rc::new(move |id: &str, _: &mut Window, cx: &mut App| {
            let id = id.to_owned();
            _ = view.update(cx, |this, cx| this.select_node(Some(id), cx));
        });
        let diagram = arch_diagram::arch_diagram(
            graph,
            DiagramOptions {
                selected: self.arch_selected.clone(),
                zoom: step_zoom(self.arch_zoom),
                title: Some(SharedString::from(
                    labels::repo_name(&self.key.repo).to_owned(),
                )),
                on_select: Some(on_select),
                ..DiagramOptions::default()
            },
            cx,
        );
        div()
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(cx.theme().background)
            .child(dot_grid(cx))
            .child(
                div()
                    .id("project-arch-board")
                    .size_full()
                    .track_scroll(&self.arch_scroll)
                    .overflow_scroll()
                    .p(px(BOARD_PADDING))
                    .child(diagram)
                    .scrollbar(&self.arch_scroll, ScrollbarAxis::Both),
            )
    }

    fn node_inspector(
        &self,
        graph: &ArchGraph,
        node: &ArchNode,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let close = Button::new("project-arch-close")
            .icon(Icon::new(IconName::X))
            .xsmall()
            .ghost()
            .tooltip(labels::CLOSE)
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.select_node(None, cx)));
        let open = node.repo.clone().map(|repo| {
            Button::new("project-arch-open")
                .icon(Icon::new(IconName::BookOpen))
                .label(labels::OPEN_PROJECT)
                .small()
                .outline()
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    let key = RepoKey::new(this.key.host.clone(), repo.clone());
                    super::open(key, window, cx)
                }))
        });
        let theme = cx.theme();
        let caption = match node.repo {
            Some(_) => format!("{} · {}", kind_label(node.kind), labels::RELATED_PROJECT),
            None => kind_label(node.kind).to_owned(),
        };
        let header = h_flex()
            .items_start()
            .gap_2()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(
                        div()
                            .font_semibold()
                            .text_color(theme.foreground)
                            .child(node.label.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(caption),
                    ),
            )
            .child(close);
        let links = node_links(graph, &node.id);
        let links = if links.is_empty() {
            v_flex().child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(labels::NO_LINKS),
            )
        } else {
            v_flex()
                .gap_1()
                .text_xs()
                .text_color(theme.foreground)
                .children(links)
        };
        let reading = if node.read {
            chip(labels::READ, theme.success)
        } else {
            chip(arch_diagram::labels::NOT_READ, theme.warning)
        };
        let repo = node.repo.clone().map(|repo| {
            properties::row(
                labels::NODE_REPO,
                div()
                    .min_w_0()
                    .truncate()
                    .font_family(theme.mono_font_family.clone())
                    .text_xs()
                    .child(repo),
                cx,
            )
        });
        properties::panel(cx)
            .id("project-arch-node")
            .overflow_y_scroll()
            .child(header)
            .child(properties::row(labels::NODE_LINKS, links, cx))
            .child(properties::row(labels::NODE_READING, reading, cx))
            .children(repo)
            .children(open)
    }
}

fn chip(text: &'static str, color: Hsla) -> Div {
    div()
        .flex_none()
        .px_1p5()
        .rounded_sm()
        .border_1()
        .border_color(color.opacity(0.5))
        .text_xs()
        .text_color(color)
        .child(text)
}

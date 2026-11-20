mod graph;
pub mod labels;
mod layout;
mod paint;
mod zoom;

#[cfg(test)]
mod tests;

use std::rc::Rc;

use argon_core::{ArchGraph, Via};
use gpui_kit::component::{ActiveTheme as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, ElementId, InteractiveElement as _, IntoElement, ParentElement as _,
    RenderOnce, SharedString, StatefulInteractiveElement as _, Styled as _, Window, canvas, div,
    px,
};

pub use graph::{excerpt, kind_label, node_links};
pub use layout::layout;
pub use paint::{SelectHandler, dot_grid};
pub use zoom::{
    DEFAULT_STEP, can_zoom_in, can_zoom_out, fit_step, step_zoom, zoom_in, zoom_out, zoom_percent,
};

use graph::is_hot_edge;
use paint::{EdgeStroke, NodeLook, edge_label, edges_canvas, node_element, project_box};

const FIGURE_GAP: f32 = 8.;
const FIGURE_MAX_WIDTH: f32 = 762.;
const LEGEND_GAP_X: f32 = 14.;
const LEGEND_GAP_Y: f32 = 2.;
const MEASURE_TOLERANCE: f32 = 0.5;
const MEASURE_KEY: &str = "arch-excerpt-width";

pub type OpenHandler = Rc<dyn Fn(&mut Window, &mut App)>;

#[derive(Clone)]
pub struct DiagramOptions {
    pub touched: Vec<String>,
    pub selected: Option<String>,
    pub zoom: f32,
    pub title: Option<SharedString>,
    pub on_select: Option<SelectHandler>,
}

impl Default for DiagramOptions {
    fn default() -> Self {
        Self {
            touched: Vec::new(),
            selected: None,
            zoom: 1.,
            title: None,
            on_select: None,
        }
    }
}

pub fn arch_diagram(graph: &ArchGraph, options: DiagramOptions, cx: &App) -> AnyElement {
    let layout = layout(graph, options.zoom);
    let zoom = layout.zoom;
    let touched = &options.touched;
    let strokes: Vec<EdgeStroke> = layout
        .edges
        .iter()
        .filter_map(|placed| {
            let edge = graph.edges.get(placed.index)?;
            Some(EdgeStroke {
                start: placed.start,
                segments: placed.segments.clone(),
                tip: placed.tip,
                arrow: placed.arrow,
                dashed: edge.via == Via::Queue,
                hot: is_hot_edge(edge, touched),
            })
        })
        .collect();
    let edge_labels: Vec<_> = layout
        .edges
        .iter()
        .filter_map(|placed| {
            let edge = graph.edges.get(placed.index)?;
            (!edge.label.is_empty()).then(|| {
                edge_label(
                    SharedString::from(edge.label.clone()),
                    placed.label_at,
                    placed.label_align,
                    is_hot_edge(edge, touched),
                    zoom,
                    cx,
                )
            })
        })
        .collect();
    let nodes: Vec<_> = layout
        .nodes
        .iter()
        .filter_map(|placed| {
            let node = graph.nodes.get(placed.index)?;
            let look = NodeLook {
                hot: touched.contains(&node.id),
                selected: options.selected.as_deref() == Some(node.id.as_str()),
            };
            Some(node_element(
                placed.index,
                node,
                placed.rect,
                look,
                zoom,
                options.on_select.clone(),
                cx,
            ))
        })
        .collect();
    let frame = layout
        .project_box
        .map(|rect| project_box(rect, options.title.clone(), zoom, cx));
    div()
        .relative()
        .flex_none()
        .w(px(layout.width))
        .h(px(layout.height))
        .children(frame)
        .child(edges_canvas(strokes, zoom, cx))
        .children(edge_labels)
        .children(nodes)
        .into_any_element()
}

fn legend_item(term: &'static str, meaning: &'static str, cx: &App) -> gpui_kit::Div {
    h_flex()
        .gap_1()
        .child(
            div()
                .font_medium()
                .text_color(cx.theme().foreground)
                .child(term),
        )
        .child(meaning)
}

pub fn legend(cx: &App) -> Vec<gpui_kit::Div> {
    vec![
        legend_item(labels::LEGEND_SOLID, labels::LEGEND_SOLID_MEANING, cx),
        legend_item(labels::LEGEND_DASHED, labels::LEGEND_DASHED_MEANING, cx),
        legend_item(
            labels::LEGEND_UNDERLINED,
            labels::LEGEND_UNDERLINED_MEANING,
            cx,
        ),
    ]
}

#[derive(IntoElement)]
pub struct ArchExcerpt {
    id: ElementId,
    graph: ArchGraph,
    touched: Vec<String>,
    title: Option<SharedString>,
    on_open: Option<OpenHandler>,
}

impl ArchExcerpt {
    pub fn new(id: impl Into<ElementId>, graph: &ArchGraph, touched: &[String]) -> Option<Self> {
        let graph = excerpt(graph, touched)?;
        Some(Self {
            id: id.into(),
            graph,
            touched: touched.to_vec(),
            title: None,
            on_open: None,
        })
    }

    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn on_open(mut self, on_open: OpenHandler) -> Self {
        self.on_open = Some(on_open);
        self
    }
}

impl RenderOnce for ArchExcerpt {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let measured = window.with_element_namespace(self.id.clone(), |window| {
            window.use_keyed_state(MEASURE_KEY, cx, |_, _| None::<f32>)
        });
        let available = *measured.read(cx);
        let base_width = layout(&self.graph, 1.).width;
        let zoom = zoom::excerpt_zoom(base_width, available);
        let probe = canvas(
            move |bounds, window, cx| {
                let width = f32::from(bounds.size.width);
                let changed =
                    available.is_none_or(|known| (known - width).abs() > MEASURE_TOLERANCE);
                if changed {
                    window.defer(cx, move |_, cx| {
                        measured.update(cx, |value, cx| {
                            *value = Some(width);
                            cx.notify();
                        });
                    });
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();
        let diagram = arch_diagram(
            &self.graph,
            DiagramOptions {
                touched: self.touched,
                zoom,
                title: self.title,
                ..DiagramOptions::default()
            },
            cx,
        );
        let theme = cx.theme();
        let link = self.on_open.map(|on_open| {
            div()
                .id("arch-excerpt-open")
                .text_color(theme.link)
                .cursor_pointer()
                .hover(|style| style.underline())
                .on_click(move |_, window, cx| on_open(window, cx))
                .child(labels::OPEN_BOARD)
        });
        let caption = h_flex()
            .flex_wrap()
            .gap_x(px(LEGEND_GAP_X))
            .gap_y(px(LEGEND_GAP_Y))
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(
                div()
                    .font_semibold()
                    .text_color(theme.primary)
                    .child(labels::EXCERPT_NOTE),
            )
            .children(legend(cx))
            .children(link);
        v_flex()
            .id(self.id)
            .w_full()
            .max_w(px(FIGURE_MAX_WIDTH))
            .gap(px(FIGURE_GAP))
            .child(
                div()
                    .id("arch-excerpt-board")
                    .relative()
                    .w_full()
                    .overflow_x_scroll()
                    .child(probe)
                    .child(diagram),
            )
            .child(caption)
    }
}

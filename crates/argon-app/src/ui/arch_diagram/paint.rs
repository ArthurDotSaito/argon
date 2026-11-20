use std::rc::Rc;

use argon_core::ArchNode;
use gpui_kit::component::{ActiveTheme as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::{
    App, Bounds, Canvas, Div, Hsla, InteractiveElement as _, ParentElement as _, PathBuilder,
    Pixels, Point, SharedString, Stateful, StatefulInteractiveElement as _, Styled as _, Window,
    canvas, div, fill, point, prelude::FluentBuilder as _, px, size,
};

use super::graph::node_caption;
use super::layout::{Arrow, LabelAlign, Pt, Rect, Segment};

pub type SelectHandler = Rc<dyn Fn(&str, &mut Window, &mut App)>;

const LINE_WIDTH: f32 = 1.4;
const HOT_LINE_WIDTH: f32 = 1.8;
const MIN_LINE_WIDTH: f32 = 1.;
const DASH: f32 = 6.;
const DASH_GAP: f32 = 4.;
const ARROW_HALF_WIDTH: f32 = 4.;
const NODE_RADIUS: f32 = 4.;
const NODE_PADDING: f32 = 10.;
const NODE_TITLE_SIZE: f32 = 12.5;
const NODE_TITLE_LINE: f32 = 16.;
const NODE_KIND_SIZE: f32 = 10.5;
const NODE_KIND_LINE: f32 = 14.;
const HOT_FILL: f32 = 0.14;
const BOX_RADIUS: f32 = 6.;
const BOX_LABEL_LEFT: f32 = 12.;
const BOX_LABEL_TOP: f32 = 8.;
const EDGE_LABEL_SIZE: f32 = 11.;
const EDGE_LABEL_HEIGHT: f32 = 16.;
const EDGE_LABEL_PADDING: f32 = 3.;
const EDGE_LABEL_HALF_SPAN: f32 = 130.;
const GRID_STEP: f32 = 20.;
const GRID_DOT: f32 = 1.;

pub struct EdgeStroke {
    pub start: Pt,
    pub segments: Vec<Segment>,
    pub tip: Pt,
    pub arrow: Arrow,
    pub dashed: bool,
    pub hot: bool,
}

pub struct NodeLook {
    pub hot: bool,
    pub selected: bool,
}

fn at(origin: Point<Pixels>, p: Pt) -> Point<Pixels> {
    point(origin.x + px(p.x), origin.y + px(p.y))
}

fn arrow_base(tip: Pt, arrow: Arrow, length: f32, half: f32) -> [Pt; 2] {
    match arrow {
        Arrow::Right => [
            Pt::new(tip.x - length, tip.y - half),
            Pt::new(tip.x - length, tip.y + half),
        ],
        Arrow::Left => [
            Pt::new(tip.x + length, tip.y - half),
            Pt::new(tip.x + length, tip.y + half),
        ],
        Arrow::Down => [
            Pt::new(tip.x - half, tip.y - length),
            Pt::new(tip.x + half, tip.y - length),
        ],
        Arrow::Up => [
            Pt::new(tip.x - half, tip.y + length),
            Pt::new(tip.x + half, tip.y + length),
        ],
    }
}

fn paint_edge(
    origin: Point<Pixels>,
    stroke: &EdgeStroke,
    zoom: f32,
    color: Hsla,
    window: &mut Window,
) {
    let base_width = if stroke.hot {
        HOT_LINE_WIDTH
    } else {
        LINE_WIDTH
    };
    let mut line = PathBuilder::stroke(px((base_width * zoom).max(MIN_LINE_WIDTH)));
    if stroke.dashed {
        line = line.dash_array(&[px(DASH * zoom), px(DASH_GAP * zoom)]);
    }
    line.move_to(at(origin, stroke.start));
    for segment in &stroke.segments {
        match *segment {
            Segment::Line(to) => line.line_to(at(origin, to)),
            Segment::Cubic { ctrl1, ctrl2, to } => {
                line.cubic_bezier_to(at(origin, to), at(origin, ctrl1), at(origin, ctrl2))
            }
        }
    }
    if let Ok(path) = line.build() {
        window.paint_path(path, color);
    }
    let [left, right] = arrow_base(
        stroke.tip,
        stroke.arrow,
        super::layout::ARROW_LENGTH * zoom,
        ARROW_HALF_WIDTH * zoom,
    );
    let mut head = PathBuilder::fill();
    head.move_to(at(origin, stroke.tip));
    head.line_to(at(origin, left));
    head.line_to(at(origin, right));
    head.close();
    if let Ok(path) = head.build() {
        window.paint_path(path, color);
    }
}

pub fn edges_canvas(strokes: Vec<EdgeStroke>, zoom: f32, cx: &App) -> Canvas<()> {
    let theme = cx.theme();
    let (cold, hot) = (theme.muted_foreground, theme.primary);
    canvas(
        |_, _, _| (),
        move |bounds, (), window, _| {
            for stroke in &strokes {
                let color = if stroke.hot { hot } else { cold };
                paint_edge(bounds.origin, stroke, zoom, color, window);
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

pub fn dot_grid(cx: &App) -> Canvas<()> {
    let color = cx.theme().border;
    canvas(
        |_, _, _| (),
        move |bounds: Bounds<Pixels>, (), window, _| {
            let area = bounds.intersect(&window.content_mask().bounds);
            let step = px(GRID_STEP);
            let dot = size(px(GRID_DOT), px(GRID_DOT));
            let mut y = bounds.origin.y + step / 2.;
            while y < area.bottom() {
                if y >= area.top() {
                    let mut x = bounds.origin.x + step / 2.;
                    while x < area.right() {
                        if x >= area.left() {
                            window.paint_quad(fill(Bounds::new(point(x, y), dot), color));
                        }
                        x += step;
                    }
                }
                y += step;
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

pub fn project_box(rect: Rect, title: Option<SharedString>, zoom: f32, cx: &App) -> Div {
    let theme = cx.theme();
    div()
        .absolute()
        .left(px(rect.x))
        .top(px(rect.y))
        .w(px(rect.w))
        .h(px(rect.h))
        .rounded(px(BOX_RADIUS * zoom))
        .border_1()
        .border_color(theme.border)
        .pl(px(BOX_LABEL_LEFT * zoom))
        .pt(px(BOX_LABEL_TOP * zoom))
        .text_size(px(NODE_KIND_SIZE * zoom))
        .line_height(px(NODE_KIND_LINE * zoom))
        .text_color(theme.muted_foreground)
        .whitespace_nowrap()
        .children(title)
}

pub fn edge_label(
    text: SharedString,
    anchor: Pt,
    align: LabelAlign,
    hot: bool,
    zoom: f32,
    cx: &App,
) -> Div {
    let theme = cx.theme();
    let height = EDGE_LABEL_HEIGHT * zoom;
    let chip = div()
        .flex_none()
        .px(px(EDGE_LABEL_PADDING * zoom))
        .bg(theme.background)
        .text_size(px(EDGE_LABEL_SIZE * zoom))
        .line_height(px(height))
        .whitespace_nowrap()
        .text_color(if hot {
            theme.primary
        } else {
            theme.muted_foreground
        })
        .when(hot, |this| this.font_semibold())
        .child(text);
    let row = h_flex()
        .absolute()
        .top(px(anchor.y - height / 2.))
        .h(px(height))
        .items_center();
    match align {
        LabelAlign::Center => {
            let half = EDGE_LABEL_HALF_SPAN * zoom;
            row.left(px(anchor.x - half))
                .w(px(half * 2.))
                .justify_center()
                .child(chip)
        }
        LabelAlign::Start => row.left(px(anchor.x)).child(chip),
    }
}

pub fn node_element(
    position: usize,
    node: &ArchNode,
    rect: Rect,
    look: NodeLook,
    zoom: f32,
    on_select: Option<SelectHandler>,
    cx: &App,
) -> Stateful<Div> {
    let theme = cx.theme();
    let ghost = !node.read;
    let border = if look.selected {
        theme.foreground
    } else if look.hot {
        theme.primary
    } else {
        theme.border
    };
    let background = if ghost {
        theme.background.opacity(0.)
    } else if look.hot {
        theme.muted.blend(theme.primary.opacity(HOT_FILL))
    } else {
        theme.muted
    };
    let title = div()
        .w_full()
        .truncate()
        .font_medium()
        .text_size(px(NODE_TITLE_SIZE * zoom))
        .line_height(px(NODE_TITLE_LINE * zoom))
        .text_color(if ghost {
            theme.muted_foreground
        } else {
            theme.foreground
        })
        .when(node.repo.is_some(), |this| this.underline())
        .child(node.label.clone());
    let caption = div()
        .w_full()
        .truncate()
        .text_size(px(NODE_KIND_SIZE * zoom))
        .line_height(px(NODE_KIND_LINE * zoom))
        .text_color(theme.muted_foreground)
        .child(node_caption(node));
    let id = node.id.clone();
    v_flex()
        .id(("arch-node", position))
        .absolute()
        .left(px(rect.x))
        .top(px(rect.y))
        .w(px(rect.w))
        .h(px(rect.h))
        .justify_center()
        .px(px(NODE_PADDING * zoom))
        .rounded(px(NODE_RADIUS * zoom))
        .map(|this| {
            if look.selected {
                this.border_2()
            } else {
                this.border_1()
            }
        })
        .when(ghost, |this| this.border_dashed())
        .border_color(border)
        .bg(background)
        .overflow_hidden()
        .child(title)
        .child(caption)
        .when_some(on_select, move |this, on_select| {
            this.cursor_pointer()
                .on_click(move |_, window, cx| on_select(&id, window, cx))
        })
}

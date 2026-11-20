use std::collections::HashMap;

use argon_core::{ArchGraph, Lane};

pub const NODE_WIDTH: f32 = 170.;
pub const NODE_HEIGHT: f32 = 40.;
pub const ARROW_LENGTH: f32 = 7.;
pub const LABEL_OFFSET: f32 = 6.;
const MARGIN: f32 = 20.;
const ROW_GAP: f32 = 50.;
const COLUMN_GAP: f32 = 70.;
const LANE_GAP: f32 = 110.;
const BOX_PADDING: f32 = 20.;
const BOX_HEADER: f32 = 30.;
const SIDE_OFFSET: f32 = 14.;
const BACK_STEP: f32 = 8.;
const MAX_BACK_LEVELS: usize = 6;
const ORDER_SWEEPS: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pt {
    pub x: f32,
    pub y: f32,
}

impl Pt {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    fn scaled(self, zoom: f32) -> Self {
        Self::new(self.x * zoom, self.y * zoom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn center_x(&self) -> f32 {
        self.x + self.w / 2.
    }

    pub fn center_y(&self) -> f32 {
        self.y + self.h / 2.
    }

    fn scaled(self, zoom: f32) -> Self {
        Self {
            x: self.x * zoom,
            y: self.y * zoom,
            w: self.w * zoom,
            h: self.h * zoom,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Segment {
    Line(Pt),
    Cubic { ctrl1: Pt, ctrl2: Pt, to: Pt },
}

impl Segment {
    fn scaled(self, zoom: f32) -> Self {
        match self {
            Self::Line(to) => Self::Line(to.scaled(zoom)),
            Self::Cubic { ctrl1, ctrl2, to } => Self::Cubic {
                ctrl1: ctrl1.scaled(zoom),
                ctrl2: ctrl2.scaled(zoom),
                to: to.scaled(zoom),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrow {
    Right,
    Left,
    Down,
    Up,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeShape {
    Curve,
    Vertical,
    Side,
    Back,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelAlign {
    Center,
    Start,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NodeLayout {
    pub index: usize,
    pub column: usize,
    pub rect: Rect,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EdgeLayout {
    pub index: usize,
    pub shape: EdgeShape,
    pub start: Pt,
    pub segments: Vec<Segment>,
    pub tip: Pt,
    pub arrow: Arrow,
    pub label_at: Pt,
    pub label_align: LabelAlign,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DiagramLayout {
    pub zoom: f32,
    pub nodes: Vec<NodeLayout>,
    pub edges: Vec<EdgeLayout>,
    pub project_box: Option<Rect>,
    pub width: f32,
    pub height: f32,
}

impl DiagramLayout {
    fn scaled(self, zoom: f32) -> Self {
        Self {
            zoom,
            nodes: self
                .nodes
                .into_iter()
                .map(|node| NodeLayout {
                    rect: node.rect.scaled(zoom),
                    ..node
                })
                .collect(),
            edges: self
                .edges
                .into_iter()
                .map(|edge| EdgeLayout {
                    start: edge.start.scaled(zoom),
                    segments: edge
                        .segments
                        .into_iter()
                        .map(|segment| segment.scaled(zoom))
                        .collect(),
                    tip: edge.tip.scaled(zoom),
                    label_at: edge.label_at.scaled(zoom),
                    ..edge
                })
                .collect(),
            project_box: self.project_box.map(|rect| rect.scaled(zoom)),
            width: self.width * zoom,
            height: self.height * zoom,
        }
    }
}

struct Column {
    lane: Lane,
    nodes: Vec<usize>,
}

struct Link {
    index: usize,
    from: usize,
    to: usize,
}

pub fn layout(graph: &ArchGraph, zoom: f32) -> DiagramLayout {
    let zoom = if zoom.is_finite() && zoom > 0. {
        zoom
    } else {
        1.
    };
    let links = links(graph);
    let mut columns = columns(graph, &links);
    order_columns(&mut columns, graph.nodes.len(), &links);
    place(&columns, &links).scaled(zoom)
}

fn links(graph: &ArchGraph) -> Vec<Link> {
    let ids: HashMap<&str, usize> = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(ix, node)| (node.id.as_str(), ix))
        .collect();
    graph
        .edges
        .iter()
        .enumerate()
        .filter_map(|(index, edge)| {
            let from = *ids.get(edge.from.as_str())?;
            let to = *ids.get(edge.to.as_str())?;
            (from != to).then_some(Link { index, from, to })
        })
        .collect()
}

fn columns(graph: &ArchGraph, links: &[Link]) -> Vec<Column> {
    let in_lane = |lane: Lane| -> Vec<usize> {
        graph
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.lane == lane)
            .map(|(ix, _)| ix)
            .collect()
    };
    let mut columns = Vec::new();
    let callers = in_lane(Lane::Callers);
    if !callers.is_empty() {
        columns.push(Column {
            lane: Lane::Callers,
            nodes: callers,
        });
    }
    let project = in_lane(Lane::Project);
    let layers = project_layers(&project, links);
    let depth = layers.iter().copied().max().map_or(0, |max| max + 1);
    for layer in 0..depth {
        let nodes: Vec<usize> = project
            .iter()
            .zip(&layers)
            .filter(|(_, node_layer)| **node_layer == layer)
            .map(|(node, _)| *node)
            .collect();
        if !nodes.is_empty() {
            columns.push(Column {
                lane: Lane::Project,
                nodes,
            });
        }
    }
    let uses = in_lane(Lane::Uses);
    if !uses.is_empty() {
        columns.push(Column {
            lane: Lane::Uses,
            nodes: uses,
        });
    }
    columns
}

fn project_layers(project: &[usize], links: &[Link]) -> Vec<usize> {
    let count = project.len();
    let local: HashMap<usize, usize> = project
        .iter()
        .enumerate()
        .map(|(local, node)| (*node, local))
        .collect();
    let mut outgoing = vec![Vec::new(); count];
    let mut has_caller = vec![false; count];
    for link in links {
        if let (Some(&from), Some(&to)) = (local.get(&link.from), local.get(&link.to)) {
            outgoing[from].push(to);
            has_caller[to] = true;
        }
    }
    let mut state = vec![Visit::New; count];
    let mut forward = Vec::new();
    let roots = (0..count)
        .filter(|node| !has_caller[*node])
        .chain(0..count)
        .collect::<Vec<_>>();
    for root in roots {
        if state[root] == Visit::New {
            visit(root, &outgoing, &mut state, &mut forward);
        }
    }
    let mut layers = vec![0usize; count];
    let cap = count.saturating_sub(1);
    for _ in 0..count {
        let mut changed = false;
        for &(from, to) in &forward {
            let next = (layers[from] + 1).min(cap);
            if layers[to] < next {
                layers[to] = next;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    layers
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Visit {
    New,
    Open,
    Done,
}

fn visit(
    node: usize,
    outgoing: &[Vec<usize>],
    state: &mut [Visit],
    forward: &mut Vec<(usize, usize)>,
) {
    state[node] = Visit::Open;
    for &to in &outgoing[node] {
        match state[to] {
            Visit::Open => {}
            Visit::Done => forward.push((node, to)),
            Visit::New => {
                forward.push((node, to));
                visit(to, outgoing, state, forward);
            }
        }
    }
    state[node] = Visit::Done;
}

fn order_columns(columns: &mut [Column], node_count: usize, links: &[Link]) {
    let mut column_of = vec![0usize; node_count];
    let mut position = vec![0f32; node_count];
    for (cix, column) in columns.iter().enumerate() {
        for node in &column.nodes {
            column_of[*node] = cix;
        }
        set_positions(&column.nodes, &mut position);
    }
    let mut neighbors = vec![Vec::new(); node_count];
    for link in links {
        neighbors[link.from].push(link.to);
        neighbors[link.to].push(link.from);
    }
    for sweep in 0..ORDER_SWEEPS {
        let backward = sweep % 2 == 1;
        let order: Vec<usize> = if backward {
            (0..columns.len()).rev().collect()
        } else {
            (0..columns.len()).collect()
        };
        for cix in order {
            let side = |other: usize| {
                if backward {
                    column_of[other] > cix
                } else {
                    column_of[other] < cix
                }
            };
            let mut keyed: Vec<(f32, usize)> = columns[cix]
                .nodes
                .iter()
                .map(|node| {
                    let near: Vec<f32> = neighbors[*node]
                        .iter()
                        .filter(|other| side(**other))
                        .map(|other| position[*other])
                        .collect();
                    let key = if near.is_empty() {
                        position[*node]
                    } else {
                        near.iter().sum::<f32>() / near.len() as f32
                    };
                    (key, *node)
                })
                .collect();
            keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
            columns[cix].nodes = keyed.into_iter().map(|(_, node)| node).collect();
            set_positions(&columns[cix].nodes, &mut position);
        }
    }
}

fn set_positions(nodes: &[usize], position: &mut [f32]) {
    let middle = nodes.len().saturating_sub(1) as f32 / 2.;
    for (row, node) in nodes.iter().enumerate() {
        position[*node] = row as f32 - middle;
    }
}

fn column_height(len: usize) -> f32 {
    if len == 0 {
        return 0.;
    }
    len as f32 * NODE_HEIGHT + (len - 1) as f32 * ROW_GAP
}

fn place(columns: &[Column], links: &[Link]) -> DiagramLayout {
    let has_project = columns.iter().any(|column| column.lane == Lane::Project);
    let content_height = columns
        .iter()
        .map(|column| column_height(column.nodes.len()))
        .fold(0., f32::max);
    let mut column_of = HashMap::new();
    let mut row_of = HashMap::new();
    for (cix, column) in columns.iter().enumerate() {
        for (row, node) in column.nodes.iter().enumerate() {
            column_of.insert(*node, cix);
            row_of.insert(*node, row);
        }
    }
    let back_count = links
        .iter()
        .filter(|link| column_of.get(&link.to) < column_of.get(&link.from))
        .count();
    let back_levels = back_count.min(MAX_BACK_LEVELS);
    let back_band = if back_levels == 0 {
        0.
    } else {
        BACK_STEP * (back_levels + 1) as f32
    };
    let header = if has_project { BOX_HEADER } else { 0. };
    let nodes_top = MARGIN + back_band + header;

    let mut nodes = Vec::new();
    let mut x = MARGIN;
    let mut box_span: Option<(f32, f32)> = None;
    let mut previous: Option<Lane> = None;
    for (cix, column) in columns.iter().enumerate() {
        match previous {
            None => {}
            Some(lane) if lane == column.lane => x += COLUMN_GAP,
            Some(_) => x += LANE_GAP,
        }
        if column.lane == Lane::Project && previous != Some(Lane::Project) {
            box_span = Some((x, x));
            x += BOX_PADDING;
        }
        let top = nodes_top + (content_height - column_height(column.nodes.len())) / 2.;
        for (row, node) in column.nodes.iter().enumerate() {
            nodes.push(NodeLayout {
                index: *node,
                column: cix,
                rect: Rect {
                    x,
                    y: top + row as f32 * (NODE_HEIGHT + ROW_GAP),
                    w: NODE_WIDTH,
                    h: NODE_HEIGHT,
                },
            });
        }
        x += NODE_WIDTH;
        if column.lane == Lane::Project {
            let next_is_project = columns
                .get(cix + 1)
                .is_some_and(|next| next.lane == Lane::Project);
            if !next_is_project {
                x += BOX_PADDING;
                if let Some((left, _)) = box_span {
                    box_span = Some((left, x));
                }
            }
        }
        previous = Some(column.lane);
    }
    let width = if columns.is_empty() {
        2. * MARGIN
    } else {
        x + MARGIN
    };
    let project_box = box_span.map(|(left, right)| Rect {
        x: left,
        y: nodes_top - BOX_HEADER,
        w: right - left,
        h: BOX_HEADER + content_height + BOX_PADDING,
    });
    let bottom_padding = if has_project { BOX_PADDING } else { 0. };
    let height = nodes_top + content_height + bottom_padding + MARGIN;

    let rect_of: HashMap<usize, Rect> = nodes.iter().map(|node| (node.index, node.rect)).collect();
    let mut back_seen = 0usize;
    let edges = links
        .iter()
        .filter_map(|link| {
            let source = *rect_of.get(&link.from)?;
            let target = *rect_of.get(&link.to)?;
            let from_column = column_of.get(&link.from)?;
            let to_column = column_of.get(&link.to)?;
            let edge = if to_column > from_column {
                curve(link.index, source, target)
            } else if to_column == from_column {
                let gap = row_of.get(&link.from)?.abs_diff(*row_of.get(&link.to)?);
                if gap <= 1 {
                    vertical(link.index, source, target)
                } else {
                    side(link.index, source, target)
                }
            } else {
                let level = back_seen % back_levels.max(1);
                back_seen += 1;
                let route_y = MARGIN + BACK_STEP * (level + 1) as f32;
                back(link.index, source, target, route_y)
            };
            Some(edge)
        })
        .collect();

    DiagramLayout {
        zoom: 1.,
        nodes,
        edges,
        project_box,
        width,
        height,
    }
}

fn curve(index: usize, source: Rect, target: Rect) -> EdgeLayout {
    let start = Pt::new(source.right(), source.center_y());
    let tip = Pt::new(target.x, target.center_y());
    let mid = start.x + (tip.x - start.x) / 2.;
    EdgeLayout {
        index,
        shape: EdgeShape::Curve,
        start,
        segments: vec![Segment::Cubic {
            ctrl1: Pt::new(mid, start.y),
            ctrl2: Pt::new(mid, tip.y),
            to: Pt::new(tip.x - ARROW_LENGTH, tip.y),
        }],
        tip,
        arrow: Arrow::Right,
        label_at: Pt::new(mid, (start.y + tip.y) / 2.),
        label_align: LabelAlign::Center,
    }
}

fn vertical(index: usize, source: Rect, target: Rect) -> EdgeLayout {
    let x = source.center_x();
    let (start, tip, base, arrow) = if target.y >= source.y {
        let tip = Pt::new(x, target.y);
        (
            Pt::new(x, source.bottom()),
            tip,
            Pt::new(x, tip.y - ARROW_LENGTH),
            Arrow::Down,
        )
    } else {
        let tip = Pt::new(x, target.bottom());
        (
            Pt::new(x, source.y),
            tip,
            Pt::new(x, tip.y + ARROW_LENGTH),
            Arrow::Up,
        )
    };
    EdgeLayout {
        index,
        shape: EdgeShape::Vertical,
        start,
        segments: vec![Segment::Line(base)],
        tip,
        arrow,
        label_at: Pt::new(x + LABEL_OFFSET, (start.y + tip.y) / 2.),
        label_align: LabelAlign::Start,
    }
}

fn side(index: usize, source: Rect, target: Rect) -> EdgeLayout {
    let lane_x = source.right() + SIDE_OFFSET;
    let start = Pt::new(source.right(), source.center_y());
    let tip = Pt::new(target.right(), target.center_y());
    EdgeLayout {
        index,
        shape: EdgeShape::Side,
        start,
        segments: vec![
            Segment::Line(Pt::new(lane_x, start.y)),
            Segment::Line(Pt::new(lane_x, tip.y)),
            Segment::Line(Pt::new(tip.x + ARROW_LENGTH, tip.y)),
        ],
        tip,
        arrow: Arrow::Left,
        label_at: Pt::new(lane_x + LABEL_OFFSET, (start.y + tip.y) / 2.),
        label_align: LabelAlign::Start,
    }
}

fn back(index: usize, source: Rect, target: Rect, route_y: f32) -> EdgeLayout {
    let out_x = source.right() + SIDE_OFFSET;
    let in_x = target.x - SIDE_OFFSET;
    let start = Pt::new(source.right(), source.center_y());
    let tip = Pt::new(target.x, target.center_y());
    EdgeLayout {
        index,
        shape: EdgeShape::Back,
        start,
        segments: vec![
            Segment::Line(Pt::new(out_x, start.y)),
            Segment::Line(Pt::new(out_x, route_y)),
            Segment::Line(Pt::new(in_x, route_y)),
            Segment::Line(Pt::new(in_x, tip.y)),
            Segment::Line(Pt::new(tip.x - ARROW_LENGTH, tip.y)),
        ],
        tip,
        arrow: Arrow::Right,
        label_at: Pt::new((out_x + in_x) / 2., route_y),
        label_align: LabelAlign::Center,
    }
}

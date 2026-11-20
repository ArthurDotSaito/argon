use std::time::{Duration, Instant};

use argon_core::{ArchEdge, ArchGraph, ArchNode, Lane, NodeKind, Via};

use super::graph::{excerpt, is_hot_edge, node_caption, node_links};
use super::layout::{
    DiagramLayout, EdgeShape, LabelAlign, NODE_HEIGHT, NODE_WIDTH, NodeLayout, Rect, Segment,
    layout,
};
use super::zoom::{
    DEFAULT_STEP, ZOOM_STEPS, can_zoom_in, can_zoom_out, excerpt_zoom, fit_step, zoom_in, zoom_out,
    zoom_percent,
};

fn node(id: &str, lane: Lane) -> ArchNode {
    ArchNode {
        id: id.into(),
        label: id.to_uppercase(),
        lane,
        kind: NodeKind::Component,
        repo: None,
        read: true,
    }
}

fn edge(from: &str, to: &str) -> ArchEdge {
    ArchEdge {
        from: from.into(),
        to: to.into(),
        label: String::new(),
        via: Via::Call,
    }
}

fn graph(nodes: Vec<ArchNode>, edges: Vec<ArchEdge>) -> ArchGraph {
    ArchGraph { nodes, edges }
}

trait Lookup {
    fn node(&self, index: usize) -> Option<&NodeLayout>;
}

impl Lookup for DiagramLayout {
    fn node(&self, index: usize) -> Option<&NodeLayout> {
        self.nodes.iter().find(|node| node.index == index)
    }
}

fn overlaps(a: &Rect, b: &Rect) -> bool {
    a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
}

fn column_of(layout: &DiagramLayout, index: usize) -> usize {
    layout.node(index).unwrap().column
}

fn x_of(layout: &DiagramLayout, index: usize) -> f32 {
    layout.node(index).unwrap().rect.x
}

fn distinct_x(layout: &DiagramLayout) -> usize {
    let mut xs: Vec<i64> = layout
        .nodes
        .iter()
        .map(|node| node.rect.x.round() as i64)
        .collect();
    xs.sort_unstable();
    xs.dedup();
    xs.len()
}

fn assert_no_overlap(layout: &DiagramLayout) {
    for (ix, a) in layout.nodes.iter().enumerate() {
        for b in &layout.nodes[ix + 1..] {
            assert!(!overlaps(&a.rect, &b.rect), "{a:?} overlaps {b:?}");
        }
    }
}

#[test]
fn a_single_lane_uses_one_column() {
    let g = graph(
        vec![node("a", Lane::Project), node("b", Lane::Project)],
        vec![],
    );
    let placed = layout(&g, 1.);
    assert_eq!(distinct_x(&placed), 1);
    assert!(placed.project_box.is_some());
    assert_eq!(placed.nodes[0].rect.w, NODE_WIDTH);
    assert_eq!(placed.nodes[0].rect.h, NODE_HEIGHT);
}

#[test]
fn two_lanes_go_left_to_right() {
    let g = graph(
        vec![node("api", Lane::Project), node("web", Lane::Callers)],
        vec![edge("web", "api")],
    );
    let placed = layout(&g, 1.);
    assert!(x_of(&placed, 1) < x_of(&placed, 0));
    assert_eq!(placed.edges.len(), 1);
    assert_eq!(placed.edges[0].shape, EdgeShape::Curve);
}

#[test]
fn three_lanes_are_callers_project_uses() {
    let g = graph(
        vec![
            node("db", Lane::Uses),
            node("api", Lane::Project),
            node("web", Lane::Callers),
        ],
        vec![edge("web", "api"), edge("api", "db")],
    );
    let placed = layout(&g, 1.);
    assert!(x_of(&placed, 2) < x_of(&placed, 1));
    assert!(x_of(&placed, 1) < x_of(&placed, 0));
    let frame = placed.project_box.unwrap();
    let api = placed.node(1).unwrap().rect;
    assert!(frame.x < api.x && frame.right() > api.right());
    assert!(frame.y < api.y && frame.bottom() > api.bottom());
    assert!(placed.node(2).unwrap().rect.right() < frame.x);
    assert!(placed.node(0).unwrap().rect.x > frame.right());
}

#[test]
fn an_empty_lane_takes_no_width() {
    let with_project = graph(
        vec![
            node("web", Lane::Callers),
            node("api", Lane::Project),
            node("db", Lane::Uses),
        ],
        vec![],
    );
    let without_project = graph(
        vec![node("web", Lane::Callers), node("db", Lane::Uses)],
        vec![],
    );
    let only_callers = graph(vec![node("web", Lane::Callers)], vec![]);
    let full = layout(&with_project, 1.);
    let gap = layout(&without_project, 1.);
    let single = layout(&only_callers, 1.);
    assert!(gap.width < full.width);
    assert!(gap.project_box.is_none());
    assert_eq!(single.width, NODE_WIDTH + 40.);
    assert!(single.nodes[0].rect.x <= 20.);
}

#[test]
fn nodes_in_the_same_column_do_not_overlap() {
    let nodes = (0..8)
        .map(|ix| node(&format!("n{ix}"), Lane::Uses))
        .chain((0..5).map(|ix| node(&format!("c{ix}"), Lane::Callers)))
        .collect();
    let placed = layout(&graph(nodes, vec![]), 1.);
    assert_no_overlap(&placed);
}

#[test]
fn project_components_are_layered_by_calls() {
    let g = graph(
        vec![
            node("c", Lane::Project),
            node("a", Lane::Project),
            node("b", Lane::Project),
            node("web", Lane::Callers),
            node("db", Lane::Uses),
        ],
        vec![
            edge("web", "c"),
            edge("a", "b"),
            edge("b", "c"),
            edge("c", "db"),
        ],
    );
    let placed = layout(&g, 1.);
    let (a, b, c) = (
        column_of(&placed, 1),
        column_of(&placed, 2),
        column_of(&placed, 0),
    );
    assert_eq!(b, a + 1);
    assert_eq!(c, a + 1 + 1);
    assert!(x_of(&placed, 1) < x_of(&placed, 2));
    assert!(x_of(&placed, 2) < x_of(&placed, 0));
    assert_eq!(column_of(&placed, 3) + 1, a);
    assert_eq!(column_of(&placed, 4), c + 1);
}

#[test]
fn a_cycle_ends_and_draws_the_closing_link_backwards() {
    let g = graph(
        vec![
            node("a", Lane::Project),
            node("b", Lane::Project),
            node("c", Lane::Project),
        ],
        vec![edge("a", "b"), edge("b", "c"), edge("c", "a")],
    );
    let placed = layout(&g, 1.);
    assert_eq!(placed.nodes.len(), 3);
    assert_eq!(placed.edges.len(), 3);
    assert!(
        placed
            .edges
            .iter()
            .any(|edge| edge.shape == EdgeShape::Back)
    );
    let top = placed
        .nodes
        .iter()
        .map(|node| node.rect.y)
        .fold(f32::MAX, f32::min);
    let back = placed
        .edges
        .iter()
        .find(|edge| edge.shape == EdgeShape::Back)
        .unwrap();
    assert!(back.label_at.y < top);
    assert!(back.label_at.y > 0.);
}

#[test]
fn a_cycle_without_entry_still_terminates() {
    let g = graph(
        vec![node("a", Lane::Project), node("b", Lane::Project)],
        vec![edge("a", "b"), edge("b", "a"), edge("a", "a")],
    );
    let placed = layout(&g, 1.);
    assert_eq!(placed.nodes.len(), 2);
    assert_eq!(placed.edges.len(), 2);
}

#[test]
fn a_link_inside_one_column_is_vertical() {
    let g = graph(
        vec![node("db", Lane::Uses), node("cache", Lane::Uses)],
        vec![edge("db", "cache")],
    );
    let placed = layout(&g, 1.);
    let link = &placed.edges[0];
    assert_eq!(link.shape, EdgeShape::Vertical);
    assert_eq!(link.start.x, link.tip.x);
    assert_eq!(link.label_align, LabelAlign::Start);
    assert!(matches!(link.segments[..], [Segment::Line(to)] if to.x == link.start.x));
}

#[test]
fn a_far_link_inside_one_column_goes_around_the_nodes() {
    let g = graph(
        vec![
            node("a", Lane::Uses),
            node("b", Lane::Uses),
            node("c", Lane::Uses),
        ],
        vec![edge("a", "c")],
    );
    let placed = layout(&g, 1.);
    let link = &placed.edges[0];
    assert_eq!(link.shape, EdgeShape::Side);
    let column_right = placed.nodes[0].rect.right();
    for segment in &link.segments {
        if let Segment::Line(to) = segment {
            assert!(to.x >= column_right);
        }
    }
}

#[test]
fn a_curve_goes_from_the_right_side_to_the_left_side() {
    let g = graph(
        vec![node("api", Lane::Project), node("db", Lane::Uses)],
        vec![edge("api", "db")],
    );
    let placed = layout(&g, 1.);
    let api = placed.node(0).unwrap().rect;
    let db = placed.node(1).unwrap().rect;
    let link = &placed.edges[0];
    assert_eq!(link.start.x, api.right());
    assert_eq!(link.tip.x, db.x);
    assert!(matches!(link.segments[..], [Segment::Cubic { .. }]));
}

#[test]
fn size_grows_with_the_nodes() {
    let small = graph(vec![node("a", Lane::Project)], vec![]);
    let tall = graph(
        (0..4)
            .map(|ix| node(&format!("n{ix}"), Lane::Project))
            .collect(),
        vec![],
    );
    let wide = graph(
        vec![node("a", Lane::Project), node("b", Lane::Project)],
        vec![edge("a", "b")],
    );
    let base = layout(&small, 1.);
    assert!(layout(&tall, 1.).height > base.height);
    assert!(layout(&wide, 1.).width > base.width);
}

#[test]
fn zoom_scales_everything() {
    let g = graph(
        vec![
            node("web", Lane::Callers),
            node("api", Lane::Project),
            node("db", Lane::Uses),
        ],
        vec![edge("web", "api"), edge("api", "db")],
    );
    let one = layout(&g, 1.);
    let half = layout(&g, 0.5);
    assert_eq!(half.width, one.width * 0.5);
    assert_eq!(half.height, one.height * 0.5);
    assert_eq!(half.nodes[1].rect.x, one.nodes[1].rect.x * 0.5);
    assert_eq!(half.nodes[1].rect.w, NODE_WIDTH * 0.5);
    assert_eq!(half.edges[0].tip.x, one.edges[0].tip.x * 0.5);
    assert_eq!(
        half.project_box.unwrap().w,
        one.project_box.unwrap().w * 0.5
    );
    assert_eq!(layout(&g, f32::NAN).width, one.width);
}

#[test]
fn the_largest_graph_lays_out_quickly_without_overlap() {
    let lanes = [Lane::Callers, Lane::Project, Lane::Project, Lane::Uses];
    let nodes: Vec<ArchNode> = (0..40)
        .map(|ix| node(&format!("n{ix}"), lanes[ix % lanes.len()]))
        .collect();
    let edges: Vec<ArchEdge> = (0..80)
        .map(|ix| edge(&format!("n{}", ix % 40), &format!("n{}", (ix * 7 + 3) % 40)))
        .collect();
    let g = graph(nodes, edges);
    let started = Instant::now();
    let placed = layout(&g, 1.);
    assert!(started.elapsed() < Duration::from_millis(200));
    assert_eq!(placed.nodes.len(), 40);
    assert_no_overlap(&placed);
    for link in &placed.edges {
        assert!(link.tip.x >= 0. && link.tip.x <= placed.width);
        assert!(link.label_at.y >= 0. && link.label_at.y <= placed.height);
    }
}

#[test]
fn ordering_puts_a_node_next_to_what_it_calls() {
    let g = graph(
        vec![
            node("web", Lane::Callers),
            node("app", Lane::Callers),
            node("x", Lane::Project),
            node("y", Lane::Project),
        ],
        vec![edge("web", "y"), edge("app", "x")],
    );
    let placed = layout(&g, 1.);
    let web = placed.node(0).unwrap().rect.y;
    let app = placed.node(1).unwrap().rect.y;
    let x = placed.node(2).unwrap().rect.y;
    let y = placed.node(3).unwrap().rect.y;
    assert_eq!(web < app, y < x);
}

fn sample() -> ArchGraph {
    let mut db = node("db", Lane::Uses);
    db.kind = NodeKind::Database;
    let mut pacientes = node("pacientes", Lane::Uses);
    pacientes.label = "pacientes-api".into();
    pacientes.kind = NodeKind::Service;
    pacientes.repo = Some("acme/pacientes-api".into());
    let mut fila = node("fila", Lane::Uses);
    fila.kind = NodeKind::Queue;
    fila.read = false;
    let mut clientes = node("clientes", Lane::Project);
    clientes.label = "ClientesHttp".into();
    graph(
        vec![
            node("web", Lane::Callers),
            node("api", Lane::Project),
            clientes,
            db,
            pacientes,
            fila,
        ],
        vec![
            edge("web", "api"),
            edge("api", "clientes"),
            ArchEdge {
                label: "HTTP".into(),
                ..edge("clientes", "pacientes")
            },
            ArchEdge {
                label: "EF Core".into(),
                ..edge("api", "db")
            },
            ArchEdge {
                label: "publica".into(),
                via: Via::Queue,
                ..edge("api", "fila")
            },
        ],
    )
}

fn ids(graph: &ArchGraph) -> Vec<&str> {
    graph.nodes.iter().map(|node| node.id.as_str()).collect()
}

#[test]
fn excerpt_keeps_touched_nodes_and_direct_neighbors() {
    let cut = excerpt(&sample(), &["clientes".into()]).unwrap();
    assert_eq!(ids(&cut), vec!["api", "clientes", "pacientes"]);
    assert_eq!(cut.edges.len(), 2);
    let wide = excerpt(&sample(), &["api".into()]).unwrap();
    assert_eq!(ids(&wide), vec!["web", "api", "clientes", "db", "fila"]);
    assert!(
        wide.edges
            .iter()
            .all(|edge| edge.from == "api" || edge.to == "api")
    );
}

#[test]
fn excerpt_ignores_unknown_ids() {
    let cut = excerpt(&sample(), &["ghost".into(), "db".into()]).unwrap();
    assert_eq!(ids(&cut), vec!["api", "db"]);
    assert!(excerpt(&sample(), &["ghost".into()]).is_none());
}

#[test]
fn excerpt_of_nothing_is_nothing() {
    assert!(excerpt(&sample(), &[]).is_none());
}

#[test]
fn a_link_is_hot_only_when_both_ends_are_touched() {
    let touched = vec!["api".to_owned(), "fila".to_owned()];
    let g = sample();
    let hot: Vec<bool> = g
        .edges
        .iter()
        .map(|edge| is_hot_edge(edge, &touched))
        .collect();
    assert_eq!(hot, vec![false, false, false, false, true]);
}

#[test]
fn node_links_describe_each_connection() {
    let g = sample();
    assert_eq!(
        node_links(&g, "pacientes"),
        vec!["ClientesHttp chama por HTTP"]
    );
    assert_eq!(
        node_links(&g, "api"),
        vec![
            "WEB chama",
            "chama ClientesHttp",
            "chama DB por EF Core",
            "envia por fila para FILA (publica)",
        ]
    );
    assert_eq!(node_links(&g, "fila"), vec!["API envia por fila (publica)"]);
    assert!(node_links(&g, "ghost").is_empty());
}

#[test]
fn package_links_read_naturally() {
    let g = graph(
        vec![node("api", Lane::Project), node("contratos", Lane::Uses)],
        vec![ArchEdge {
            via: Via::Package,
            ..edge("api", "contratos")
        }],
    );
    assert_eq!(node_links(&g, "api"), vec!["usa o pacote CONTRATOS"]);
    assert_eq!(node_links(&g, "contratos"), vec!["API usa como pacote"]);
}

#[test]
fn node_caption_names_the_kind_and_unread() {
    let g = sample();
    assert_eq!(node_caption(&g.nodes[3]), "banco");
    assert_eq!(node_caption(&g.nodes[4]), "serviço");
    assert_eq!(node_caption(&g.nodes[5]), "fila · não lido");
}

#[test]
fn zoom_steps_stop_at_the_limits() {
    assert_eq!(zoom_percent(DEFAULT_STEP), "100%");
    assert_eq!(zoom_out(0), 0);
    assert_eq!(zoom_in(ZOOM_STEPS.len() - 1), ZOOM_STEPS.len() - 1);
    assert_eq!(zoom_percent(zoom_in(DEFAULT_STEP)), "125%");
    assert_eq!(zoom_percent(zoom_out(DEFAULT_STEP)), "75%");
    assert_eq!(zoom_percent(0), "50%");
    assert_eq!(zoom_percent(4), "150%");
    assert!(!can_zoom_out(0) && can_zoom_out(1));
    assert!(!can_zoom_in(4) && can_zoom_in(3));
}

#[test]
fn fit_picks_the_largest_step_that_fits() {
    assert_eq!(zoom_percent(fit_step(1000., 2000.)), "150%");
    assert_eq!(zoom_percent(fit_step(1000., 1300.)), "125%");
    assert_eq!(zoom_percent(fit_step(1000., 1000.)), "100%");
    assert_eq!(zoom_percent(fit_step(1000., 760.)), "75%");
    assert_eq!(zoom_percent(fit_step(1000., 300.)), "50%");
    assert_eq!(fit_step(1000., 0.), DEFAULT_STEP);
    assert_eq!(fit_step(1000., f32::NAN), DEFAULT_STEP);
}

#[test]
fn excerpt_zoom_shrinks_to_fit_but_not_below_sixty_percent() {
    assert_eq!(excerpt_zoom(800., Some(1000.)), 1.);
    assert_eq!(excerpt_zoom(1000., Some(800.)), 0.8);
    assert_eq!(excerpt_zoom(1000., Some(300.)), 0.6);
    assert_eq!(excerpt_zoom(1000., None), 1.);
}

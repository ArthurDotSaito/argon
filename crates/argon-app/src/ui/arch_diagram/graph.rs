use std::collections::HashSet;

use argon_core::{ArchEdge, ArchGraph, ArchNode, NodeKind, Via};

use super::labels;

pub fn excerpt(graph: &ArchGraph, touched: &[String]) -> Option<ArchGraph> {
    let touched: HashSet<&str> = touched
        .iter()
        .map(String::as_str)
        .filter(|id| graph.nodes.iter().any(|node| node.id == *id))
        .collect();
    if touched.is_empty() {
        return None;
    }
    let edges: Vec<ArchEdge> = graph
        .edges
        .iter()
        .filter(|edge| touched.contains(edge.from.as_str()) || touched.contains(edge.to.as_str()))
        .cloned()
        .collect();
    let kept: HashSet<&str> = edges
        .iter()
        .flat_map(|edge| [edge.from.as_str(), edge.to.as_str()])
        .chain(touched.iter().copied())
        .collect();
    let nodes: Vec<ArchNode> = graph
        .nodes
        .iter()
        .filter(|node| kept.contains(node.id.as_str()))
        .cloned()
        .collect();
    let ids: HashSet<&str> = nodes.iter().map(|node| node.id.as_str()).collect();
    let edges = edges
        .into_iter()
        .filter(|edge| ids.contains(edge.from.as_str()) && ids.contains(edge.to.as_str()))
        .collect();
    Some(ArchGraph { nodes, edges })
}

pub fn is_hot_edge(edge: &ArchEdge, touched: &[String]) -> bool {
    touched.contains(&edge.from) && touched.contains(&edge.to)
}

pub fn kind_label(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Component => labels::KIND_COMPONENT,
        NodeKind::Service => labels::KIND_SERVICE,
        NodeKind::Queue => labels::KIND_QUEUE,
        NodeKind::Database => labels::KIND_DATABASE,
        NodeKind::Cache => labels::KIND_CACHE,
        NodeKind::External => labels::KIND_EXTERNAL,
    }
}

pub fn node_caption(node: &ArchNode) -> String {
    let kind = kind_label(node.kind);
    if node.read {
        kind.to_owned()
    } else {
        format!("{kind} · {}", labels::NOT_READ)
    }
}

pub fn node_links(graph: &ArchGraph, id: &str) -> Vec<String> {
    let label_of = |other: &str| {
        graph
            .nodes
            .iter()
            .find(|node| node.id == other)
            .map_or_else(|| other.to_owned(), |node| node.label.clone())
    };
    graph
        .edges
        .iter()
        .filter(|edge| edge.from != edge.to)
        .filter_map(|edge| {
            if edge.to == id {
                Some(incoming_text(&label_of(&edge.from), edge))
            } else if edge.from == id {
                Some(outgoing_text(&label_of(&edge.to), edge))
            } else {
                None
            }
        })
        .collect()
}

fn detail(edge: &ArchEdge) -> String {
    let label = edge.label.trim();
    match (edge.via, label.is_empty()) {
        (_, true) => String::new(),
        (Via::Call, false) => format!(" por {label}"),
        (Via::Queue | Via::Package, false) => format!(" ({label})"),
    }
}

fn incoming_text(other: &str, edge: &ArchEdge) -> String {
    let verb = match edge.via {
        Via::Call => labels::LINK_CALLS,
        Via::Queue => labels::LINK_SENDS_BY_QUEUE,
        Via::Package => labels::LINK_USES_AS_PACKAGE,
    };
    format!("{other} {verb}{}", detail(edge))
}

fn outgoing_text(other: &str, edge: &ArchEdge) -> String {
    let lead = match edge.via {
        Via::Call => labels::LINK_CALLS.to_owned(),
        Via::Queue => format!("{} {}", labels::LINK_SENDS_BY_QUEUE, labels::LINK_TO),
        Via::Package => labels::LINK_USES_PACKAGE.to_owned(),
    };
    format!("{lead} {other}{}", detail(edge))
}

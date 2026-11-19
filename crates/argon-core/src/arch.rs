use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::fence::last_fenced_block;

#[cfg(test)]
mod tests;

const ARCH_INFO_PREFIX: &str = "argon-arch-";
const TOUCHED_INFO_PREFIX: &str = "argon-touched-";
const MAX_ID_CHARS: usize = 40;
const MAX_LABEL_CHARS: usize = 40;
const ELLIPSIS: char = '…';

pub const MAX_ARCH_NODES: usize = 40;
pub const MAX_ARCH_EDGES: usize = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lane {
    Callers,
    Project,
    Uses,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
    Component,
    Service,
    Queue,
    Database,
    Cache,
    External,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Via {
    Call,
    Queue,
    Package,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchNode {
    pub id: String,
    pub label: String,
    pub lane: Lane,
    pub kind: NodeKind,
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default = "default_read")]
    pub read: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchEdge {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub label: String,
    pub via: Via,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchGraph {
    pub nodes: Vec<ArchNode>,
    #[serde(default)]
    pub edges: Vec<ArchEdge>,
}

fn default_read() -> bool {
    true
}

fn is_invisible_format(c: char) -> bool {
    matches!(
        c,
        '\u{00ad}'
            | '\u{061c}'
            | '\u{180e}'
            | '\u{200b}'..='\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206f}'
            | '\u{feff}'
    )
}

fn clean_label(raw: &str) -> String {
    let spaced: String = raw
        .chars()
        .map(|c| {
            if c.is_control() || is_invisible_format(c) {
                ' '
            } else {
                c
            }
        })
        .collect();
    let joined = spaced.split_whitespace().collect::<Vec<_>>().join(" ");
    if joined.chars().count() <= MAX_LABEL_CHARS {
        return joined;
    }
    let mut cut: String = joined.chars().take(MAX_LABEL_CHARS - 1).collect();
    cut.truncate(cut.trim_end().len());
    cut.push(ELLIPSIS);
    cut
}

fn is_valid_id(id: &str) -> bool {
    (1..=MAX_ID_CHARS).contains(&id.chars().count())
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn is_repo_part(part: &str) -> bool {
    !part.is_empty()
        && !part.chars().all(|c| c == '.')
        && part
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

pub fn is_repo_full_name(value: &str) -> bool {
    value
        .split_once('/')
        .is_some_and(|(owner, name)| is_repo_part(owner) && is_repo_part(name))
}

fn valid_repo(repo: Option<String>) -> Option<String> {
    repo.filter(|value| is_repo_full_name(value))
}

fn normalize_node(node: ArchNode) -> Option<ArchNode> {
    if !is_valid_id(&node.id) {
        return None;
    }
    let label = clean_label(&node.label);
    if label.is_empty() {
        return None;
    }
    Some(ArchNode {
        label,
        repo: valid_repo(node.repo),
        ..node
    })
}

fn normalize(graph: ArchGraph) -> Option<ArchGraph> {
    if graph.nodes.is_empty()
        || graph.nodes.len() > MAX_ARCH_NODES
        || graph.edges.len() > MAX_ARCH_EDGES
    {
        return None;
    }
    let nodes = graph
        .nodes
        .into_iter()
        .map(normalize_node)
        .collect::<Option<Vec<_>>>()?;
    let mut ids = HashSet::new();
    if !nodes.iter().all(|node| ids.insert(node.id.as_str())) {
        return None;
    }
    let edges = graph
        .edges
        .into_iter()
        .filter(|edge| ids.contains(edge.from.as_str()) && ids.contains(edge.to.as_str()))
        .map(|edge| ArchEdge {
            label: clean_label(&edge.label),
            ..edge
        })
        .collect();
    Some(ArchGraph { nodes, edges })
}

pub fn parse_arch(message: &str, nonce: &str) -> Option<ArchGraph> {
    let info = format!("{ARCH_INFO_PREFIX}{nonce}");
    let block = last_fenced_block(message, &info)?;
    let graph: ArchGraph = serde_json::from_str(block.trim()).ok()?;
    normalize(graph)
}

pub fn parse_touched(message: &str, nonce: &str) -> Vec<String> {
    let info = format!("{TOUCHED_INFO_PREFIX}{nonce}");
    let Some(block) = last_fenced_block(message, &info) else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    block
        .lines()
        .map(str::trim)
        .filter(|line| is_valid_id(line))
        .filter(|line| seen.insert(*line))
        .take(MAX_ARCH_NODES)
        .map(str::to_owned)
        .collect()
}

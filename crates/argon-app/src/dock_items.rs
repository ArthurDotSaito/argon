use gpui_kit::Entity;
use gpui_kit::component::dock::{DockArea, DockPlacement, NodeId, PaneRef, PaneTree, PanelId};

use crate::center::CenterPanel;

const ITEM_PLACEMENTS: [DockPlacement; 4] = [
    DockPlacement::Center,
    DockPlacement::Left,
    DockPlacement::Right,
    DockPlacement::Bottom,
];

pub fn locate(area: &DockArea, panel: PanelId) -> Option<(DockPlacement, NodeId)> {
    ITEM_PLACEMENTS.into_iter().find_map(|placement| {
        let node = area.layout(placement)?.find_panel_node(panel)?;
        Some((placement, node))
    })
}

pub fn is_visible(area: &DockArea, panel: PanelId) -> bool {
    locate(area, panel).is_some_and(|(placement, _)| {
        placement == DockPlacement::Center || area.is_dock_open(placement)
    })
}

pub fn center_panels(area: &DockArea) -> Vec<Entity<CenterPanel>> {
    ITEM_PLACEMENTS
        .iter()
        .filter_map(|placement| area.layout(*placement))
        .flat_map(PaneTree::panels)
        .filter_map(|id| area.panel(id))
        .filter_map(|panel| panel.view().downcast::<CenterPanel>().ok())
        .collect()
}

pub fn active_in_group_of(area: &DockArea, panel: PanelId) -> Option<PanelId> {
    let (placement, node) = locate(area, panel)?;
    match area.layout(placement)?.find_node(node)?.kind() {
        PaneRef::Tabs { panels, active_ix } => panels.get(active_ix).copied(),
        PaneRef::Split { .. } => None,
    }
}

pub fn group_panels(area: &DockArea, node: NodeId) -> Vec<PanelId> {
    ITEM_PLACEMENTS
        .into_iter()
        .filter_map(|placement| area.layout(placement)?.find_node(node))
        .find_map(|found| match found.kind() {
            PaneRef::Tabs { panels, .. } => Some(panels.to_vec()),
            PaneRef::Split { .. } => None,
        })
        .unwrap_or_default()
}

pub fn first_center_active(area: &DockArea) -> Option<PanelId> {
    let tree = area.layout(DockPlacement::Center)?;
    let mut active = None;
    tree.root().walk(&mut |node| {
        if let (None, PaneRef::Tabs { panels, active_ix }) = (active, node.kind()) {
            active = panels.get(active_ix).copied();
        }
    });
    active
}

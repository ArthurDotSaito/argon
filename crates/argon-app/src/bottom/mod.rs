mod labels;
mod log_lines;
mod log_model;
mod log_panel;
mod log_view;
mod run_status;

use gpui_kit::component::dock::{DockArea, DockPlacement, PanelId};
use gpui_kit::{App, AppContext as _, Entity, KeyBinding};

pub use log_lines::clean_line;
pub use log_model::{JobLogEvent, JobLogHandle, JobLogModel, LogTarget};
pub use log_panel::LogPanel;
pub use run_status::{duration_label, is_running, run_status_icon};

use log_panel::{FocusLogSearch, LOG_CONTEXT, NextLogMatch, PreviousLogMatch};

const PLACEMENTS: [DockPlacement; 4] = [
    DockPlacement::Bottom,
    DockPlacement::Left,
    DockPlacement::Right,
    DockPlacement::Center,
];

pub fn init(cx: &mut App) -> Entity<JobLogModel> {
    if let Some(model) = JobLogModel::global(cx) {
        return model;
    }
    cx.bind_keys([
        KeyBinding::new("ctrl-f", FocusLogSearch, Some(LOG_CONTEXT)),
        KeyBinding::new("f3", NextLogMatch, Some(LOG_CONTEXT)),
        KeyBinding::new("shift-f3", PreviousLogMatch, Some(LOG_CONTEXT)),
    ]);
    let model = cx.new(|_| JobLogModel::new());
    cx.set_global(JobLogHandle(model.clone()));
    model
}

pub fn locate_log_panel(area: &DockArea) -> Option<(DockPlacement, PanelId)> {
    PLACEMENTS.into_iter().find_map(|placement| {
        let tree = area.layout(placement)?;
        tree.panels().into_iter().find_map(|id| {
            let panel = area.panel(id)?;
            panel
                .view()
                .downcast::<LogPanel>()
                .ok()
                .map(|_| (placement, id))
        })
    })
}

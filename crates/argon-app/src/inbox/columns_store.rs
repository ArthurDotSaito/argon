use std::fs;
use std::io;
use std::time::Duration;

use argon_core::Paths;
use gpui_kit::{App, AppContext as _, Task};

use super::columns::ColumnWidths;
use crate::layout_store::write_atomic;

const SAVE_DEBOUNCE: Duration = Duration::from_millis(300);

pub fn load() -> ColumnWidths {
    let path = Paths::resolve().inbox_columns_file();
    let Ok(content) = fs::read_to_string(&path) else {
        return ColumnWidths::default();
    };
    match serde_json::from_str::<ColumnWidths>(&content) {
        Ok(widths) => widths.clamped(),
        Err(error) => {
            tracing::warn!(
                "ignoring invalid inbox columns at {}: {error}",
                path.display()
            );
            ColumnWidths::default()
        }
    }
}

#[derive(Default)]
pub struct ColumnsSaver {
    pending: Option<Task<()>>,
    latest: Option<ColumnWidths>,
}

impl ColumnsSaver {
    pub fn schedule(&mut self, widths: ColumnWidths, cx: &mut App) {
        self.latest = Some(widths);
        self.pending = Some(cx.spawn(async move |cx| {
            cx.background_executor().timer(SAVE_DEBOUNCE).await;
            let saved = cx.background_spawn(async move { save(&widths) }).await;
            report_save(saved);
        }));
    }
}

impl Drop for ColumnsSaver {
    fn drop(&mut self) {
        self.pending = None;
        if let Some(widths) = self.latest.take() {
            report_save(save(&widths));
        }
    }
}

fn report_save(result: io::Result<()>) {
    if let Err(error) = result {
        tracing::warn!("failed to save inbox columns: {error}");
    }
}

fn save(widths: &ColumnWidths) -> io::Result<()> {
    let content = serde_json::to_string_pretty(widths).map_err(io::Error::other)?;
    write_atomic(&Paths::resolve().inbox_columns_file(), &content)
}

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use gpui_kit::component::dock::{DockArea, DockAreaState, DockPlacement};
use gpui_kit::{App, AppContext as _, Entity, Task, WeakEntity, Window};

pub const LAYOUT_VERSION: usize = 1;

const APP_DIR: &str = "Argon";
const LAYOUT_FILE: &str = "layout.json";
const SAVE_DEBOUNCE: Duration = Duration::from_millis(300);

static STAGING_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Default)]
pub struct LayoutSaver {
    pending: Option<Task<()>>,
}

impl LayoutSaver {
    pub fn schedule(&mut self, area: WeakEntity<DockArea>, cx: &mut App) {
        self.pending = Some(cx.spawn(async move |cx| {
            cx.background_executor().timer(SAVE_DEBOUNCE).await;
            let Ok(state) = area.read_with(cx, |area, cx| area.dump(cx)) else {
                return;
            };
            let saved = cx.background_spawn(async move { save(&state) }).await;
            report_save(saved);
        }));
    }

    pub fn save_now(&mut self, area: &Entity<DockArea>, cx: &App) {
        self.pending = None;
        report_save(save(&area.read(cx).dump(cx)));
    }
}

pub fn restore(
    area: &Entity<DockArea>,
    required_docks: &[DockPlacement],
    window: &mut Window,
    cx: &mut App,
) -> bool {
    let Some(state) = load() else {
        return false;
    };
    if state.version != Some(LAYOUT_VERSION) {
        return false;
    }
    if let Err(error) = area.update(cx, |area, cx| area.load(state, window, cx)) {
        eprintln!("failed to restore layout: {error:#}");
        return false;
    }
    let area = area.read(cx);
    required_docks.iter().all(|dock| area.has_dock(*dock))
}

fn layout_path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|dir| dir.join(APP_DIR).join(LAYOUT_FILE))
}

fn load() -> Option<DockAreaState> {
    let path = layout_path()?;
    let content = fs::read_to_string(&path).ok()?;
    match serde_json::from_str(&content) {
        Ok(state) => Some(state),
        Err(error) => {
            eprintln!("ignoring invalid layout at {}: {error}", path.display());
            None
        }
    }
}

fn save(state: &DockAreaState) -> io::Result<()> {
    let path = layout_path()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no local data directory"))?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let content = serde_json::to_string_pretty(state).map_err(io::Error::other)?;
    let staging = staging_path(&path);
    fs::write(&staging, content)?;
    fs::rename(&staging, &path).inspect_err(|_| {
        _ = fs::remove_file(&staging);
    })
}

fn staging_path(path: &Path) -> PathBuf {
    let sequence = STAGING_COUNTER.fetch_add(1, Ordering::Relaxed);
    path.with_extension(format!("json.{}-{sequence}.tmp", std::process::id()))
}

fn report_save(result: io::Result<()>) {
    if let Err(error) = result {
        eprintln!("failed to save layout: {error}");
    }
}

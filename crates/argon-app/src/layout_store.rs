use std::fs;
use std::io;
use std::path::PathBuf;

use gpui_kit::component::dock::DockAreaState;

const APP_DIR: &str = "Argon";
const LAYOUT_FILE: &str = "layout.json";

fn layout_path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|dir| dir.join(APP_DIR).join(LAYOUT_FILE))
}

pub fn load() -> Option<DockAreaState> {
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

pub fn save(state: &DockAreaState) -> io::Result<()> {
    let path = layout_path()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no local data directory"))?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let content = serde_json::to_string_pretty(state).map_err(io::Error::other)?;
    let staging = path.with_extension("json.tmp");
    fs::write(&staging, content)?;
    fs::rename(&staging, &path)
}

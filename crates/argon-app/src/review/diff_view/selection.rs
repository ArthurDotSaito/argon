use gpui_kit::App;

use super::DiffView;

impl DiffView {
    pub fn selected_line(&self, cx: &App) -> Option<(String, u32)> {
        let range = self.state.read(cx).selected_lines()?;
        let line = u32::try_from(range.end()).ok()?;
        Some((range.path().to_string(), line))
    }
}

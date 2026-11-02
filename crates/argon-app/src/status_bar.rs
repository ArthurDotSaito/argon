use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Selectable as _, Sizable as _, h_flex,
};
use gpui_kit::{
    App, IntoElement, ParentElement as _, RenderOnce, SharedString, Styled as _, WeakEntity,
    Window, div, px,
};

use crate::workspace::{DockPosition, Workspace};

const STATUS_BAR_HEIGHT: f32 = 24.;
const VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));

#[derive(IntoElement)]
pub struct WorkspaceStatusBar {
    workspace: WeakEntity<Workspace>,
    docks: Vec<(DockPosition, bool)>,
    needs_you: usize,
    running_agents: usize,
}

impl WorkspaceStatusBar {
    pub fn new(
        workspace: WeakEntity<Workspace>,
        docks: Vec<(DockPosition, bool)>,
        needs_you: usize,
        running_agents: usize,
    ) -> Self {
        Self {
            workspace,
            docks,
            needs_you,
            running_agents,
        }
    }

    fn dock_toggle(&self, dock: DockPosition, open: bool) -> Button {
        let workspace = self.workspace.clone();
        let (id, icon, tooltip) = match dock {
            DockPosition::Left => ("toggle-left-dock", IconName::PanelLeft, "Visões (Ctrl+B)"),
            DockPosition::Bottom => ("toggle-bottom-dock", IconName::PanelBottom, "Log (Ctrl+J)"),
            DockPosition::Right => (
                "toggle-right-dock",
                IconName::PanelRight,
                "Agentes (Ctrl+Alt+B)",
            ),
        };
        Button::new(id)
            .icon(icon)
            .xsmall()
            .ghost()
            .selected(open)
            .tab_stop(false)
            .tooltip(tooltip)
            .on_click(move |_, window, cx| {
                _ = workspace.update(cx, |workspace, cx| workspace.toggle_dock(dock, window, cx));
            })
    }
}

fn status_item(icon: IconName, label: SharedString) -> impl IntoElement {
    h_flex()
        .gap_1()
        .items_center()
        .child(Icon::new(icon).xsmall())
        .child(label)
}

impl RenderOnce for WorkspaceStatusBar {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let toggles = h_flex().gap_0p5().children(
            self.docks
                .iter()
                .map(|(dock, open)| self.dock_toggle(*dock, *open)),
        );
        let agents_label = match self.running_agents {
            1 => "1 agente rodando".to_string(),
            count => format!("{count} agentes rodando"),
        };

        StatusBar::new()
            .h(px(STATUS_BAR_HEIGHT))
            .py_0()
            .left(toggles)
            .child(div().text_color(cx.theme().primary).child(status_item(
                IconName::Inbox,
                format!("{} needs you", self.needs_you).into(),
            )))
            .child(status_item(IconName::Bot, agents_label.into()))
            .right(div().child(VERSION))
    }
}

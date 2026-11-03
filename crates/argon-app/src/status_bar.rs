use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Selectable as _, Sizable as _, h_flex,
};
use gpui_kit::{
    App, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce, SharedString,
    StatefulInteractiveElement as _, Styled as _, WeakEntity, Window, div, px,
};

use crate::agents::{Activity, activity_label};
use crate::inbox::{InboxView, needs_you_status};
use crate::workspace::{DockPosition, Workspace};

const STATUS_BAR_HEIGHT: f32 = 24.;
const VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));

#[derive(IntoElement)]
pub struct WorkspaceStatusBar {
    workspace: WeakEntity<Workspace>,
    docks: Vec<(DockPosition, bool)>,
    needs_you: usize,
    agents: Activity,
    update_label: Option<String>,
}

impl WorkspaceStatusBar {
    pub fn new(
        workspace: WeakEntity<Workspace>,
        docks: Vec<(DockPosition, bool)>,
        needs_you: usize,
        agents: Activity,
        update_label: Option<String>,
    ) -> Self {
        Self {
            workspace,
            docks,
            needs_you,
            agents,
            update_label,
        }
    }

    fn update_button(&self, label: String) -> Button {
        let workspace = self.workspace.clone();
        Button::new("status-update")
            .icon(Icon::new(IconName::ArrowUp))
            .label(label)
            .xsmall()
            .primary()
            .tab_stop(false)
            .on_click(move |_, window, cx| {
                _ = workspace.update(cx, |workspace, cx| workspace.show_about(window, cx));
            })
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
        let workspace = self.workspace.clone();
        let needs_you = div()
            .id("status-needs-you")
            .cursor_pointer()
            .text_color(cx.theme().primary)
            .hover(|style| style.text_color(cx.theme().foreground))
            .child(status_item(
                IconName::Inbox,
                needs_you_status(self.needs_you).into(),
            ))
            .on_click(move |_, _, cx| {
                _ = workspace.update(cx, |workspace, cx| {
                    workspace.show_view(InboxView::NeedsYou, cx)
                });
            });

        let active = self.agents.running + self.agents.awaiting > 0;
        let agents_workspace = self.workspace.clone();
        let agents = active.then(|| {
            let color = if self.agents.awaiting > 0 {
                cx.theme().warning
            } else {
                cx.theme().muted_foreground
            };
            div()
                .id("status-agents")
                .cursor_pointer()
                .text_color(color)
                .hover(|style| style.text_color(cx.theme().foreground))
                .child(status_item(
                    IconName::Bot,
                    activity_label(self.agents.running, self.agents.awaiting).into(),
                ))
                .on_click(move |_, window, cx| {
                    _ = agents_workspace
                        .update(cx, |workspace, cx| workspace.reveal_agents(window, cx));
                })
        });

        let version = match self.update_label.clone() {
            Some(label) => self.update_button(label).into_any_element(),
            None => div().child(VERSION).into_any_element(),
        };

        StatusBar::new()
            .h(px(STATUS_BAR_HEIGHT))
            .py_0()
            .left(toggles)
            .child(h_flex().gap_3().child(needs_you).children(agents))
            .right(version)
    }
}

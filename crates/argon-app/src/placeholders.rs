use gpui_kit::component::dock::{BasePanel, Panel, PanelControl, PanelEvent};
use gpui_kit::component::{ActiveTheme as _, StyledExt as _, v_flex};
use gpui_kit::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, Styled as _, Subscription, Window, div,
};

use crate::agents::AgentSessions;
use crate::inbox;

pub struct CenterPlaceholder {
    title: SharedString,
    description: SharedString,
    focus_handle: FocusHandle,
}

impl CenterPlaceholder {
    pub fn new(
        title: impl Into<SharedString>,
        description: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            title: title.into(),
            description: description.into(),
            focus_handle: cx.focus_handle(),
        }
    }
}

impl Focusable for CenterPlaceholder {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for CenterPlaceholder {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        placeholder_body(
            &self.focus_handle,
            self.title.clone(),
            self.description.clone(),
            cx,
        )
    }
}

fn placeholder_body(
    focus_handle: &FocusHandle,
    title: SharedString,
    description: SharedString,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    v_flex()
        .track_focus(focus_handle)
        .size_full()
        .items_center()
        .justify_center()
        .gap_1()
        .child(
            div()
                .text_lg()
                .font_semibold()
                .text_color(theme.foreground)
                .child(title),
        )
        .child(div().text_color(theme.muted_foreground).child(description))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockSlot {
    Views,
    Agents,
    Log,
}

impl DockSlot {
    pub const ALL: [Self; 3] = [Self::Views, Self::Agents, Self::Log];

    pub fn panel_name(self) -> &'static str {
        match self {
            Self::Views => inbox::VIEWS_PANEL,
            Self::Agents => "argon.agents",
            Self::Log => "argon.log",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Views => "Visões",
            Self::Agents => "Agentes",
            Self::Log => "Log",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::Views => "Inbox, Needs you, Your PRs…",
            Self::Agents => "Nenhuma sessão de agente.",
            Self::Log => "Nada por aqui ainda.",
        }
    }
}

pub struct DockPlaceholder {
    slot: DockSlot,
    focus_handle: FocusHandle,
    sessions: Option<Entity<AgentSessions>>,
    _observation: Option<Subscription>,
}

impl DockPlaceholder {
    pub fn new(slot: DockSlot, cx: &mut Context<Self>) -> Self {
        let sessions = (slot == DockSlot::Agents)
            .then(|| AgentSessions::global(cx))
            .flatten();
        let observation = sessions
            .as_ref()
            .map(|sessions| cx.observe(sessions, |_, _, cx| cx.notify()));
        Self {
            slot,
            focus_handle: cx.focus_handle(),
            sessions,
            _observation: observation,
        }
    }

    fn lines(&self, cx: &App) -> Vec<SharedString> {
        self.sessions
            .as_ref()
            .map(|sessions| {
                sessions
                    .read(cx)
                    .sessions()
                    .iter()
                    .map(|session| session.summary().into())
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl EventEmitter<PanelEvent> for DockPlaceholder {}

impl Focusable for DockPlaceholder {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BasePanel for DockPlaceholder {
    fn panel_name(&self) -> &'static str {
        self.slot.panel_name()
    }

    fn closable(&self, _: &App) -> bool {
        false
    }

    fn zoomable(&self, _: &App) -> bool {
        false
    }
}

impl Panel for DockPlaceholder {
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.slot.title()
    }

    fn zoom_control(&self, _: &App) -> Option<PanelControl> {
        None
    }
}

impl Render for DockPlaceholder {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let lines = self.lines(cx);
        let theme = cx.theme();
        let body = v_flex()
            .track_focus(&self.focus_handle)
            .size_full()
            .p_3()
            .gap_1();
        if lines.is_empty() {
            return body
                .text_color(theme.muted_foreground)
                .child(self.slot.hint());
        }
        body.text_sm()
            .text_color(theme.foreground)
            .children(lines.into_iter().map(|line| div().child(line)))
    }
}

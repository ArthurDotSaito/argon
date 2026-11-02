use std::rc::Rc;

use gpui_kit::component::dock::{BasePanel, Panel, PanelEvent, PanelInfo, PanelState};
use gpui_kit::{
    AnyView, App, ClickEvent, Context, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _,
};
use serde::{Deserialize, Serialize};

pub const CENTER_PANEL: &str = "argon.center";

pub type CenterContentFactory = Rc<dyn Fn(&CenterItem, &mut Window, &mut App) -> AnyView>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CenterItem {
    Inbox,
    Pr(String),
    Settings,
}

impl CenterItem {
    pub fn title(&self) -> SharedString {
        match self {
            Self::Inbox => "Inbox".into(),
            Self::Pr(id) => match id.rsplit_once('#') {
                Some((_, number)) => format!("#{number}").into(),
                None => id.clone().into(),
            },
            Self::Settings => "Configurações".into(),
        }
    }
}

pub enum CenterPanelEvent {
    Pinned,
}

#[derive(Serialize, Deserialize)]
struct CenterPanelState {
    item: CenterItem,
    preview: bool,
}

pub struct CenterPanel {
    item: CenterItem,
    preview: bool,
    content: AnyView,
    content_factory: CenterContentFactory,
    focus_handle: FocusHandle,
}

impl CenterPanel {
    pub fn new(
        item: CenterItem,
        preview: bool,
        content_factory: CenterContentFactory,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let content = content_factory(&item, window, cx);
        Self {
            item,
            preview,
            content,
            content_factory,
            focus_handle: cx.focus_handle(),
        }
    }

    pub fn restore(
        info: &PanelInfo,
        content_factory: CenterContentFactory,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let state = match info {
            PanelInfo::Panel(value) => {
                serde_json::from_value::<CenterPanelState>(value.clone()).ok()
            }
            _ => None,
        };
        let (item, preview) = state
            .map(|state| (state.item, state.preview))
            .unwrap_or((CenterItem::Inbox, false));
        Self::new(item, preview, content_factory, window, cx)
    }

    pub fn item(&self) -> &CenterItem {
        &self.item
    }

    pub fn is_preview(&self) -> bool {
        self.preview
    }

    pub fn pin(&mut self, cx: &mut Context<Self>) {
        if self.preview {
            self.preview = false;
            cx.emit(CenterPanelEvent::Pinned);
            cx.notify();
        }
    }

    pub fn replace(&mut self, item: CenterItem, window: &mut Window, cx: &mut Context<Self>) {
        self.content = (self.content_factory)(&item, window, cx);
        self.item = item;
        cx.notify();
    }
}

impl EventEmitter<PanelEvent> for CenterPanel {}

impl EventEmitter<CenterPanelEvent> for CenterPanel {}

impl Focusable for CenterPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BasePanel for CenterPanel {
    fn panel_name(&self) -> &'static str {
        CENTER_PANEL
    }

    fn dump(&self, _: &App) -> PanelState {
        let state = CenterPanelState {
            item: self.item.clone(),
            preview: self.preview,
        };
        let mut dumped = PanelState::new(CENTER_PANEL);
        dumped.info = PanelInfo::panel(serde_json::to_value(state).unwrap_or_default());
        dumped
    }
}

impl Panel for CenterPanel {
    fn title(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id(("center-tab-title", cx.entity_id()))
            .when(self.preview, |this| this.italic())
            .child(self.item.title())
            .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                if event.click_count() >= 2 {
                    this.pin(cx);
                    window.refresh();
                }
            }))
    }
}

impl Render for CenterPanel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus_handle)
            .size_full()
            .child(self.content.clone())
    }
}

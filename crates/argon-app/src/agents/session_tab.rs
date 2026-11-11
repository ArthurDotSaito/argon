use chrono::Utc;
use gpui_kit::component::{ActiveTheme as _, v_flex};
use gpui_kit::{
    App, AppContext as _, Context, Div, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, Styled as _, Subscription, Window, div,
};

use super::catalog::SessionCatalog;
use super::panel_labels as labels;
use super::thread_view::ThreadView;
use crate::center::CenterItem;
use crate::workspace::Workspace;

pub fn open_session_tab(session_id: &str, window: &mut Window, cx: &mut App) {
    if let Some(workspace) = Workspace::global(cx) {
        let item = CenterItem::AgentSession(session_id.to_owned());
        _ = workspace.update(cx, |workspace, cx| {
            workspace.open_item(item, false, window, cx)
        });
    }
}

pub fn session_tab_title(session_id: &str, cx: &App) -> SharedString {
    let card = SessionCatalog::global(cx).and_then(|catalog| {
        catalog
            .read(cx)
            .cards(Utc::now(), cx)
            .into_iter()
            .find(|card| card.id == session_id)
    });
    match card {
        Some(card) => match card.number {
            Some(number) => format!("#{number} · {}", card.agent_label).into(),
            None => card.agent_label.into(),
        },
        None => labels::SESSION_TAB_FALLBACK.into(),
    }
}

pub struct SessionTab {
    session_id: String,
    catalog: Option<Entity<SessionCatalog>>,
    view: Option<Entity<ThreadView>>,
    focus_handle: FocusHandle,
    _subscription: Option<Subscription>,
}

impl SessionTab {
    pub fn new(session_id: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let catalog = SessionCatalog::global(cx);
        let subscription = catalog.as_ref().map(|catalog| {
            cx.observe_in(catalog, window, |this, _, window, cx| {
                this.resolve(window, cx)
            })
        });
        let mut this = Self {
            session_id,
            catalog,
            view: None,
            focus_handle: cx.focus_handle(),
            _subscription: subscription,
        };
        this.resolve(window, cx);
        this
    }

    fn resolve(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.view.is_some() {
            return;
        }
        let Some(catalog) = self.catalog.clone() else {
            return;
        };
        let session_id = self.session_id.clone();
        let Some(model) = catalog.update(cx, |catalog, cx| catalog.thread(&session_id, cx)) else {
            return;
        };
        self.view = Some(cx.new(|cx| ThreadView::new(model, window, cx)));
        cx.notify();
    }

    fn placeholder(&self, cx: &App) -> Div {
        let loaded = self
            .catalog
            .as_ref()
            .is_some_and(|catalog| catalog.read(cx).is_loaded());
        let text = if loaded {
            labels::SESSION_MISSING
        } else {
            labels::LOADING_TRANSCRIPT
        };
        div()
            .p_3()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(text)
    }
}

impl Focusable for SessionTab {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for SessionTab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match &self.view {
            Some(view) => view.clone().into_any_element(),
            None => self.placeholder(cx).into_any_element(),
        };
        v_flex()
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(cx.theme().background)
            .child(body)
    }
}

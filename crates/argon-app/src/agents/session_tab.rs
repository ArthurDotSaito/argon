use gpui_kit::component::{ActiveTheme as _, v_flex};
use gpui_kit::{
    App, AppContext as _, Context, Div, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, Styled as _, Subscription, Window, div,
};

use super::catalog::SessionCatalog;
use super::panel_labels as labels;
use super::thread_model::ThreadModel;
use super::thread_view::ThreadView;
use crate::workspace::{SessionPlacement, Workspace};

pub fn open_session_tab(session_id: &str, window: &mut Window, cx: &mut App) {
    if let Some(workspace) = Workspace::global(cx) {
        _ = workspace.update(cx, |workspace, cx| {
            workspace.open_session(session_id.to_owned(), SessionPlacement::Center, window, cx)
        });
    }
}

pub fn session_tab_title(session_id: &str, cx: &App) -> SharedString {
    SessionCatalog::global(cx)
        .and_then(|catalog| catalog.read(cx).tab_title(session_id, cx))
        .unwrap_or_else(|| labels::SESSION_TAB_FALLBACK.into())
}

pub struct SessionTab {
    session_id: String,
    catalog: Option<Entity<SessionCatalog>>,
    view: Option<(Entity<ThreadModel>, Entity<ThreadView>)>,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl SessionTab {
    pub fn new(session_id: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let catalog = SessionCatalog::global(cx);
        let focus_handle = cx.focus_handle();
        let mut subscriptions = vec![cx.on_focus(&focus_handle, window, |this, window, cx| {
            this.focus_composer(window, cx)
        })];
        subscriptions.extend(catalog.as_ref().map(|catalog| {
            cx.observe_in(catalog, window, |this, _, window, cx| {
                this.resolve(window, cx)
            })
        }));
        let mut this = Self {
            session_id,
            catalog,
            view: None,
            focus_handle,
            _subscriptions: subscriptions,
        };
        this.resolve(window, cx);
        this
    }

    fn resolve(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(catalog) = self.catalog.clone() else {
            return;
        };
        let session_id = self.session_id.clone();
        let Some(model) = catalog.update(cx, |catalog, cx| catalog.thread(&session_id, cx)) else {
            return;
        };
        if self
            .view
            .as_ref()
            .is_some_and(|(current, _)| *current == model)
        {
            return;
        }
        let had_focus = self.focus_handle.contains_focused(window, cx);
        let view = cx.new(|cx| ThreadView::new(model.clone(), window, cx));
        self.view = Some((model, view));
        if had_focus {
            match self.composer_focus_handle(cx) {
                Some(handle) => handle.focus(window, cx),
                None => self.focus_handle.focus(window, cx),
            }
        }
        cx.notify();
    }

    fn focus_composer(&self, window: &mut Window, cx: &mut App) {
        if let Some(handle) = self.composer_focus_handle(cx) {
            handle.focus(window, cx);
        }
    }

    fn composer_focus_handle(&self, cx: &App) -> Option<FocusHandle> {
        self.view
            .as_ref()
            .and_then(|(_, view)| view.read(cx).composer_focus_handle(cx))
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
            Some((_, view)) => view.clone().into_any_element(),
            None => self.placeholder(cx).into_any_element(),
        };
        v_flex()
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(cx.theme().background)
            .child(body)
    }
}

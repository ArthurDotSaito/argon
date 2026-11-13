use gpui_kit::{AppContext as _, Context, Entity, FocusHandle, Subscription, Window};

use super::Workspace;
use crate::actions::OpenPalette;
use crate::center::CenterItem;
use crate::palette::{Palette, PaletteEvent};

pub(super) struct PaletteSlot {
    pub(super) view: Entity<Palette>,
    previous_focus: Option<FocusHandle>,
    _subscription: Subscription,
}

impl Workspace {
    pub(super) fn toggle_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.is_some() {
            self.close_palette(window, cx);
            return;
        }
        let previous_focus = window.focused(cx);
        let view = cx.new(|cx| Palette::new(&self.inbox, window, cx));
        let subscription = cx.subscribe_in(&view, window, Self::on_palette_event);
        self.palette = Some(PaletteSlot {
            view,
            previous_focus,
            _subscription: subscription,
        });
        cx.notify();
    }

    pub(super) fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(slot) = self.palette.take() else {
            return;
        };
        if slot.view.read(cx).contains_focus(window, cx) {
            match slot.previous_focus {
                Some(handle) => handle.focus(window, cx),
                None => self.focus_handle.focus(window, cx),
            }
        }
        cx.notify();
    }

    pub(super) fn on_palette_event(
        &mut self,
        _: &Entity<Palette>,
        event: &PaletteEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_palette(window, cx);
        match event {
            PaletteEvent::OpenPr(key) => {
                self.open_item(CenterItem::Pr(key.clone()), false, window, cx);
            }
            PaletteEvent::OpenIssue(key) => {
                self.open_item(CenterItem::Issue(key.clone()), false, window, cx);
            }
            PaletteEvent::Dismiss => {}
        }
    }

    pub(super) fn on_open_palette(
        &mut self,
        _: &OpenPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_palette(window, cx);
    }
}

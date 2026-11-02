use std::rc::Rc;

use gpui_kit::{AppContext as _, Entity};

use crate::center::{CenterContent, CenterContentFactory, CenterItem};
use crate::inbox::{InboxModel, InboxPane};
use crate::placeholders::{CenterPlaceholder, PrPlaceholder};

const SETTINGS_HINT: &str = "Preferências do Argon.";

pub fn content_factory(inbox: Entity<InboxModel>) -> CenterContentFactory {
    Rc::new(move |item, window, cx| match item {
        CenterItem::Inbox => {
            let pane = cx.new(|cx| InboxPane::new(inbox.clone(), window, cx));
            CenterContent::new(pane, cx)
        }
        CenterItem::Pr(id) => {
            let view = cx.new(|cx| PrPlaceholder::new(id.clone(), inbox.clone(), cx));
            CenterContent::new(view, cx)
        }
        CenterItem::Settings => {
            let title = item.title(cx);
            let view = cx.new(|cx| CenterPlaceholder::new(title, SETTINGS_HINT, cx));
            CenterContent::new(view, cx)
        }
    })
}

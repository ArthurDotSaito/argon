use std::rc::Rc;

use gpui_kit::{AppContext as _, Entity};

use crate::agents::{AgentsView, SessionTab};
use crate::center::{CenterContent, CenterContentFactory, CenterItem};
use crate::inbox::{InboxModel, InboxPane};
use crate::issue::IssueView;
use crate::pr::PrView;
use crate::project::ProjectView;
use crate::settings::{self, SettingsView};

pub fn content_factory(inbox: Entity<InboxModel>) -> CenterContentFactory {
    Rc::new(move |item, origin, window, cx| match item {
        CenterItem::Inbox => {
            let pane = cx.new(|cx| InboxPane::new(inbox.clone(), window, cx));
            CenterContent::new(pane, cx)
        }
        CenterItem::Pr(key) => {
            let view = cx.new(|cx| PrView::new(key.clone(), inbox.clone(), window, cx));
            settings::note_pr_opened(key.clone(), &view, cx);
            CenterContent::new(view, cx)
        }
        CenterItem::Issue(key) => {
            let view = cx.new(|cx| IssueView::new(key.clone(), inbox.clone(), origin, window, cx));
            CenterContent::new(view, cx)
        }
        CenterItem::Agents => {
            let view = cx.new(|cx| AgentsView::new(window, cx));
            CenterContent::new(view, cx)
        }
        CenterItem::AgentSession(id) => {
            let view = cx.new(|cx| SessionTab::new(id.clone(), window, cx));
            CenterContent::new(view, cx)
        }
        CenterItem::Project(key) => {
            let view = cx.new(|cx| ProjectView::new(key.clone(), window, cx));
            CenterContent::new(view, cx)
        }
        CenterItem::Settings => {
            let view = cx.new(|cx| SettingsView::new(window, cx));
            CenterContent::new(view, cx)
        }
    })
}

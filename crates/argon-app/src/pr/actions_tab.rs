use std::rc::Rc;

use argon_github::PrDetail;
use gpui_kit::assets::IconName;
use gpui_kit::{Context, IntoElement, Render, Window};

use super::labels;
use super::placeholder::tab_placeholder;

pub struct ActionsTab {
    detail: Option<Rc<PrDetail>>,
}

impl ActionsTab {
    pub fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self { detail: None }
    }

    pub fn set_detail(&mut self, detail: Rc<PrDetail>, cx: &mut Context<Self>) {
        self.detail = Some(detail);
        cx.notify();
    }

    pub fn count(&self) -> Option<usize> {
        None
    }
}

impl Render for ActionsTab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let hint = self
            .detail
            .as_ref()
            .map(|detail| labels::actions_soon(&detail.head_oid))
            .unwrap_or_else(|| labels::ACTIONS_SOON.to_string());
        tab_placeholder(IconName::Play, labels::UNDER_CONSTRUCTION, &hint, cx)
    }
}

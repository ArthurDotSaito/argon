use std::rc::Rc;

use argon_github::PrDetail;
use gpui_kit::assets::IconName;
use gpui_kit::{Context, IntoElement, Render, Window};

use super::labels;
use super::placeholder::tab_placeholder;

pub struct ReviewTab {
    detail: Option<Rc<PrDetail>>,
}

impl ReviewTab {
    pub fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self { detail: None }
    }

    pub fn set_detail(&mut self, detail: Rc<PrDetail>, cx: &mut Context<Self>) {
        self.detail = Some(detail);
        cx.notify();
    }

    pub fn count(&self) -> Option<usize> {
        self.detail
            .as_ref()
            .map(|detail| detail.pr.changed_files as usize)
    }
}

impl Render for ReviewTab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        tab_placeholder(
            IconName::FileDiff,
            labels::UNDER_CONSTRUCTION,
            labels::REVIEW_SOON,
            cx,
        )
    }
}

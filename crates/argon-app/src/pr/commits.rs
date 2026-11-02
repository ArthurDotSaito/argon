use std::rc::Rc;

use argon_github::PrDetail;
use gpui_kit::assets::IconName;
use gpui_kit::{Context, IntoElement, Render, Window};

use super::labels;
use super::placeholder::tab_placeholder;

pub struct CommitsTab {
    detail: Option<Rc<PrDetail>>,
}

impl CommitsTab {
    pub fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self { detail: None }
    }

    pub fn set_detail(&mut self, detail: Rc<PrDetail>, cx: &mut Context<Self>) {
        self.detail = Some(detail);
        cx.notify();
    }

    pub fn count(&self) -> Option<usize> {
        self.detail.as_ref().map(|detail| detail.commits.len())
    }
}

impl Render for CommitsTab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        tab_placeholder(
            IconName::GitCommitHorizontal,
            labels::UNDER_CONSTRUCTION,
            labels::COMMITS_SOON,
            cx,
        )
    }
}

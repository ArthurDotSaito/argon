use std::rc::Rc;

use argon_github::{PrDetail, PrFile};
use gpui_kit::assets::IconName;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    App, ClickEvent, Context, Div, EventEmitter, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, ScrollHandle, SharedString, StatefulInteractiveElement as _,
    Styled as _, Task, Window, div, point, px,
};

use super::commit_files::commit_files_list;
use super::conversation::CONTENT_MAX_WIDTH;
use super::labels::{self, short_sha};
use super::placeholder::tab_placeholder;
use crate::services::AppServices;

const SEPARATOR: &str = "·";
const SHOW_ALL: &str = "Ver todos";
const LOADING_FILES: &str = "Carregando arquivos do commit…";
const NO_FILES: &str = "Este commit não alterou arquivos.";

fn commit_banner(oid: &str, headline: &str) -> String {
    format!("Mostrando só o commit {} — {headline}", short_sha(oid))
}

fn files_failed(error: &str) -> String {
    format!("Falha ao carregar os arquivos do commit: {error}")
}

fn commit_gone(oid: &str) -> String {
    format!(
        "O commit {} não faz mais parte do PR (force-push)",
        short_sha(oid)
    )
}

fn commit_file_count(count: usize) -> String {
    match count {
        1 => "1 arquivo neste commit".to_string(),
        count => format!("{count} arquivos neste commit"),
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ReviewScope {
    #[default]
    All,
    Commit {
        oid: String,
        headline: String,
    },
}

pub enum ReviewEvent {
    ShowAll,
}

enum CommitFiles {
    Idle,
    Loading,
    Ready(Vec<PrFile>),
    Failed(SharedString),
}

pub struct ReviewTab {
    detail: Option<Rc<PrDetail>>,
    scope: ReviewScope,
    files: CommitFiles,
    notice: Option<SharedString>,
    epoch: u64,
    load_task: Option<Task<()>>,
    scroll: ScrollHandle,
}

impl EventEmitter<ReviewEvent> for ReviewTab {}

impl ReviewTab {
    pub fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self {
            detail: None,
            scope: ReviewScope::All,
            files: CommitFiles::Idle,
            notice: None,
            epoch: 0,
            load_task: None,
            scroll: ScrollHandle::new(),
        }
    }

    pub fn set_detail(&mut self, detail: Rc<PrDetail>, cx: &mut Context<Self>) {
        if let ReviewScope::Commit { oid, .. } = &self.scope
            && !detail.commits.iter().any(|commit| commit.oid == *oid)
        {
            self.notice = Some(commit_gone(oid).into());
            self.scope = ReviewScope::All;
            self.files = CommitFiles::Idle;
        }
        self.detail = Some(detail);
        if matches!(self.files, CommitFiles::Idle) {
            self.load_files(cx);
        }
        cx.notify();
    }

    pub fn count(&self) -> Option<usize> {
        self.detail
            .as_ref()
            .map(|detail| detail.pr.changed_files as usize)
    }

    pub fn set_scope(&mut self, scope: ReviewScope, cx: &mut Context<Self>) {
        if scope != self.scope {
            self.files = CommitFiles::Idle;
            self.scope = scope;
            self.scroll.set_offset(point(px(0.), px(0.)));
        }
        self.notice = None;
        if matches!(self.files, CommitFiles::Idle | CommitFiles::Failed(_)) {
            self.load_files(cx);
        }
        cx.notify();
    }

    fn load_files(&mut self, cx: &mut Context<Self>) {
        self.epoch += 1;
        self.load_task = None;
        let ReviewScope::Commit { oid, .. } = &self.scope else {
            self.files = CommitFiles::Idle;
            return;
        };
        let Some(detail) = self.detail.clone() else {
            self.files = CommitFiles::Idle;
            return;
        };
        let Some(services) = cx
            .try_global::<AppServices>()
            .map(|global| global.0.clone())
        else {
            self.files = CommitFiles::Failed(files_failed(labels::SERVICES_UNAVAILABLE).into());
            return;
        };
        self.files = CommitFiles::Loading;
        let epoch = self.epoch;
        let oid = oid.clone();
        let repo = detail.pr.repo.clone();
        let task = services.spawn_github(cx, detail.pr.host.clone(), move |client| async move {
            client.commit_files(&repo, &oid).await
        });
        self.load_task = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            _ = this.update(cx, |this, cx| {
                if epoch != this.epoch {
                    return;
                }
                this.files = match result {
                    Ok(files) => CommitFiles::Ready(files),
                    Err(error) => {
                        tracing::warn!(%error, "failed to load commit files");
                        CommitFiles::Failed(files_failed(&error.to_string()).into())
                    }
                };
                cx.notify();
            });
        }));
    }

    fn banner(&self, oid: &str, headline: &str, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        h_flex()
            .w_full()
            .flex_none()
            .px_4()
            .py_2()
            .gap_2()
            .items_center()
            .bg(theme.muted)
            .border_b_1()
            .border_color(theme.border)
            .text_sm()
            .child(
                Icon::new(IconName::GitCommitHorizontal)
                    .small()
                    .text_color(theme.muted_foreground),
            )
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_color(theme.foreground)
                    .child(commit_banner(oid, headline)),
            )
            .child(
                div()
                    .flex_none()
                    .text_color(theme.muted_foreground)
                    .child(SEPARATOR),
            )
            .child(
                div()
                    .id("review-show-all")
                    .flex_none()
                    .text_color(theme.link)
                    .cursor_pointer()
                    .hover(|style| style.underline())
                    .child(SHOW_ALL)
                    .on_click(
                        cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(ReviewEvent::ShowAll)),
                    ),
            )
    }

    fn files_body(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let note = |text: SharedString, color| div().text_sm().text_color(color).child(text);
        match &self.files {
            CommitFiles::Idle | CommitFiles::Loading => {
                v_flex().child(note(LOADING_FILES.into(), theme.muted_foreground))
            }
            CommitFiles::Failed(message) => v_flex()
                .gap_2()
                .child(note(message.clone(), theme.danger))
                .child(
                    div()
                        .id("review-files-retry")
                        .text_sm()
                        .text_color(theme.link)
                        .cursor_pointer()
                        .hover(|style| style.underline())
                        .child(labels::RETRY)
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.load_files(cx);
                            cx.notify();
                        })),
                ),
            CommitFiles::Ready(files) if files.is_empty() => {
                v_flex().child(note(NO_FILES.into(), theme.muted_foreground))
            }
            CommitFiles::Ready(files) => v_flex()
                .gap_2()
                .child(note(
                    commit_file_count(files.len()).into(),
                    theme.muted_foreground,
                ))
                .child(commit_files_list(files, cx)),
        }
    }
}

fn notice_banner(message: SharedString, cx: &App) -> Div {
    let theme = cx.theme();
    h_flex()
        .w_full()
        .flex_none()
        .px_4()
        .py_2()
        .gap_2()
        .items_center()
        .border_b_1()
        .border_color(theme.warning.opacity(0.5))
        .bg(theme.warning.opacity(0.08))
        .text_sm()
        .text_color(theme.foreground)
        .child(
            Icon::new(IconName::TriangleAlert)
                .small()
                .text_color(theme.warning),
        )
        .child(message)
}

impl Render for ReviewTab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ReviewScope::Commit { oid, headline } = self.scope.clone() else {
            let placeholder = tab_placeholder(
                IconName::FileDiff,
                labels::UNDER_CONSTRUCTION,
                labels::REVIEW_SOON,
                cx,
            );
            return match self.notice.clone() {
                Some(notice) => v_flex()
                    .size_full()
                    .child(notice_banner(notice, cx))
                    .child(placeholder.flex_1().min_h_0()),
                None => placeholder,
            };
        };
        let banner = self.banner(&oid, &headline, cx);
        let files = self.files_body(cx);
        let theme = cx.theme();
        let hint = div()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(labels::REVIEW_SOON);
        let content = v_flex()
            .w_full()
            .min_w_0()
            .max_w(px(CONTENT_MAX_WIDTH))
            .mx_auto()
            .px_4()
            .py_4()
            .gap_4()
            .text_color(theme.foreground)
            .child(files)
            .child(hint);
        v_flex().size_full().child(banner).child(
            div()
                .id("pr-review-scroll")
                .w_full()
                .flex_1()
                .min_h_0()
                .track_scroll(&self.scroll)
                .overflow_y_scroll()
                .vertical_scrollbar(&self.scroll)
                .child(content),
        )
    }
}

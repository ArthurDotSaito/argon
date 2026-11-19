use argon_store::PrSummary;
use chrono::Utc;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _, StyledExt as _, WindowExt as _, h_flex,
};
use gpui_kit::{App, Context, Div, ParentElement as _, Styled as _, Window, div};

use super::{Load, PrView};
use crate::agents::{SummaryTarget, Target, open_start_dialog};
use crate::pr::labels;
use crate::services::{AppServices, flatten};
use crate::ui;
use crate::workspace::{SessionPlacement, Workspace};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummaryStatus {
    None,
    Fresh,
    Stale,
}

pub fn summary_status(summary: Option<&PrSummary>, head_oid: &str) -> SummaryStatus {
    match summary {
        None => SummaryStatus::None,
        Some(summary) if head_oid.is_empty() || summary.head_sha == head_oid => {
            SummaryStatus::Fresh
        }
        Some(_) => SummaryStatus::Stale,
    }
}

fn open_session_beside(session_id: String, window: &mut Window, cx: &mut App) {
    if let Some(workspace) = Workspace::global(cx) {
        _ = workspace.update(cx, |workspace, cx| {
            workspace.open_session(session_id, SessionPlacement::BesideSubject, window, cx)
        });
    }
}

impl PrView {
    pub(super) fn settle_pending_summarize(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.pending_summarize {
            return;
        }
        if self.pr().is_some() {
            self.pending_summarize = false;
            cx.defer_in(window, |this, window, cx| this.open_summarize(window, cx));
        } else if matches!(self.load, Load::Failed(_)) {
            self.pending_summarize = false;
            cx.defer_in(window, |_, window, cx| {
                window.push_notification(Notification::error(labels::SUMMARIZE_LOAD_FAILED), cx)
            });
        }
    }

    pub fn open_summarize(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pr) = self.pr().cloned() else {
            self.pending_summarize = true;
            cx.notify();
            return;
        };
        let target = SummaryTarget {
            pr,
            detail: self.detail.as_deref().cloned(),
        };
        open_start_dialog(Target::Summary(target), window, cx);
    }

    pub(super) fn reload_summary(&mut self, cx: &mut Context<Self>) {
        let Some(services) = cx
            .try_global::<AppServices>()
            .map(|global| global.0.clone())
        else {
            return;
        };
        let (host, id) = (self.key.host.clone(), self.key.id.0.clone());
        self.summary_task = Some(cx.spawn(async move |this, cx| {
            let stored = flatten(
                services
                    .store()
                    .call(move |store| store.latest_summary_for_pr(&host, &id))
                    .await,
            );
            _ = this.update(cx, |this, cx| match stored {
                Ok(summary) => {
                    this.summary = summary;
                    cx.notify();
                }
                Err(error) => tracing::warn!(%error, "failed to load the pr summary"),
            });
        }));
    }

    fn summary_status(&self) -> SummaryStatus {
        let head = self
            .detail
            .as_ref()
            .map_or("", |detail| detail.head_oid.as_str());
        summary_status(self.summary.as_ref(), head)
    }

    pub(super) fn summary_banner(&self, cx: &mut Context<Self>) -> Option<Div> {
        let status = self.summary_status();
        let summary = self.summary.as_ref()?;
        let session = summary.session_id.clone();
        let stale = status == SummaryStatus::Stale;
        let theme = cx.theme();
        let accent = if stale { theme.warning } else { theme.primary };
        let (icon, title, note) = if stale {
            (
                IconName::TriangleAlert,
                labels::SUMMARY_STALE,
                labels::SUMMARY_STALE_NOTE.to_string(),
            )
        } else {
            (
                IconName::TextAlignStart,
                labels::SUMMARY_AVAILABLE,
                labels::summary_fresh_note(&ui::ago(Utc::now(), summary.created_at)),
            )
        };
        let regenerate = stale.then(|| {
            Button::new("pr-summary-regenerate")
                .icon(Icon::new(IconName::RefreshCw))
                .label(labels::SUMMARY_REGENERATE)
                .small()
                .outline()
                .on_click(cx.listener(|this, _, window, cx| this.open_summarize(window, cx)))
        });
        Some(
            h_flex()
                .flex_none()
                .mx_4()
                .my_2()
                .px_3()
                .py_1p5()
                .gap_2()
                .items_center()
                .rounded(theme.radius)
                .border_1()
                .border_color(accent.opacity(0.5))
                .bg(accent.opacity(0.08))
                .text_sm()
                .child(Icon::new(icon).small().text_color(accent))
                .child(div().font_semibold().child(title))
                .child(div().text_color(theme.muted_foreground).child(note))
                .child(div().flex_1())
                .child(
                    Button::new("pr-summary-open")
                        .label(labels::OPEN_SUMMARY)
                        .small()
                        .link()
                        .on_click(move |_, window, cx| {
                            open_session_beside(session.clone(), window, cx)
                        }),
                )
                .children(regenerate),
        )
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;

    fn summary(head_sha: &str) -> PrSummary {
        PrSummary {
            id: 1,
            host: "github.com".into(),
            pr_id: "PR_1".into(),
            session_id: "s1".into(),
            body: "Resumo".into(),
            head_sha: head_sha.into(),
            touched: Vec::new(),
            created_at: Utc.with_ymd_and_hms(2026, 11, 13, 10, 0, 0).unwrap(),
        }
    }

    #[test]
    fn no_summary_means_no_banner() {
        assert_eq!(summary_status(None, "abc"), SummaryStatus::None);
    }

    #[test]
    fn matching_head_is_fresh() {
        assert_eq!(
            summary_status(Some(&summary("abc")), "abc"),
            SummaryStatus::Fresh
        );
    }

    #[test]
    fn different_head_is_stale() {
        assert_eq!(
            summary_status(Some(&summary("abc")), "def"),
            SummaryStatus::Stale
        );
    }

    #[test]
    fn unknown_head_counts_as_fresh() {
        assert_eq!(
            summary_status(Some(&summary("abc")), ""),
            SummaryStatus::Fresh
        );
    }
}

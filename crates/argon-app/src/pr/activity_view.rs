use argon_github::{Label, PrDetail, ReviewThread, TimelineItem};
use chrono::{DateTime, Local, NaiveDate, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::Button;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Div, ElementId, Hsla, InteractiveElement as _,
    IntoElement, ParentElement as _, SharedString, Stateful, StatefulInteractiveElement as _,
    Styled as _, div, px,
};

use super::activity::{
    self, Activity, ActivityDay, ActivityEntry, ActivityFilter, ActivityKind, ActivityOrder, Hidden,
};
use super::commits::day_label;
use super::conversation::{Conversation, ConversationEvent, truncated_notice};
use super::header::label_chip;
use super::labels;
use super::timeline_item::{
    EVENT_ICON_BOX, MarkdownBlock, card, card_body, card_text, diff_hunk, event, file_line,
    review_pill, status_pill,
};
use crate::ui;

const FOLD_COMMITS: usize = 5;
const FOLD_INDENT: f32 = EVENT_ICON_BOX + 14.;
const REPLY_AVATAR: f32 = 18.;
const OPEN_ACCENT_WIDTH: f32 = 3.;
const SEGMENT_HEIGHT: f32 = 22.;
const DIVIDER_OPACITY: f32 = 0.45;

fn local_day(at: DateTime<Utc>) -> NaiveDate {
    at.with_timezone(&Local).date_naive()
}

impl Conversation {
    pub(super) fn activity_section(
        &self,
        detail: &PrDetail,
        now: DateTime<Utc>,
        cx: &Context<Self>,
    ) -> Div {
        let today = local_day(now);
        let activity = activity::build(detail, self.filter, self.order, local_day);
        let days: Vec<Div> = activity
            .days
            .iter()
            .map(|day| self.day(day, detail, today, now, cx))
            .collect();
        let hidden = (self.filter == ActivityFilter::Conversation && activity.hidden.total() > 0)
            .then(|| self.hidden_note(&activity.hidden, cx));
        let truncated = (detail.truncated_timeline || detail.truncated_threads).then(|| {
            let open =
                cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(ConversationEvent::OpenOnGithub));
            truncated_notice("pr-timeline-open-github", open, cx)
        });
        let section = v_flex()
            .w_full()
            .gap_4()
            .child(self.activity_header(&activity, cx));
        match self.order {
            ActivityOrder::NewestFirst => section
                .child(self.composer.clone())
                .children(days)
                .children(hidden)
                .children(truncated),
            ActivityOrder::OldestFirst => section
                .children(truncated)
                .children(hidden)
                .children(days)
                .child(self.composer.clone()),
        }
    }

    fn activity_header(&self, activity: &Activity, cx: &Context<Self>) -> Div {
        let newest = self.order == ActivityOrder::NewestFirst;
        let (icon, label) = if newest {
            (IconName::ArrowDown, labels::ORDER_NEWEST)
        } else {
            (IconName::ArrowUp, labels::ORDER_OLDEST)
        };
        let order = Button::new("pr-activity-order")
            .icon(Icon::new(icon))
            .label(label)
            .xsmall()
            .outline()
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_order(cx)));
        h_flex()
            .w_full()
            .gap_2()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .font_semibold()
                    .text_color(cx.theme().foreground)
                    .child(labels::ACTIVITY),
            )
            .child(order)
            .child(self.filter_segment(activity, cx))
    }

    fn filter_segment(&self, activity: &Activity, cx: &Context<Self>) -> Div {
        let theme = cx.theme();
        let options = [
            (
                ActivityFilter::Conversation,
                labels::FILTER_CONVERSATION,
                activity.talk_count,
            ),
            (
                ActivityFilter::All,
                labels::FILTER_ALL,
                activity.total_count,
            ),
        ];
        h_flex()
            .h(px(SEGMENT_HEIGHT))
            .flex_none()
            .rounded(theme.radius)
            .border_1()
            .border_color(theme.border)
            .overflow_hidden()
            .children(
                options
                    .into_iter()
                    .enumerate()
                    .map(|(ix, (filter, label, count))| {
                        let selected = filter == self.filter;
                        h_flex()
                            .id(("pr-activity-filter", ix))
                            .h_full()
                            .px_2()
                            .gap_1p5()
                            .items_center()
                            .text_xs()
                            .cursor_pointer()
                            .when(ix > 0, |this| this.border_l_1().border_color(theme.border))
                            .map(|this| {
                                if selected {
                                    this.bg(theme.accent).text_color(theme.foreground)
                                } else {
                                    this.text_color(theme.muted_foreground)
                                        .hover(|style| style.text_color(theme.foreground))
                                }
                            })
                            .child(label)
                            .child(
                                div()
                                    .font_family(theme.mono_font_family.clone())
                                    .child(count.to_string()),
                            )
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.set_filter(filter, cx)
                            }))
                    }),
            )
    }

    fn day(
        &self,
        day: &ActivityDay,
        detail: &PrDetail,
        today: NaiveDate,
        now: DateTime<Utc>,
        cx: &Context<Self>,
    ) -> Div {
        let theme = cx.theme();
        let divider = h_flex()
            .w_full()
            .gap_2()
            .items_center()
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .font_semibold()
                    .text_color(theme.muted_foreground)
                    .child(day_label(day.date, today)),
            )
            .child(div().flex_1().h(px(1.)).bg(theme.border));
        v_flex().w_full().gap_3().child(divider).children(
            day.entries
                .iter()
                .filter_map(|entry| self.entry(entry, detail, now, cx)),
        )
    }

    fn entry(
        &self,
        entry: &ActivityEntry,
        detail: &PrDetail,
        now: DateTime<Utc>,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        match &entry.kind {
            ActivityKind::Comment { ix } => {
                let TimelineItem::Comment {
                    id,
                    author,
                    created_at,
                    ..
                } = detail.timeline.get(*ix)?
                else {
                    return None;
                };
                let body = self.bodies.get(*ix)?.as_ref();
                Some(if entry.talk {
                    let action = div().child(labels::COMMENTED);
                    person_card(author, action, *created_at, now, body, cx)
                } else {
                    let key = format!("pr-fold-comment-{id}");
                    self.bot_line(key, author, labels::COMMENTED, *created_at, body, now, cx)
                })
            }
            ActivityKind::Review { ix } => {
                let TimelineItem::Review {
                    id,
                    author,
                    state,
                    submitted_at,
                    ..
                } = detail.timeline.get(*ix)?
                else {
                    return None;
                };
                let body = self.bodies.get(*ix)?.as_ref();
                Some(if entry.talk {
                    let action = h_flex()
                        .gap_1p5()
                        .items_center()
                        .child(labels::REVIEWED)
                        .child(review_pill(*state, cx));
                    person_card(author, action, *submitted_at, now, body, cx)
                } else {
                    let key = format!("pr-fold-review-{id}");
                    self.bot_line(key, author, labels::REVIEWED, *submitted_at, body, now, cx)
                })
            }
            ActivityKind::Thread { ix } => self.thread_card(*ix, detail.threads.get(*ix)?, now, cx),
            ActivityKind::Commits { ixs } => self.commits_line(ixs, detail, entry.at, now, cx),
            ActivityKind::Event { ix } => {
                event_line(detail.timeline.get(*ix)?, &detail.labels, now, cx)
            }
            ActivityKind::Opened => {
                let pr = &detail.pr;
                let text = actor_text(&pr.author, div().child(labels::OPENED_THIS_PR), cx);
                let color = cx.theme().success;
                Some(event(
                    IconName::GitPullRequest,
                    color,
                    text,
                    pr.created_at,
                    now,
                    cx,
                ))
            }
        }
    }

    fn fold_link(
        &self,
        key: SharedString,
        open: bool,
        show: &'static str,
        hide: &'static str,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let label = if open { hide } else { show };
        link(ElementId::Name(key.clone()), label, cx).on_click(
            cx.listener(move |this, _: &ClickEvent, _, cx| this.toggle_fold(key.clone(), cx)),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn bot_line(
        &self,
        key: String,
        author: &str,
        action: &'static str,
        at: DateTime<Utc>,
        body: Option<&MarkdownBlock>,
        now: DateTime<Utc>,
        cx: &Context<Self>,
    ) -> AnyElement {
        let key: SharedString = key.into();
        let open = self.open_folds.contains(&key);
        let toggle = body.map(|_| self.fold_link(key, open, labels::SHOW, labels::HIDE, cx));
        let text = actor_text(author, div().child(action), cx).children(toggle);
        let line = event(
            IconName::Bot,
            cx.theme().muted_foreground,
            text,
            at,
            now,
            cx,
        );
        let fold = body
            .filter(|_| open)
            .map(|body| fold(boxed(cx).child(card_text(body.view()))));
        v_flex()
            .w_full()
            .gap_1p5()
            .child(line)
            .children(fold)
            .into_any_element()
    }

    fn commits_line(
        &self,
        ixs: &[usize],
        detail: &PrDetail,
        at: DateTime<Utc>,
        now: DateTime<Utc>,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        let theme = cx.theme();
        let commits: Vec<(&str, &str, &str)> = ixs
            .iter()
            .filter_map(|ix| match detail.timeline.get(*ix)? {
                TimelineItem::Commit {
                    oid,
                    headline,
                    author,
                    ..
                } => Some((oid.as_str(), headline.as_str(), author.as_str())),
                _ => None,
            })
            .collect();
        let (first_oid, _, first_author) = *commits.first()?;
        let several = commits.iter().any(|(_, _, author)| *author != first_author);
        let author = if several {
            labels::SEVERAL_AUTHORS
        } else {
            first_author
        };
        let key: SharedString = format!("pr-fold-commits-{first_oid}").into();
        let open = self.open_folds.contains(&key);
        let toggle = self.fold_link(key, open, labels::SHOW_COMMITS, labels::HIDE_COMMITS, cx);
        let text = actor_text(
            author,
            div().child(labels::added_commits(commits.len(), several)),
            cx,
        )
        .child(toggle);
        let line = event(
            IconName::GitCommitHorizontal,
            theme.muted_foreground,
            text,
            at,
            now,
            cx,
        );
        let list = open.then(|| {
            let rest = commits.len().saturating_sub(FOLD_COMMITS);
            let rows = commits
                .iter()
                .rev()
                .take(FOLD_COMMITS)
                .map(|(oid, headline, _)| {
                    h_flex()
                        .w_full()
                        .min_w_0()
                        .gap_2()
                        .text_sm()
                        .child(
                            div()
                                .flex_none()
                                .font_family(theme.mono_font_family.clone())
                                .text_xs()
                                .text_color(theme.link)
                                .child(labels::short_sha(oid).to_string()),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_color(theme.foreground)
                                .child(headline.to_string()),
                        )
                });
            let open_id = ElementId::Name(format!("pr-activity-open-commits-{first_oid}").into());
            let open_tab = link(open_id, labels::OPEN_COMMITS_TAB, cx).on_click(
                cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(ConversationEvent::OpenCommits)),
            );
            let more = h_flex()
                .gap_1p5()
                .text_sm()
                .text_color(theme.muted_foreground)
                .children((rest > 0).then(|| labels::more_commits(rest)))
                .child(open_tab);
            fold(v_flex().w_full().gap_1().children(rows).child(more))
        });
        Some(
            v_flex()
                .w_full()
                .gap_1p5()
                .child(line)
                .children(list)
                .into_any_element(),
        )
    }

    fn thread_card(
        &self,
        ix: usize,
        thread: &ReviewThread,
        now: DateTime<Utc>,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        let theme = cx.theme();
        let bodies = self.thread_bodies.get(ix)?;
        let first = thread.comments.first()?;
        let is_open = !thread.resolved && !thread.outdated;
        let (status, color) = if thread.resolved {
            (labels::THREAD_RESOLVED, theme.success)
        } else if thread.outdated {
            (labels::THREAD_OUTDATED, theme.muted_foreground)
        } else {
            (labels::THREAD_OPEN, theme.warning)
        };
        let action = h_flex()
            .gap_1p5()
            .items_center()
            .child(labels::COMMENTED_ON_CODE)
            .child(status_pill(None, status, color, color));
        let line = thread.line.or(thread.original_line);
        let hunk = self.hunks.get(ix).and_then(Option::as_deref);
        let replies = thread
            .comments
            .iter()
            .zip(bodies)
            .skip(1)
            .map(|(comment, body)| {
                h_flex()
                    .w_full()
                    .px_3()
                    .py_2()
                    .gap_2()
                    .items_start()
                    .border_t_1()
                    .border_color(theme.border.opacity(DIVIDER_OPACITY))
                    .child(ui::avatar(&comment.author, REPLY_AVATAR))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_0p5()
                            .text_sm()
                            .child(
                                h_flex()
                                    .gap_1p5()
                                    .text_color(theme.muted_foreground)
                                    .child(
                                        div()
                                            .font_semibold()
                                            .text_color(theme.foreground)
                                            .child(comment.author.clone()),
                                    )
                                    .child(
                                        div()
                                            .font_family(theme.mono_font_family.clone())
                                            .text_xs()
                                            .child(ui::age_label(now, comment.created_at)),
                                    ),
                            )
                            .child(body.view()),
                    )
            });
        let reveal = line.filter(|_| !thread.outdated).map(|line| {
            let path = thread.path.clone();
            let side = thread.side;
            let button = Button::new(("pr-thread-reveal", ix))
                .label(labels::GO_TO_LINE)
                .xsmall()
                .outline()
                .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                    cx.emit(ConversationEvent::Reveal {
                        path: path.clone(),
                        line,
                        side,
                    })
                }));
            h_flex()
                .w_full()
                .px_3()
                .py_1p5()
                .border_t_1()
                .border_color(theme.border.opacity(DIVIDER_OPACITY))
                .child(button)
        });
        let body = card_body(&first.author, action, first.created_at, now, cx)
            .child(file_line(&thread.path, line, cx))
            .children(hunk.map(|hunk| diff_hunk(hunk, cx)))
            .children(bodies.first().map(|body| card_text(body.view())))
            .children(replies)
            .children(reveal);
        let card = card(&first.author, body);
        if !is_open {
            return Some(card.into_any_element());
        }
        Some(
            h_flex()
                .w_full()
                .min_w_0()
                .gap_2()
                .items_stretch()
                .child(
                    div()
                        .w(px(OPEN_ACCENT_WIDTH))
                        .flex_none()
                        .rounded_full()
                        .bg(theme.warning),
                )
                .child(card)
                .into_any_element(),
        )
    }

    fn hidden_note(&self, hidden: &Hidden, cx: &Context<Self>) -> Div {
        let theme = cx.theme();
        let show_all = link("pr-activity-show-all", labels::SHOW_ALL, cx).on_click(
            cx.listener(|this, _: &ClickEvent, _, cx| this.set_filter(ActivityFilter::All, cx)),
        );
        h_flex()
            .w_full()
            .pt_3()
            .gap_1p5()
            .border_t_1()
            .border_color(theme.border)
            .text_sm()
            .text_color(theme.muted_foreground)
            .child(labels::hidden_events(
                hidden.commits,
                hidden.bot,
                hidden.events,
            ))
            .child(show_all)
    }
}

pub(super) fn link(id: impl Into<ElementId>, label: &'static str, cx: &App) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .text_color(cx.theme().link)
        .cursor_pointer()
        .hover(|style| style.underline())
        .child(label)
}

fn actor_text(actor: &str, action: impl IntoElement, cx: &App) -> Div {
    h_flex()
        .min_w_0()
        .gap_1p5()
        .items_center()
        .child(
            div()
                .flex_none()
                .font_medium()
                .text_color(cx.theme().foreground)
                .child(actor.to_string()),
        )
        .child(action)
}

fn fold(content: Div) -> Div {
    div().w_full().pl(px(FOLD_INDENT)).child(content)
}

fn boxed(cx: &App) -> Div {
    let theme = cx.theme();
    div()
        .w_full()
        .min_w_0()
        .overflow_hidden()
        .border_1()
        .border_color(theme.border)
        .rounded(theme.radius)
        .text_color(theme.foreground)
}

fn person_card(
    author: &str,
    action: impl IntoElement,
    at: DateTime<Utc>,
    now: DateTime<Utc>,
    body: Option<&MarkdownBlock>,
    cx: &App,
) -> AnyElement {
    card(
        author,
        card_body(author, action, at, now, cx).children(body.map(|body| card_text(body.view()))),
    )
    .into_any_element()
}

fn event_line(
    item: &TimelineItem,
    pr_labels: &[Label],
    now: DateTime<Utc>,
    cx: &App,
) -> Option<AnyElement> {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let plain = |text: &'static str| div().child(text);
    let (icon, color, actor, action, at): (IconName, Hsla, &str, Div, DateTime<Utc>) = match item {
        TimelineItem::ReviewRequested {
            actor,
            reviewer,
            at,
        } => (
            IconName::UserPlus,
            muted,
            actor,
            div().child(labels::review_requested(reviewer)),
            *at,
        ),
        TimelineItem::Labeled { actor, label, at } => {
            let color = pr_labels
                .iter()
                .find(|known| known.name.eq_ignore_ascii_case(label))
                .map(|known| known.color.as_str());
            let action = h_flex()
                .gap_1p5()
                .items_center()
                .child(labels::LABELED)
                .child(label_chip(label, color, cx));
            (IconName::Tag, muted, actor, action, *at)
        }
        TimelineItem::HeadRefForcePushed { actor, at } => (
            IconName::GitCommitHorizontal,
            theme.warning,
            actor,
            plain(labels::FORCE_PUSHED),
            *at,
        ),
        TimelineItem::Merged { actor, at } => (
            IconName::GitMerge,
            theme.magenta,
            actor,
            plain(labels::MERGED),
            *at,
        ),
        TimelineItem::Closed { actor, at } => (
            IconName::GitPullRequestClosed,
            theme.danger,
            actor,
            plain(labels::CLOSED),
            *at,
        ),
        TimelineItem::ReadyForReview { actor, at } => (
            IconName::GitPullRequest,
            theme.success,
            actor,
            plain(labels::READY_FOR_REVIEW),
            *at,
        ),
        _ => return None,
    };
    Some(event(
        icon,
        color,
        actor_text(actor, action, cx),
        at,
        now,
        cx,
    ))
}

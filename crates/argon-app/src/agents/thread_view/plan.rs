use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::{
    AnyElement, AppContext as _, ClickEvent, Context, Entity, IntoElement, ParentElement as _,
    SharedString, Styled as _, Subscription, Window, div,
};

use super::ThreadView;
use crate::agents::panel_labels as labels;
use crate::agents::plan_card::{PlanCardView, can_publish, plan_card_view};
use crate::agents::safe_markdown::{agent_markdown, is_safe_link};
use crate::agents::thread_model::PlanBusy;
use crate::agents::transcript::Item;
use crate::review::{CommentEditor, CommentEditorEvent, EditorAction, EditorKind};

pub(super) struct PlanEditor {
    draft_id: i64,
    editor: Entity<CommentEditor>,
    _subscriptions: [Subscription; 2],
}

struct CardData {
    view: PlanCardView,
    body: String,
    edited: bool,
    busy: Option<PlanBusy>,
    error: Option<SharedString>,
}

fn open_comment(url: &str) {
    if !is_safe_link(url) {
        return;
    }
    if let Err(error) = open::that_detached(url) {
        tracing::warn!(%error, "failed to open the published plan");
    }
}

impl ThreadView {
    fn card_data(&self, id: i64, cx: &Context<Self>) -> Option<CardData> {
        let model = self.model.read(cx);
        let draft = model.plan_draft(id)?;
        Some(CardData {
            view: plan_card_view(draft, model.is_latest_plan(id)),
            body: draft.body.clone(),
            edited: draft.edited,
            busy: model.plan_busy(id),
            error: model.plan_error(id).cloned(),
        })
    }

    pub(super) fn plan_card(&self, id: i64, cx: &mut Context<Self>) -> AnyElement {
        let Some(data) = self.card_data(id, cx) else {
            return div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(labels::PLAN_LOADING)
                .into_any_element();
        };
        let editor = self
            .plan_editor
            .as_ref()
            .filter(|editing| editing.draft_id == id)
            .map(|editing| editing.editor.clone());
        let editing = editor.is_some();
        let published = matches!(data.view, PlanCardView::Published { .. });
        let published_link = match &data.view {
            PlanCardView::Published { url, .. } => Some(self.published_link(id, url.clone())),
            PlanCardView::Draft { .. } => None,
        };
        let show_body = match &data.view {
            PlanCardView::Draft { .. } => !editing,
            PlanCardView::Published { latest, .. } => *latest,
        };
        let conflict = matches!(data.view, PlanCardView::Draft { new_version: true })
            .then(|| self.new_version_notice(id, data.busy, cx));
        let actions = matches!(data.view, PlanCardView::Draft { .. })
            .then(|| self.plan_actions(id, &data, editing, cx));
        let theme = cx.theme();
        let edited = (data.edited && !published).then(|| {
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(labels::PLAN_EDITED)
        });
        let header = h_flex()
            .gap_1p5()
            .items_center()
            .child(
                Icon::new(IconName::NotebookPen)
                    .xsmall()
                    .text_color(theme.primary),
            )
            .child(div().text_sm().font_semibold().child(labels::PLAN))
            .children(edited)
            .child(div().flex_1())
            .children(published_link);
        let body = show_body.then(|| {
            div().text_sm().child(agent_markdown(
                SharedString::from(format!("plan-body-{id}")),
                &data.body,
            ))
        });
        let error = data
            .error
            .map(|error| div().text_xs().text_color(theme.danger).child(error));
        v_flex()
            .p_2()
            .gap_2()
            .rounded(theme.radius)
            .border_1()
            .border_color(if published {
                theme.border
            } else {
                theme.primary.opacity(0.6)
            })
            .child(header)
            .children(conflict)
            .children(body)
            .children(editor)
            .children(error)
            .children(actions)
            .into_any_element()
    }

    fn published_link(&self, id: i64, url: Option<String>) -> Button {
        let url = url.filter(|url| is_safe_link(url));
        Button::new(SharedString::from(format!("plan-published-{id}")))
            .icon(Icon::new(IconName::ExternalLink))
            .label(labels::PUBLISHED_PLAN)
            .xsmall()
            .link()
            .disabled(url.is_none())
            .on_click(move |_, _, _| {
                if let Some(url) = &url {
                    open_comment(url);
                }
            })
    }

    fn new_version_notice(
        &self,
        id: i64,
        busy: Option<PlanBusy>,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let resolving = busy == Some(PlanBusy::Resolving);
        let theme = cx.theme();
        let warning = theme.warning;
        h_flex()
            .px_2()
            .py_1p5()
            .gap_2()
            .flex_wrap()
            .items_center()
            .rounded(theme.radius)
            .border_1()
            .border_color(warning.opacity(0.5))
            .bg(warning.opacity(0.08))
            .child(Icon::new(IconName::Info).xsmall().text_color(warning))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_xs()
                    .child(labels::NEW_PLAN_VERSION),
            )
            .child(
                Button::new(SharedString::from(format!("plan-use-new-{id}")))
                    .label(labels::USE_NEW_VERSION)
                    .xsmall()
                    .primary()
                    .loading(resolving)
                    .disabled(busy.is_some())
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.close_plan_editor(id, cx);
                        this.model
                            .update(cx, |model, cx| model.resolve_plan_version(id, true, cx));
                    })),
            )
            .child(
                Button::new(SharedString::from(format!("plan-keep-mine-{id}")))
                    .label(labels::KEEP_MINE)
                    .xsmall()
                    .ghost()
                    .disabled(busy.is_some())
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.model
                            .update(cx, |model, cx| model.resolve_plan_version(id, false, cx));
                    })),
            )
    }

    fn plan_actions(
        &self,
        id: i64,
        data: &CardData,
        editing: bool,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let publishing = data.busy == Some(PlanBusy::Publishing);
        let publishable = can_publish(&data.view, data.busy.is_some(), editing);
        let edit = (!editing).then(|| {
            Button::new(SharedString::from(format!("plan-edit-{id}")))
                .icon(Icon::new(IconName::Pencil))
                .label(labels::EDIT_PLAN)
                .xsmall()
                .ghost()
                .disabled(data.busy.is_some())
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.edit_plan(id, window, cx)
                }))
        });
        h_flex()
            .gap_1p5()
            .items_center()
            .child(div().flex_1())
            .children(edit)
            .child(
                Button::new(SharedString::from(format!("plan-publish-{id}")))
                    .icon(Icon::new(IconName::Send))
                    .label(labels::PUBLISH_PLAN)
                    .xsmall()
                    .primary()
                    .loading(publishing)
                    .disabled(!publishable)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.model
                            .update(cx, |model, cx| model.publish_plan(id, cx));
                    })),
            )
    }

    fn edit_plan(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(body) = self
            .model
            .read(cx)
            .plan_draft(id)
            .map(|draft| draft.body.clone())
        else {
            return;
        };
        let editor = cx.new(|cx| {
            CommentEditor::new(
                EditorKind::EditDraft,
                Some(labels::EDIT_PLAN_CAPTION.into()),
                None,
                &body,
                window,
                cx,
            )
        });
        let subscriptions = [
            cx.subscribe(
                &editor,
                |this, _, event: &CommentEditorEvent, cx| match event {
                    CommentEditorEvent::Submit(_) => this.save_plan(cx),
                    CommentEditorEvent::Cancel => {
                        if let Some(id) = this.plan_editor.as_ref().map(|editing| editing.draft_id)
                        {
                            this.close_plan_editor(id, cx);
                        }
                    }
                },
            ),
            cx.observe(&editor, move |this, _, cx| this.remeasure_plan(id, cx)),
        ];
        self.plan_editor = Some(PlanEditor {
            draft_id: id,
            editor,
            _subscriptions: subscriptions,
        });
        self.list.pause_following_tail();
        self.remeasure_plan(id, cx);
    }

    fn save_plan(&mut self, cx: &mut Context<Self>) {
        let Some((id, editor)) = self
            .plan_editor
            .as_ref()
            .map(|editing| (editing.draft_id, editing.editor.clone()))
        else {
            return;
        };
        let Some(body) = editor.read(cx).body(cx) else {
            return;
        };
        editor.update(cx, |editor, cx| {
            editor.set_busy(Some(EditorAction::Save), cx)
        });
        let task = self
            .model
            .update(cx, |model, cx| model.save_plan(id, body, cx));
        cx.spawn(async move |this, cx| {
            let saved = task.await;
            _ = this.update(cx, |this, cx| match saved {
                Ok(()) => this.close_plan_editor(id, cx),
                Err(error) => editor.update(cx, |editor, cx| editor.fail(error, cx)),
            });
        })
        .detach();
    }

    fn close_plan_editor(&mut self, id: i64, cx: &mut Context<Self>) {
        if self
            .plan_editor
            .take_if(|editing| editing.draft_id == id)
            .is_some()
        {
            self.remeasure_plan(id, cx);
        }
    }

    fn remeasure_plan(&mut self, id: i64, cx: &mut Context<Self>) {
        let row = self
            .model
            .read(cx)
            .transcript()
            .items()
            .iter()
            .position(|item| *item == Item::PlanDraft(id))
            .and_then(|ix| self.rows.iter().position(|range| range.contains(&ix)));
        match row {
            Some(row) => self.list.remeasure_items(row..row + 1),
            None => self.list.remeasure_items(0..self.list.item_count()),
        }
        cx.notify();
    }
}

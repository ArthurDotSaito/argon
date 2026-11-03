use std::collections::HashMap;
use std::rc::Rc;

use gpui_kit::{Context, IntoElement as _, Window};

use super::editor::{EditorTarget, OpenEditor};
use super::{Files, ReviewScope, ReviewTab};
use crate::review::confirm::{Confirm, confirm};
use crate::review::diff_view::{BlockRender, BlockSource, InlineBlock};
use crate::review::drafts::{DraftActions, draft_blocks};
use crate::review::labels;

impl ReviewTab {
    fn edit_draft(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self
            .drafts
            .read(cx)
            .drafts()
            .iter()
            .find(|d| d.id == id)
            .cloned()
        else {
            return;
        };
        let target = EditorTarget::Line {
            path: draft.path,
            anchor: (draft.line, draft.side),
            start: draft
                .start_line
                .map(|line| (line, draft.start_side.unwrap_or(draft.side))),
            draft: Some(id),
        };
        self.request_editor(target, draft.body, None, window, cx);
    }

    fn delete_draft(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        let drafts = self.drafts.clone();
        let request = Confirm {
            title: labels::DELETE_DRAFT_TITLE,
            message: labels::DELETE_DRAFT_MESSAGE.to_string(),
            confirm: labels::DELETE,
        };
        confirm(request, window, cx, move |_, cx| {
            drafts.update(cx, |drafts, cx| drafts.delete(id, cx));
        });
    }

    pub(super) fn sync_drafts(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.files, Files::Ready) {
            return;
        }
        let editing = match &self.editor {
            Some(OpenEditor {
                target: EditorTarget::Line { draft, .. },
                ..
            }) => *draft,
            _ => None,
        };
        let mut blocks = if self.scope == ReviewScope::All {
            let actions = self.draft_actions(cx);
            let drafts = self.drafts.read(cx);
            draft_blocks(
                drafts.drafts(),
                editing,
                drafts.suspects(),
                self.viewer_login.as_deref(),
                &actions,
            )
        } else {
            HashMap::new()
        };
        let paths = self.diff_paths(cx);
        self.diff.update(cx, |diff, cx| {
            for path in &paths {
                let blocks = blocks.remove(path).unwrap_or_default();
                diff.set_blocks(path, BlockSource::Drafts, blocks, cx);
            }
        });
    }

    pub(super) fn sync_editor(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.files, Files::Ready) {
            return;
        }
        let block = match &self.editor {
            Some(OpenEditor {
                entity,
                target:
                    EditorTarget::Line {
                        path,
                        anchor,
                        start,
                        ..
                    },
                ..
            }) => {
                let entity = entity.clone();
                let render: BlockRender = Rc::new(move |_, _| entity.clone().into_any_element());
                Some((
                    path.clone(),
                    InlineBlock::line(*anchor, "editor", render).with_start(*start),
                ))
            }
            _ => None,
        };
        let paths = self.diff_paths(cx);
        self.diff.update(cx, |diff, cx| {
            for path in &paths {
                let blocks = match &block {
                    Some((target, block)) if target == path => vec![block.clone()],
                    _ => Vec::new(),
                };
                diff.set_blocks(path, BlockSource::Editor, blocks, cx);
            }
        });
    }

    fn draft_actions(&self, cx: &mut Context<Self>) -> DraftActions {
        let edit_tab = cx.weak_entity();
        let delete_tab = cx.weak_entity();
        DraftActions {
            edit: Rc::new(move |id, window, cx| {
                _ = edit_tab.update(cx, |tab, cx| tab.edit_draft(id, window, cx));
            }),
            delete: Rc::new(move |id, window, cx| {
                _ = delete_tab.update(cx, |tab, cx| tab.delete_draft(id, window, cx));
            }),
        }
    }
}

use argon_core::{PromptTemplate, ReviewCfg, TemplateKind};
use gpui_kit::{AppContext as _, Context, Task, Window};

use super::TemplatesSection;
use super::files::{self, compose, new_template, template_path, write_atomic};
use super::naming::{is_built_in, slugify, unique_id, validate_name};
use super::profile::{join_names, profiles_of};
use super::review_opts::default_of;
use super::templates_of;
use crate::config_watch;
use crate::review::{Confirm, confirm};
use crate::settings::labels;

fn forget_template(review: &mut ReviewCfg, id: &str) {
    let fallback = ReviewCfg::default();
    if review.default_template == id {
        review.default_template = fallback.default_template;
    }
    if review.default_plan_template == id {
        review.default_plan_template = fallback.default_plan_template;
    }
    if review.default_summary_template == id {
        review.default_summary_template = fallback.default_summary_template;
    }
    review.repo_templates.retain(|_, template| template != id);
}

impl TemplatesSection {
    fn existing_ids(&self) -> Vec<String> {
        self.saved_templates()
            .iter()
            .map(|template| template.id.clone())
            .chain(self.invalid.iter().map(|invalid| invalid.id.clone()))
            .collect()
    }

    fn fresh_id(&self, name: &str) -> String {
        let ids = self.existing_ids();
        unique_id(&slugify(name), ids.iter().map(String::as_str))
    }

    pub(super) fn run_io(
        &mut self,
        job: Task<Result<(), String>>,
        select: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.busy = true;
        self.error = None;
        self.io_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = job.await;
            _ = this.update_in(cx, |this, window, cx| match result {
                Ok(()) => this.reload(select, window, cx),
                Err(error) => {
                    this.busy = false;
                    this.error = Some(labels::save_failed(&error).into());
                    cx.notify();
                }
            });
        }));
        cx.notify();
    }

    pub(super) fn write(
        &mut self,
        id: String,
        source: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(dir) = self.dir.clone() else {
            return;
        };
        let path = template_path(&dir, &id);
        let job = cx.background_spawn(async move {
            write_atomic(&path, &source).map_err(|error| error.to_string())
        });
        self.run_io(job, Some(id), window, cx);
    }

    pub(super) fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.fresh_id(labels::NEW_TEMPLATE_NAME);
        let source = compose(&new_template(self.kind));
        self.write(id, source, window, cx);
    }

    pub(super) fn request_kind(
        &mut self,
        kind: TemplateKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.kind == kind {
            return;
        }
        if !self.is_dirty(cx) {
            self.set_kind(kind, window, cx);
            return;
        }
        let name = self.current().map(|t| t.name.clone()).unwrap_or_default();
        let this = cx.entity().downgrade();
        confirm(
            Confirm {
                title: labels::DISCARD_TITLE,
                message: labels::discard_message(&name),
                confirm: labels::DISCARD,
            },
            window,
            cx,
            move |window, cx| {
                _ = this.update(cx, |this, cx| this.set_kind(kind, window, cx));
            },
        );
    }

    fn set_kind(&mut self, kind: TemplateKind, window: &mut Window, cx: &mut Context<Self>) {
        self.kind = kind;
        let keep = self.selected.clone().filter(|id| {
            templates_of(kind, self.saved_templates())
                .iter()
                .any(|template| &template.id == id)
        });
        let wanted = keep.or_else(|| Some(default_of(&self.config.review, kind).to_owned()));
        self.select(wanted, window, cx);
        self.sync_review_options(window, cx);
    }

    pub(super) fn create_profile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let configured = &self.config.review.default_template;
        let base = self
            .saved_templates()
            .iter()
            .find(|template| {
                &template.id == configured
                    && template.kind == TemplateKind::Review
                    && template.base.is_none()
            })
            .map_or_else(
                || ReviewCfg::default().default_template,
                |template| template.id.clone(),
            );
        let id = self.fresh_id(labels::NEW_PROFILE_NAME);
        let source = compose(&PromptTemplate {
            name: labels::NEW_PROFILE_NAME.into(),
            base: Some(base),
            body: labels::NEW_PROFILE_BODY.into(),
            ..PromptTemplate::default()
        });
        self.write(id, source, window, cx);
    }

    pub(super) fn duplicate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.draft(cx) else {
            return;
        };
        let name = labels::copy_of(draft.name.trim());
        let id = self.fresh_id(&name);
        let source = compose(&PromptTemplate { name, ..draft });
        self.write(id, source, window, cx);
    }

    pub(super) fn validated_draft(&mut self, cx: &mut Context<Self>) -> Option<PromptTemplate> {
        let draft = self.draft(cx)?;
        match validate_name(&draft.name) {
            Ok(name) => Some(PromptTemplate { name, ..draft }),
            Err(error) => {
                self.error = Some(error.message().into());
                cx.notify();
                None
            }
        }
    }

    pub(super) fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.validated_draft(cx) else {
            return;
        };
        let id = draft.id.clone();
        let source = compose(&draft);
        self.write(id, source, window, cx);
    }

    pub(super) fn request_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        self.request_delete_of(id, window, cx);
    }

    pub(super) fn request_delete_of(
        &mut self,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected.as_ref() != Some(&id) && !self.is_dirty(cx) {
            self.select(Some(id.clone()), window, cx);
        }
        let Some(name) = self
            .saved_templates()
            .iter()
            .find(|template| template.id == id)
            .map(|template| template.name.clone())
        else {
            return;
        };
        if is_built_in(&id) {
            return;
        }
        let profiles = join_names(profiles_of(self.saved_templates(), &id));
        if !profiles.is_empty() {
            self.error = Some(labels::base_in_use(&profiles).into());
            cx.notify();
            return;
        }
        let this = cx.entity().downgrade();
        confirm(
            Confirm {
                title: labels::DELETE_TITLE,
                message: labels::delete_message(&name),
                confirm: labels::DELETE,
            },
            window,
            cx,
            move |window, cx| {
                let id = id.clone();
                _ = this.update(cx, |this, cx| this.delete(id, window, cx));
            },
        );
    }

    fn delete(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dir) = self.dir.clone() else {
            return;
        };
        let mut forgotten = self.config.review.clone();
        forget_template(&mut forgotten, &id);
        if forgotten != self.config.review {
            self.config =
                config_watch::update(cx, |config| forget_template(&mut config.review, &id));
        }
        if self.selected.as_ref() == Some(&id) {
            self.selected = None;
        }
        let job = cx.background_spawn(async move {
            files::delete(&dir, &id).map_err(|error| error.to_string())
        });
        self.run_io(job, None, window, cx);
    }

    pub(super) fn request_restore(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((id, name)) = self.current().map(|t| (t.id.clone(), t.name.clone())) else {
            return;
        };
        if !is_built_in(&id) {
            return;
        }
        let this = cx.entity().downgrade();
        confirm(
            Confirm {
                title: labels::RESTORE_TITLE,
                message: labels::restore_message(&name),
                confirm: labels::RESTORE,
            },
            window,
            cx,
            move |window, cx| {
                let id = id.clone();
                _ = this.update(cx, |this, cx| this.restore(id, window, cx));
            },
        );
    }

    fn restore(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dir) = self.dir.clone() else {
            return;
        };
        let select = Some(id.clone());
        let job = cx.background_spawn(async move {
            files::restore_default(&dir, &id).map_err(|error| error.to_string())
        });
        self.run_io(job, select, window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn customized() -> ReviewCfg {
        let mut review = ReviewCfg {
            default_template: "meu".into(),
            default_plan_template: "meu".into(),
            default_summary_template: "meu".into(),
            ..ReviewCfg::default()
        };
        review.repo_templates.insert("h/a".into(), "meu".into());
        review.repo_templates.insert("h/b".into(), "outro".into());
        review
    }

    #[test]
    fn forgetting_a_template_restores_every_default_that_pointed_to_it() {
        let mut review = customized();
        forget_template(&mut review, "meu");
        let fallback = ReviewCfg::default();
        assert_eq!(review.default_template, fallback.default_template);
        assert_eq!(review.default_plan_template, fallback.default_plan_template);
        assert_eq!(
            review.default_summary_template,
            fallback.default_summary_template
        );
        assert_eq!(review.repo_templates.len(), 1);
        assert_eq!(review.repo_templates["h/b"], "outro");
    }

    #[test]
    fn forgetting_one_default_leaves_the_others_alone() {
        let configured = ReviewCfg {
            default_template: "a".into(),
            default_plan_template: "b".into(),
            default_summary_template: "c".into(),
            ..ReviewCfg::default()
        };
        let fallback = ReviewCfg::default();
        let mut review = configured.clone();
        forget_template(&mut review, "a");
        assert_eq!(review.default_template, fallback.default_template);
        assert_eq!(review.default_plan_template, "b");
        assert_eq!(review.default_summary_template, "c");
        let mut review = configured.clone();
        forget_template(&mut review, "b");
        assert_eq!(review.default_template, "a");
        assert_eq!(review.default_plan_template, fallback.default_plan_template);
        assert_eq!(review.default_summary_template, "c");
        let mut review = configured;
        forget_template(&mut review, "c");
        assert_eq!(review.default_template, "a");
        assert_eq!(review.default_plan_template, "b");
        assert_eq!(
            review.default_summary_template,
            fallback.default_summary_template
        );
    }
}

use argon_core::{PromptTemplate, ReviewCfg, TemplateKind};
use gpui_kit::{AppContext as _, Context, Task, Window};

use super::TemplatesSection;
use super::files::{self, compose, template_path, write_atomic};
use super::naming::{is_built_in, slugify, unique_id, validate_name};
use super::profile::{join_names, profiles_of};
use crate::config_watch;
use crate::review::{Confirm, confirm};
use crate::settings::labels;

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
        let source = compose(&PromptTemplate {
            name: labels::NEW_TEMPLATE_NAME.into(),
            body: labels::NEW_TEMPLATE_BODY.into(),
            ..PromptTemplate::default()
        });
        self.write(id, source, window, cx);
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
        let Some((id, name)) = self.current().map(|t| (t.id.clone(), t.name.clone())) else {
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
        let review = &self.config.review;
        let referenced = review.default_template == id
            || review.default_plan_template == id
            || review
                .repo_templates
                .values()
                .any(|template| template == &id);
        if referenced {
            self.config = config_watch::update(cx, |config| {
                let fallback = ReviewCfg::default();
                if config.review.default_template == id {
                    config.review.default_template = fallback.default_template;
                }
                if config.review.default_plan_template == id {
                    config.review.default_plan_template = fallback.default_plan_template;
                }
                config
                    .review
                    .repo_templates
                    .retain(|_, template| template != &id);
            });
        }
        self.selected = None;
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

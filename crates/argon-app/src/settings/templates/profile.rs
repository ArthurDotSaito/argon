use argon_core::{
    PROFILE_END, PROFILE_SLOT, ProfileParts, PromptTemplate, base_body, base_parts, detached_body,
    profile_parts,
};
use gpui_kit::component::input::{InputEvent, TextareaState};
use gpui_kit::component::select::{SelectEvent, SelectState};
use gpui_kit::{AppContext as _, Context, Entity, SharedString, Subscription, Window};

use super::TemplatesSection;
use super::files::{compose, template_path, write_atomic};
use crate::agents::Choice;
use crate::review::{Confirm, confirm};
use crate::settings::labels;

const HEAD_ROWS: usize = 12;
const MIDDLE_ROWS: usize = 6;
const TAIL_ROWS: usize = 8;
const NEWLINES: [char; 2] = ['\r', '\n'];

pub(super) struct ProfileEditor {
    pub(super) base_select: Entity<SelectState<Vec<Choice>>>,
    pub(super) head: Entity<TextareaState>,
    pub(super) middle: Entity<TextareaState>,
    pub(super) tail: Entity<TextareaState>,
    pub(super) error: Option<SharedString>,
    base: Option<String>,
    loaded: ProfileParts,
}

impl ProfileEditor {
    pub(super) fn new(window: &mut Window, cx: &mut Context<TemplatesSection>) -> Self {
        Self {
            base_select: cx.new(|cx| SelectState::new(Vec::<Choice>::new(), None, window, cx)),
            head: cx.new(|cx| TextareaState::new(window, cx).rows(HEAD_ROWS)),
            middle: cx.new(|cx| TextareaState::new(window, cx).rows(MIDDLE_ROWS)),
            tail: cx.new(|cx| TextareaState::new(window, cx).rows(TAIL_ROWS)),
            error: None,
            base: None,
            loaded: ProfileParts::default(),
        }
    }

    pub(super) fn subscriptions(
        &self,
        window: &mut Window,
        cx: &mut Context<TemplatesSection>,
    ) -> Vec<Subscription> {
        let mut subscriptions: Vec<Subscription> = [&self.head, &self.middle, &self.tail]
            .into_iter()
            .map(|editor| {
                cx.subscribe(editor, |this, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.rerender(cx);
                    }
                })
            })
            .collect();
        subscriptions.push(cx.subscribe_in(
            &self.base_select,
            window,
            |this, _, event: &SelectEvent<Vec<Choice>>, window, cx| {
                if let SelectEvent::Confirm(Some(id)) = event {
                    this.set_profile_base(id.clone(), window, cx);
                }
            },
        ));
        subscriptions
    }
}

fn block(text: &str) -> &str {
    text.trim_start_matches(NEWLINES).trim_end()
}

pub fn base_edited(parts: &ProfileParts, base: &str) -> bool {
    let (head, tail) = base_parts(base);
    block(&parts.head) != block(&head) || block(&parts.tail) != block(&tail)
}

pub fn profile_text(middle: &str) -> String {
    format!("{}\n", middle.trim())
}

pub fn base_choices(templates: &[PromptTemplate], current: &PromptTemplate) -> Vec<Choice> {
    templates
        .iter()
        .filter(|template| {
            template.kind == current.kind && template.base.is_none() && template.id != current.id
        })
        .map(|template| Choice::new(template.id.clone(), template.name.clone()))
        .collect()
}

pub fn profiles_of<'a>(
    templates: &'a [PromptTemplate],
    base_id: &'a str,
) -> impl Iterator<Item = &'a PromptTemplate> + 'a {
    templates
        .iter()
        .filter(move |template| template.base.as_deref() == Some(base_id))
}

pub fn join_names<'a>(templates: impl Iterator<Item = &'a PromptTemplate>) -> String {
    templates
        .map(|template| template.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

impl TemplatesSection {
    pub(super) fn load_profile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(current) = self
            .current()
            .filter(|template| template.base.is_some())
            .cloned()
        else {
            self.profile.base = None;
            self.profile.error = None;
            self.profile.loaded = ProfileParts::default();
            return;
        };
        let (parts, error) = match profile_parts(&current, self.saved_templates()) {
            Ok(parts) => (parts, None),
            Err(error) => (
                ProfileParts {
                    middle: current.body.trim().to_owned(),
                    ..ProfileParts::default()
                },
                Some(labels::base_failed(&error.to_string()).into()),
            ),
        };
        let choices = base_choices(self.saved_templates(), &current);
        let selected = current.base.clone().unwrap_or_default();
        self.profile.base_select.update(cx, |select, cx| {
            select.set_items(choices, window, cx);
            select.set_selected_value(&selected, window, cx);
        });
        self.set_profile_editors(&parts, window, cx);
        self.profile.base = current.base;
        self.profile.error = error;
        self.profile.loaded = parts;
    }

    fn set_profile_editors(
        &mut self,
        parts: &ProfileParts,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let editors = [
            (&self.profile.head, &parts.head),
            (&self.profile.middle, &parts.middle),
            (&self.profile.tail, &parts.tail),
        ];
        for (editor, text) in editors {
            editor.update(cx, |input, cx| input.set_value(text.clone(), window, cx));
        }
    }

    fn set_profile_base(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(current) = self.current().cloned() else {
            return;
        };
        if self.profile.base.as_deref() == Some(id.as_str()) {
            return;
        }
        let keep_edits = self.is_base_edited(cx);
        self.profile.base = Some(id.clone());
        if !current.detached && !keep_edits {
            let probe = PromptTemplate {
                base: Some(id),
                ..current
            };
            let parts = match profile_parts(&probe, self.saved_templates()) {
                Ok(parts) => {
                    self.profile.error = None;
                    parts
                }
                Err(error) => {
                    self.profile.error = Some(labels::base_failed(&error.to_string()).into());
                    ProfileParts::default()
                }
            };
            for (editor, text) in [
                (&self.profile.head, parts.head),
                (&self.profile.tail, parts.tail),
            ] {
                editor.update(cx, |input, cx| input.set_value(text, window, cx));
            }
        }
        self.rerender(cx);
    }

    pub(super) fn editor_parts(&self, cx: &Context<Self>) -> ProfileParts {
        let text = |editor: &Entity<TextareaState>| editor.read(cx).value().to_string();
        ProfileParts {
            head: text(&self.profile.head),
            middle: text(&self.profile.middle),
            tail: text(&self.profile.tail),
        }
    }

    pub(super) fn profile_draft(
        &self,
        current: &PromptTemplate,
        name: String,
        cx: &Context<Self>,
    ) -> PromptTemplate {
        let parts = self.editor_parts(cx);
        let detached = current.detached || self.is_base_edited(cx);
        let body = if detached {
            detached_body(&parts)
        } else {
            profile_text(&parts.middle)
        };
        PromptTemplate {
            name,
            base: self.profile.base.clone(),
            detached,
            body,
            ..current.clone()
        }
    }

    pub(super) fn base_lacks_slot(&self) -> bool {
        self.current()
            .is_some_and(|template| template.base.is_some() && !template.detached)
            && self
                .selected_base()
                .is_some_and(|base| !base.body.contains(PROFILE_SLOT))
    }

    pub(super) fn can_follow_again(&self) -> bool {
        self.selected_base().is_some()
            && self.current().is_some_and(|template| {
                template.body.contains(PROFILE_SLOT) && template.body.contains(PROFILE_END)
            })
    }

    pub(super) fn profile_dirty(&self, saved: &PromptTemplate, cx: &Context<Self>) -> bool {
        *self.name.read(cx).value() != *saved.name
            || self.profile.base != saved.base
            || self.editor_parts(cx) != self.profile.loaded
    }

    pub(super) fn preview_draft(&self, cx: &Context<Self>) -> Option<PromptTemplate> {
        let current = self.current()?;
        let draft = self.draft(cx)?;
        if current.base.is_none() || (!current.detached && self.profile.error.is_some()) {
            return Some(draft);
        }
        Some(PromptTemplate {
            detached: true,
            body: detached_body(&self.editor_parts(cx)),
            ..draft
        })
    }

    pub(super) fn selected_base(&self) -> Option<&PromptTemplate> {
        let id = self.profile.base.as_deref()?;
        let kind = self.current()?.kind;
        self.saved_templates()
            .iter()
            .find(|template| template.id == id && template.base.is_none() && template.kind == kind)
    }

    pub(super) fn base_label(&self) -> String {
        let id = self.profile.base.clone().unwrap_or_default();
        self.saved_templates()
            .iter()
            .find(|template| template.id == id)
            .map_or(id, |template| template.name.clone())
    }

    pub(super) fn is_base_edited(&self, cx: &Context<Self>) -> bool {
        let following = self
            .current()
            .is_some_and(|template| template.base.is_some() && !template.detached);
        following
            && self.profile.error.is_none()
            && self
                .selected_base()
                .is_some_and(|base| base_edited(&self.editor_parts(cx), &base.body))
    }

    pub(super) fn sibling_profiles(&self) -> String {
        let (Some(base), Some(current)) = (self.profile.base.as_deref(), self.current()) else {
            return String::new();
        };
        join_names(
            profiles_of(self.saved_templates(), base)
                .filter(|template| !template.detached && template.id != current.id),
        )
    }

    pub(super) fn save_profile(
        &mut self,
        detached: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(draft) = self.validated_draft(cx) else {
            return;
        };
        let parts = self.editor_parts(cx);
        let body = if detached {
            detached_body(&parts)
        } else {
            profile_text(&parts.middle)
        };
        let id = draft.id.clone();
        let source = compose(&PromptTemplate {
            detached,
            body,
            ..draft
        });
        self.write(id, source, window, cx);
    }

    pub(super) fn save_in_base(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(dir), Some(base)) = (self.dir.clone(), self.selected_base().cloned()) else {
            return;
        };
        let Some(draft) = self.validated_draft(cx) else {
            return;
        };
        let parts = self.editor_parts(cx);
        let base_path = template_path(&dir, &base.id);
        let profile_path = template_path(&dir, &draft.id);
        let id = draft.id.clone();
        let base_source = compose(&PromptTemplate {
            base: None,
            detached: false,
            body: base_body(&parts.head, &parts.tail),
            ..base
        });
        let profile_source = compose(&PromptTemplate {
            detached: false,
            body: profile_text(&parts.middle),
            ..draft
        });
        let job = cx.background_spawn(async move {
            write_atomic(&profile_path, &profile_source)
                .and_then(|()| write_atomic(&base_path, &base_source))
                .map_err(|error| error.to_string())
        });
        self.run_io(job, Some(id), window, cx);
    }

    pub(super) fn discard_profile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.select(self.selected.clone(), window, cx);
    }

    pub(super) fn request_follow_again(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_follow_again() {
            return;
        }
        let this = cx.entity().downgrade();
        confirm(
            Confirm {
                title: labels::FOLLOW_AGAIN,
                message: labels::follow_again_message(&self.base_label()),
                confirm: labels::FOLLOW_AGAIN,
            },
            window,
            cx,
            move |window, cx| {
                _ = this.update(cx, |this, cx| this.save_profile(false, window, cx));
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use argon_core::TemplateKind;

    use super::*;

    fn template(id: &str, kind: TemplateKind, base: Option<&str>) -> PromptTemplate {
        PromptTemplate {
            id: id.into(),
            name: id.to_uppercase(),
            kind,
            base: base.map(str::to_owned),
            ..PromptTemplate::default()
        }
    }

    #[test]
    fn base_edited_ignores_surrounding_blank_lines() {
        let base = "Início\n\n{# argon:perfil #}\n\nFim\n";
        let same = ProfileParts {
            head: "Início\n".into(),
            middle: "qualquer".into(),
            tail: "\nFim".into(),
        };
        assert!(!base_edited(&same, base));
        let head = ProfileParts {
            head: "Início editado".into(),
            ..same.clone()
        };
        let tail = ProfileParts {
            tail: "Fim editado\n".into(),
            ..same
        };
        assert!(base_edited(&head, base));
        assert!(base_edited(&tail, base));
    }

    #[test]
    fn profile_text_is_trimmed_with_a_final_newline() {
        assert_eq!(
            profile_text("\n  olhe as migrações  \n\n"),
            "olhe as migrações\n"
        );
    }

    #[test]
    fn base_choices_keep_plain_templates_of_the_same_kind() {
        let current = template("perfil", TemplateKind::Review, Some("review-padrao"));
        let templates = [
            template("review-padrao", TemplateKind::Review, None),
            template("outro-perfil", TemplateKind::Review, Some("review-padrao")),
            template("plan-padrao", TemplateKind::Plan, None),
            current.clone(),
        ];
        let ids: Vec<_> = base_choices(&templates, &current)
            .into_iter()
            .map(|choice| choice.id)
            .collect();
        assert_eq!(ids, ["review-padrao"]);
    }

    #[test]
    fn profiles_of_lists_every_profile_of_a_base() {
        let templates = [
            template("review-padrao", TemplateKind::Review, None),
            template("api", TemplateKind::Review, Some("review-padrao")),
            template("web", TemplateKind::Review, Some("review-padrao")),
            template("sec", TemplateKind::Review, Some("seguranca")),
        ];
        assert_eq!(
            join_names(profiles_of(&templates, "review-padrao")),
            "API, WEB"
        );
        assert_eq!(join_names(profiles_of(&templates, "performance")), "");
    }
}

use std::collections::{BTreeMap, BTreeSet};

use argon_core::{PromptTemplate, TemplateKind, repo_key};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, h_flex};
use gpui_kit::{
    App, AppContext as _, Context, Div, Entity, ParentElement as _, Styled as _, Subscription,
    Window, div,
};

use super::TemplatesSection;
use crate::agents::Choice;
use crate::config_watch;
use crate::inbox::InboxModel;
use crate::settings::labels;
use crate::settings::ui::{card, mono, note};

pub(super) struct RepoPicker {
    select: Entity<SelectState<Vec<Choice>>>,
    available: usize,
}

impl RepoPicker {
    pub(super) fn new(window: &mut Window, cx: &mut Context<TemplatesSection>) -> Self {
        Self {
            select: cx.new(|cx| SelectState::new(Vec::<Choice>::new(), None, window, cx)),
            available: 0,
        }
    }

    pub(super) fn subscription(
        &self,
        window: &mut Window,
        cx: &mut Context<TemplatesSection>,
    ) -> Subscription {
        cx.subscribe_in(
            &self.select,
            window,
            |this, _, event: &SelectEvent<Vec<Choice>>, window, cx| {
                if let SelectEvent::Confirm(Some(repo)) = event {
                    this.add_repo(repo.clone(), window, cx);
                }
            },
        )
    }
}

pub fn repo_choices(
    known: impl IntoIterator<Item = String>,
    mapped: &BTreeMap<String, String>,
    template_id: &str,
    templates: &[PromptTemplate],
) -> Vec<Choice> {
    known
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|repo| match mapped.get(&repo) {
            Some(id) if id == template_id => None,
            Some(id) => {
                let name = templates
                    .iter()
                    .find(|template| &template.id == id)
                    .map_or(id.as_str(), |template| template.name.as_str());
                let label = labels::repo_elsewhere(&repo, name);
                Some(Choice::new(repo, label))
            }
            None => Some(Choice::new(repo.clone(), repo)),
        })
        .collect()
}

fn known_repos(cx: &App) -> Vec<String> {
    InboxModel::global(cx)
        .map(|inbox| {
            inbox
                .read(cx)
                .items()
                .iter()
                .map(|item| repo_key(item.subject.host(), item.subject.repo()))
                .collect()
        })
        .unwrap_or_default()
}

impl TemplatesSection {
    fn review_template_id(&self) -> Option<String> {
        self.current()
            .filter(|template| template.kind == TemplateKind::Review)
            .map(|template| template.id.clone())
    }

    pub(super) fn refresh_repos(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let choices = self
            .review_template_id()
            .map(|id| {
                repo_choices(
                    known_repos(cx),
                    &self.config.review.repo_templates,
                    &id,
                    self.saved_templates(),
                )
            })
            .unwrap_or_default();
        self.repo_picker.available = choices.len();
        self.repo_picker.select.update(cx, |select, cx| {
            select.set_items(choices, window, cx);
            select.set_selected_index(None, window, cx);
        });
        cx.notify();
    }

    fn add_repo(&mut self, repo: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.review_template_id() else {
            return;
        };
        self.config = config_watch::update(cx, |config| {
            config.review.repo_templates.insert(repo, id);
        });
        self.refresh_repos(window, cx);
    }

    fn remove_repo(&mut self, repo: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.config = config_watch::update(cx, |config| {
            config.review.repo_templates.remove(repo);
        });
        self.refresh_repos(window, cx);
    }

    pub(super) fn repos_card(&self, cx: &Context<Self>) -> Option<Div> {
        let id = self.review_template_id()?;
        let theme = cx.theme();
        let chips: Vec<Div> = self
            .config
            .review
            .repos_using(&id)
            .into_iter()
            .enumerate()
            .map(|(ix, repo)| {
                let remove = Button::new(("settings-repo-remove", ix))
                    .icon(Icon::new(IconName::X))
                    .xsmall()
                    .ghost()
                    .on_click(cx.listener({
                        let repo = repo.clone();
                        move |this, _, window, cx| this.remove_repo(&repo, window, cx)
                    }));
                h_flex()
                    .gap_0p5()
                    .pl_2()
                    .items_center()
                    .rounded(theme.radius)
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.muted.opacity(0.4))
                    .child(mono(repo, cx))
                    .child(remove)
            })
            .collect();
        let linked = if chips.is_empty() {
            note(labels::REPOSITORIES_EMPTY, theme.muted_foreground)
        } else {
            h_flex().flex_wrap().gap_1().children(chips)
        };
        let add = Select::new(&self.repo_picker.select)
            .small()
            .placeholder(labels::ADD_REPOSITORY)
            .disabled(self.repo_picker.available == 0);
        Some(
            card(cx)
                .child(div().text_sm().font_semibold().child(labels::REPOSITORIES))
                .child(linked)
                .child(div().w_72().child(add)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template(id: &str, name: &str) -> PromptTemplate {
        PromptTemplate {
            id: id.into(),
            name: name.into(),
            ..PromptTemplate::default()
        }
    }

    #[test]
    fn repo_choices_are_distinct_sorted_and_skip_repos_already_linked_here() {
        let templates = [
            template("perfil-api", "Perfil API"),
            template("seguranca", "Segurança"),
        ];
        let mapped = BTreeMap::from([
            ("github.com/acme/api".to_owned(), "perfil-api".to_owned()),
            ("github.com/acme/web".to_owned(), "seguranca".to_owned()),
            ("github.com/acme/old".to_owned(), "apagado".to_owned()),
        ]);
        let known = [
            "github.com/acme/web",
            "github.com/acme/zeta",
            "github.com/acme/api",
            "github.com/acme/old",
            "github.com/acme/zeta",
        ]
        .map(str::to_owned);
        let choices = repo_choices(known, &mapped, "perfil-api", &templates);
        assert_eq!(
            choices,
            [
                Choice::new("github.com/acme/old", "github.com/acme/old · hoje: apagado"),
                Choice::new(
                    "github.com/acme/web",
                    "github.com/acme/web · hoje: Segurança"
                ),
                Choice::new("github.com/acme/zeta", "github.com/acme/zeta"),
            ]
        );
    }
}

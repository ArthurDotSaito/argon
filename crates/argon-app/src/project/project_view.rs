use std::rc::Rc;

use argon_agents::defaults::agent_defaults;
use argon_core::{RepoKey, repo_key};
use argon_store::{ProjectOverview, Store};
use chrono::Local;
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Div, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, ScrollHandle, SharedString,
    Stateful, StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window, div, px,
};

use super::labels::{self, OriginLine};
use super::related::{self, RelatedError};
use super::repo_url;
use crate::agents::agent_markdown;
use crate::config_watch::{self, ConfigChanged};
use crate::pr;
use crate::review::{DraftsBus, DraftsChanged};
use crate::services::{AppServices, flatten};
use crate::ui::arch_diagram;

const READING_WIDTH: f32 = 760.;
const NOTE_WIDTH: f32 = 420.;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum ProjectTab {
    #[default]
    Overview,
    Architecture,
}

impl ProjectTab {
    fn id(self) -> &'static str {
        match self {
            Self::Overview => "project-tab-overview",
            Self::Architecture => "project-tab-architecture",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Overview => labels::TAB_OVERVIEW,
            Self::Architecture => labels::TAB_ARCHITECTURE,
        }
    }

    fn icon(self) -> IconName {
        match self {
            Self::Overview => IconName::BookOpen,
            Self::Architecture => IconName::Network,
        }
    }
}

pub(super) struct SavedOverview {
    overview: ProjectOverview,
    agent: Option<String>,
}

pub(super) enum Load {
    Loading,
    Failed(SharedString),
    Ready(Option<Rc<SavedOverview>>),
}

pub struct ProjectView {
    pub(super) key: RepoKey,
    pub(super) related: Vec<String>,
    pub(super) input: Entity<InputState>,
    pub(super) input_error: Option<RelatedError>,
    tab: ProjectTab,
    load: Load,
    scroll: ScrollHandle,
    pub(super) arch_zoom: usize,
    pub(super) arch_selected: Option<String>,
    pub(super) arch_scroll: ScrollHandle,
    focus_handle: FocusHandle,
    load_task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

fn load_saved(store: &Store, host: &str, repo: &str) -> Result<Option<SavedOverview>, String> {
    let Some(overview) = store
        .project_overview(host, repo)
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let agent = store
        .session(&overview.session_id)
        .ok()
        .flatten()
        .map(|row| {
            labels::agent_label(agent_defaults(row.agent).label, row.model_label.as_deref())
        });
    Ok(Some(SavedOverview { overview, agent }))
}

impl ProjectView {
    pub fn new(key: RepoKey, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input =
            cx.new(|cx| InputState::new(window, cx).placeholder(labels::RELATED_PLACEHOLDER));
        let watch = config_watch::entity(cx);
        let drafts = DraftsBus::global(cx);
        let subscriptions = vec![
            cx.subscribe_in(&input, window, Self::on_input_event),
            cx.subscribe(&watch, |this, _, event: &ConfigChanged, cx| {
                this.related = event
                    .0
                    .review
                    .related_repos(&this.key.host, &this.key.repo)
                    .to_vec();
                cx.notify();
            }),
            cx.subscribe(&drafts, |this, _, _: &DraftsChanged, cx| this.reload(cx)),
        ];
        let related = config_watch::current(cx)
            .review
            .related_repos(&key.host, &key.repo)
            .to_vec();
        let mut this = Self {
            key,
            related,
            input,
            input_error: None,
            tab: ProjectTab::default(),
            load: Load::Loading,
            scroll: ScrollHandle::new(),
            arch_zoom: arch_diagram::DEFAULT_STEP,
            arch_selected: None,
            arch_scroll: ScrollHandle::new(),
            focus_handle: cx.focus_handle(),
            load_task: None,
            _subscriptions: subscriptions,
        };
        this.reload(cx);
        this
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        let Some(services) = cx
            .try_global::<AppServices>()
            .map(|global| global.0.clone())
        else {
            return;
        };
        let (host, repo) = (self.key.host.clone(), self.key.repo.clone());
        self.load_task = Some(cx.spawn(async move |this, cx| {
            let loaded = flatten(
                services
                    .store()
                    .call(move |store| load_saved(store, &host, &repo))
                    .await,
            );
            _ = this.update(cx, |this, cx| {
                this.load = match loaded {
                    Ok(saved) => Load::Ready(saved.map(Rc::new)),
                    Err(error) => Load::Failed(labels::load_failed(&error).into()),
                };
                cx.notify();
            });
        }));
    }

    fn saved(&self) -> Option<&Rc<SavedOverview>> {
        match &self.load {
            Load::Ready(saved) => saved.as_ref(),
            Load::Loading | Load::Failed(_) => None,
        }
    }

    fn visible_tab(&self) -> ProjectTab {
        match self.tab {
            ProjectTab::Architecture if self.saved().is_none() => ProjectTab::Overview,
            tab => tab,
        }
    }

    fn set_tab(&mut self, tab: ProjectTab, cx: &mut Context<Self>) {
        if self.tab != tab {
            self.tab = tab;
            cx.notify();
        }
    }

    pub fn show_architecture(&mut self, cx: &mut Context<Self>) {
        self.set_tab(ProjectTab::Architecture, cx);
    }

    pub(super) fn open_on_github(&self) {
        let url = repo_url(&self.key);
        if let Err(error) = open::that_detached(&url) {
            tracing::warn!(%error, url, "failed to open project in browser");
        }
    }

    fn on_input_event(
        &mut self,
        _: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::PressEnter { .. } => self.add_related(window, cx),
            InputEvent::Change if self.input_error.is_some() => {
                self.input_error = None;
                cx.notify();
            }
            _ => {}
        }
    }

    pub(super) fn add_related(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.input.read(cx).value().to_string();
        let current = self.current_related(cx);
        match related::add_related(&current, &self.key.repo, &text) {
            Ok(next) => {
                self.input_error = None;
                self.input
                    .update(cx, |input, cx| input.set_value("", window, cx));
                self.save_related(next, cx);
            }
            Err(error) => self.input_error = Some(error),
        }
        cx.notify();
    }

    pub(super) fn remove_related(&mut self, repo: &str, cx: &mut Context<Self>) {
        let next = related::remove_related(&self.current_related(cx), repo);
        self.save_related(next, cx);
        cx.notify();
    }

    fn current_related(&self, cx: &App) -> Vec<String> {
        config_watch::current(cx)
            .review
            .related_repos(&self.key.host, &self.key.repo)
            .to_vec()
    }

    fn save_related(&mut self, next: Vec<String>, cx: &mut Context<Self>) {
        let key = repo_key(&self.key.host, &self.key.repo);
        let config = config_watch::update(cx, |config| {
            related::store_related(&mut config.review.repo_related, key, next)
        });
        self.related = config
            .review
            .related_repos(&self.key.host, &self.key.repo)
            .to_vec();
    }

    fn retry(&mut self, cx: &mut Context<Self>) {
        self.load = Load::Loading;
        self.reload(cx);
        cx.notify();
    }

    fn header(&self, saved: Option<&SavedOverview>, cx: &App) -> Div {
        let theme = cx.theme();
        let crumb = h_flex()
            .gap_1p5()
            .items_center()
            .font_family(theme.mono_font_family.clone())
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(labels::repo_owner(&self.key.repo).to_string())
            .child(
                div()
                    .text_color(theme.muted_foreground.opacity(0.6))
                    .child("/"),
            )
            .child(
                div()
                    .text_color(theme.foreground)
                    .child(labels::repo_name(&self.key.repo).to_string()),
            );
        let title = h_flex()
            .gap_2()
            .items_baseline()
            .child(
                div()
                    .text_xl()
                    .font_semibold()
                    .text_color(theme.foreground)
                    .child(labels::repo_name(&self.key.repo).to_string()),
            )
            .child(
                div()
                    .text_lg()
                    .text_color(theme.muted_foreground)
                    .child(labels::PROJECT),
            );
        v_flex()
            .w_full()
            .flex_none()
            .px_4()
            .pt_3()
            .pb_3()
            .gap_2()
            .child(crumb)
            .child(title)
            .child(origin(saved, cx))
    }

    fn tab_bar(&self, saved: &SavedOverview, cx: &mut Context<Self>) -> Div {
        let selected = self.visible_tab();
        let items: Vec<Stateful<Div>> = [ProjectTab::Overview, ProjectTab::Architecture]
            .into_iter()
            .map(|tab| {
                let count = (tab == ProjectTab::Architecture)
                    .then_some(saved.overview.diagram.as_ref())
                    .flatten()
                    .map(|graph| SharedString::from(graph.nodes.len().to_string()));
                pr::tab_button(
                    tab.id(),
                    tab.icon(),
                    tab.title(),
                    count,
                    tab == selected,
                    cx,
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.set_tab(tab, cx)))
            })
            .collect();
        h_flex()
            .flex_none()
            .px_2()
            .gap_1()
            .items_end()
            .border_b_1()
            .border_color(cx.theme().border)
            .children(items)
    }

    fn overview_tab(&self, saved: &SavedOverview, cx: &App) -> Stateful<Div> {
        let content = v_flex()
            .w_full()
            .min_w_0()
            .max_w(px(READING_WIDTH))
            .px_4()
            .py_3()
            .text_sm()
            .text_color(cx.theme().foreground)
            .child(agent_markdown(
                SharedString::from("project-overview-body"),
                &saved.overview.body,
            ));
        div()
            .id("project-overview-scroll")
            .size_full()
            .track_scroll(&self.scroll)
            .overflow_y_scroll()
            .vertical_scrollbar(&self.scroll)
            .child(content)
    }

    fn empty_state(&self, cx: &App) -> Div {
        let theme = cx.theme();
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_2()
            .child(
                Icon::new(IconName::BookOpen)
                    .large()
                    .text_color(theme.muted_foreground),
            )
            .child(
                div()
                    .font_semibold()
                    .text_color(theme.foreground)
                    .child(labels::EMPTY_TITLE),
            )
            .child(
                div()
                    .max_w(px(NOTE_WIDTH))
                    .text_sm()
                    .text_center()
                    .text_color(theme.muted_foreground)
                    .child(labels::empty_explanation(&self.key.repo)),
            )
    }

    fn body(&self, cx: &mut Context<Self>) -> AnyElement {
        match &self.load {
            Load::Loading => pr::skeleton().into_any_element(),
            Load::Failed(message) => {
                let retry = pr::retry_button("project-retry")
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.retry(cx)));
                pr::failure_panel(message.clone(), retry, cx).into_any_element()
            }
            Load::Ready(None) => self.empty_state(cx).into_any_element(),
            Load::Ready(Some(saved)) => match self.visible_tab() {
                ProjectTab::Overview => self.overview_tab(saved, cx).into_any_element(),
                ProjectTab::Architecture => {
                    self.architecture_tab(saved.overview.diagram.as_ref(), cx)
                }
            },
        }
    }
}

fn origin(saved: Option<&SavedOverview>, cx: &App) -> Div {
    let theme = cx.theme();
    let row = h_flex()
        .w_full()
        .flex_wrap()
        .gap_x_4()
        .gap_y_1()
        .items_center()
        .text_xs()
        .text_color(theme.muted_foreground);
    let Some(saved) = saved else {
        return row.child(fact(IconName::Minus, labels::NO_OVERVIEW.to_string()));
    };
    let overview = &saved.overview;
    let generated_at = overview.updated_at.with_timezone(&Local).naive_local();
    let OriginLine {
        generated,
        from_pr,
        commit,
        agent,
    } = labels::origin_line(
        generated_at,
        overview.source_pr,
        &overview.source_sha,
        saved.agent.clone(),
    );
    row.child(fact(IconName::Clock, generated))
        .child(fact(IconName::GitPullRequest, from_pr))
        .child(
            div()
                .font_family(theme.mono_font_family.clone())
                .child(commit),
        )
        .children(agent.map(|agent| fact(IconName::Bot, agent)))
}

fn fact(icon: IconName, text: String) -> Div {
    h_flex()
        .gap_1()
        .items_center()
        .child(Icon::new(icon).xsmall())
        .child(text)
}

impl Focusable for ProjectView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ProjectView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let saved = self.saved().cloned();
        let head = self.header(saved.as_deref(), cx);
        let tabs = saved.as_deref().map(|saved| self.tab_bar(saved, cx));
        let body = self.body(cx);
        let side = (self.visible_tab() == ProjectTab::Overview).then(|| self.side_panel(cx));
        let theme = cx.theme();
        let main = v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(head)
            .children(tabs)
            .child(div().flex_1().min_h_0().child(body));
        h_flex()
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(theme.background)
            .child(main)
            .children(side)
    }
}

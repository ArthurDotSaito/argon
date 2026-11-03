mod about;
mod accounts;
mod agents;
mod appearance;
mod labels;
mod orgs;
mod recent_prs;
mod templates;
mod ui;

use gpui_kit::assets::IconName;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Div, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, ScrollHandle,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};

use self::about::AboutSection;
use self::accounts::AccountsSection;
use self::agents::AgentsSection;
use self::orgs::OrgsSection;
use self::templates::TemplatesSection;
use crate::config_watch::{self, ConfigChanged, ConfigWatch};

pub use recent_prs::note_opened as note_pr_opened;

const NAV_WIDTH: f32 = 184.;
const CONTENT_MAX_WIDTH: f32 = 1100.;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Section {
    Accounts,
    Orgs,
    Agents,
    Templates,
    Notifications,
    Appearance,
    About,
}

impl Section {
    const ALL: [Self; 7] = [
        Self::Accounts,
        Self::Orgs,
        Self::Agents,
        Self::Templates,
        Self::Notifications,
        Self::Appearance,
        Self::About,
    ];

    fn title(self) -> &'static str {
        match self {
            Self::Accounts => labels::ACCOUNTS,
            Self::Orgs => labels::ORGS,
            Self::Agents => labels::AGENTS,
            Self::Templates => labels::TEMPLATES,
            Self::Notifications => labels::NOTIFICATIONS,
            Self::Appearance => labels::APPEARANCE,
            Self::About => labels::ABOUT,
        }
    }

    fn icon(self) -> IconName {
        match self {
            Self::Accounts => IconName::Users,
            Self::Orgs => IconName::Building2,
            Self::Agents => IconName::Bot,
            Self::Templates => IconName::FileText,
            Self::Notifications => IconName::Bell,
            Self::Appearance => IconName::Palette,
            Self::About => IconName::Info,
        }
    }
}

pub struct SettingsView {
    section: Section,
    focus_handle: FocusHandle,
    scroll: ScrollHandle,
    accounts: Option<Entity<AccountsSection>>,
    orgs: Option<Entity<OrgsSection>>,
    agents: Option<Entity<AgentsSection>>,
    templates: Option<Entity<TemplatesSection>>,
    about: Option<Entity<AboutSection>>,
    _subscription: Subscription,
}

impl SettingsView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let watch = config_watch::entity(cx);
        let subscription = cx.subscribe_in(&watch, window, Self::on_config_changed);
        Self {
            section: Section::Accounts,
            focus_handle: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            accounts: None,
            orgs: None,
            agents: None,
            templates: None,
            about: None,
            _subscription: subscription,
        }
    }

    fn on_config_changed(
        &mut self,
        _: &Entity<ConfigWatch>,
        event: &ConfigChanged,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let config = &event.0;
        if let Some(orgs) = &self.orgs {
            orgs.update(cx, |orgs, cx| orgs.config_changed(config, cx));
        }
        if let Some(templates) = &self.templates {
            templates.update(cx, |templates, cx| {
                templates.config_changed(config, window, cx)
            });
        }
        cx.notify();
    }

    pub fn show_about(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show(Section::About, window, cx);
    }

    fn show(&mut self, section: Section, window: &mut Window, cx: &mut Context<Self>) {
        self.section = section;
        if section == Section::Templates
            && let Some(templates) = &self.templates
        {
            templates.update(cx, |templates, cx| templates.shown(window, cx));
        }
        cx.notify();
    }

    fn content(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        match self.section {
            Section::Accounts => self
                .accounts
                .get_or_insert_with(|| cx.new(AccountsSection::new))
                .clone()
                .into_any_element(),
            Section::Orgs => self
                .orgs
                .get_or_insert_with(|| cx.new(OrgsSection::new))
                .clone()
                .into_any_element(),
            Section::Agents => self
                .agents
                .get_or_insert_with(|| cx.new(|cx| AgentsSection::new(window, cx)))
                .clone()
                .into_any_element(),
            Section::Templates => self
                .templates
                .get_or_insert_with(|| cx.new(|cx| TemplatesSection::new(window, cx)))
                .clone()
                .into_any_element(),
            Section::Notifications => appearance::notifications(cx).into_any_element(),
            Section::Appearance => appearance::appearance(cx).into_any_element(),
            Section::About => self
                .about
                .get_or_insert_with(|| cx.new(AboutSection::new))
                .clone()
                .into_any_element(),
        }
    }

    fn nav(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let items: Vec<_> = Section::ALL
            .into_iter()
            .enumerate()
            .map(|(ix, section)| {
                let active = section == self.section;
                h_flex()
                    .id(("settings-nav", ix))
                    .w_full()
                    .h(px(30.))
                    .px_2()
                    .gap_2()
                    .items_center()
                    .rounded(theme.radius)
                    .text_sm()
                    .cursor_pointer()
                    .text_color(if active {
                        theme.foreground
                    } else {
                        theme.muted_foreground
                    })
                    .when(active, |item| item.bg(theme.muted))
                    .hover(|style| style.bg(theme.muted.opacity(0.6)))
                    .child(Icon::new(section.icon()).small())
                    .child(section.title())
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.show(section, window, cx)
                    }))
            })
            .collect();
        v_flex()
            .flex_none()
            .w(px(NAV_WIDTH))
            .h_full()
            .p_2()
            .gap_0p5()
            .border_r_1()
            .border_color(theme.border)
            .children(items)
    }
}

impl Focusable for SettingsView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = self.content(window, cx);
        let nav = self.nav(cx);
        h_flex()
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(cx.theme().background)
            .child(nav)
            .child(
                div()
                    .id("settings-content")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .track_scroll(&self.scroll)
                    .overflow_y_scroll()
                    .vertical_scrollbar(&self.scroll)
                    .child(
                        div()
                            .w_full()
                            .max_w(px(CONTENT_MAX_WIDTH))
                            .p_5()
                            .child(content),
                    ),
            )
    }
}

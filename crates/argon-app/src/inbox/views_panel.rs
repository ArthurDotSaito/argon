use gpui_kit::assets::IconName;
use gpui_kit::component::dock::{BasePanel, Panel, PanelControl, PanelEvent};
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::{
    App, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, Stateful,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};

use super::items::{AccountEntry, OrgEntry};
use super::model::InboxModel;
use super::{InboxView, labels};
use crate::ui::avatar;

pub const VIEWS_PANEL: &str = "argon.views";

const ITEM_HEIGHT: f32 = 26.;
const ORG_BADGE_SIZE: f32 = 16.;
const ACCOUNT_AVATAR_SIZE: f32 = 22.;

const GROUPS: [(&str, &[InboxView]); 3] = [
    (
        labels::GROUP_INBOX,
        &[InboxView::Inbox, InboxView::NeedsYou],
    ),
    (
        labels::GROUP_YOUR_PRS,
        &[
            InboxView::YourPrs,
            InboxView::WaitingOnOthers,
            InboxView::ChangesRequested,
            InboxView::ReadyToMerge,
        ],
    ),
    (labels::GROUP_REVIEWED, &[InboxView::AlreadyDone]),
];

pub struct ViewsPanel {
    model: Entity<InboxModel>,
    focus_handle: FocusHandle,
    _subscription: Subscription,
}

impl ViewsPanel {
    pub fn new(model: Entity<InboxModel>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&model, |_, _, cx| cx.notify());
        Self {
            model,
            focus_handle: cx.focus_handle(),
            _subscription: subscription,
        }
    }

    fn view_item(&self, view: InboxView, cx: &App) -> Stateful<Div> {
        let theme = cx.theme();
        let model = self.model.read(cx);
        let count = model.count(view);
        let highlighted = view == InboxView::NeedsYou && count > 0;
        let counter = div()
            .px_1()
            .rounded(theme.radius)
            .text_xs()
            .when(highlighted, |this| {
                this.bg(theme.primary)
                    .text_color(theme.primary_foreground)
                    .font_semibold()
            })
            .when(!highlighted, |this| this.text_color(theme.muted_foreground))
            .child(count.to_string());
        let inbox = self.model.downgrade();
        nav_item(("view", view.index()), model.view() == view, cx)
            .child(Icon::new(view_icon(view)).small())
            .child(div().flex_1().truncate().child(labels::view_title(view)))
            .child(counter)
            .on_click(move |_, _, cx| {
                _ = inbox.update(cx, |model, cx| model.show_view(view, cx));
            })
    }

    fn org_item(&self, ix: usize, org: &OrgEntry, cx: &App) -> Stateful<Div> {
        let theme = cx.theme();
        let selected = self.model.read(cx).is_org_selected(&org.key);
        let badge = avatar(&org.name, ORG_BADGE_SIZE).rounded(px(3.));
        let admin = org.admin.then(|| {
            div()
                .px_1()
                .rounded(theme.radius)
                .border_1()
                .border_color(theme.border)
                .text_size(px(10.))
                .text_color(theme.muted_foreground)
                .child(labels::ADMIN_BADGE)
        });
        let inbox = self.model.downgrade();
        let key = org.key.clone();
        nav_item(("org", ix), selected, cx)
            .child(badge)
            .child(div().flex_1().truncate().child(org.name.clone()))
            .children(admin)
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(org.count.to_string()),
            )
            .on_click(move |_, _, cx| {
                _ = inbox.update(cx, |model, cx| model.toggle_org(key.clone(), cx));
            })
    }

    fn orgs(&self, cx: &App) -> Div {
        let orgs = self.model.read(cx).orgs();
        let group = group(labels::GROUP_ORGS, cx);
        if orgs.is_empty() {
            return group.child(hint(labels::NO_ORGS, cx));
        }
        group.children(
            orgs.iter()
                .enumerate()
                .map(|(ix, org)| self.org_item(ix, org, cx)),
        )
    }

    fn accounts(&self, cx: &App) -> Div {
        let theme = cx.theme();
        let accounts = self.model.read(cx).accounts();
        let footer = group(labels::GROUP_ACCOUNTS, cx)
            .pt_2()
            .border_t_1()
            .border_color(theme.border);
        if accounts.is_empty() {
            return footer.child(hint(labels::NO_ACCOUNTS, cx));
        }
        footer.children(accounts.iter().map(|account| account_item(account, cx)))
    }
}

fn view_icon(view: InboxView) -> IconName {
    match view {
        InboxView::Inbox => IconName::Inbox,
        InboxView::NeedsYou => IconName::CircleDot,
        InboxView::YourPrs => IconName::GitPullRequest,
        InboxView::WaitingOnOthers => IconName::Clock,
        InboxView::ChangesRequested => IconName::FileDiff,
        InboxView::ReadyToMerge => IconName::GitMerge,
        InboxView::AlreadyDone => IconName::CheckCheck,
    }
}

fn nav_item(id: (&'static str, usize), selected: bool, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    h_flex()
        .id(id)
        .h(px(ITEM_HEIGHT))
        .px_2()
        .gap_2()
        .items_center()
        .rounded(theme.radius)
        .cursor_pointer()
        .text_color(theme.foreground)
        .when(selected, |this| this.bg(theme.list_active))
        .when(!selected, |this| {
            this.hover(|style| style.bg(theme.list_hover))
        })
}

fn group(title: &'static str, cx: &App) -> Div {
    v_flex().gap_0p5().child(
        div()
            .px_2()
            .pb_0p5()
            .text_size(px(11.))
            .font_semibold()
            .text_color(cx.theme().muted_foreground)
            .child(SharedString::from(title.to_uppercase())),
    )
}

fn hint(text: &'static str, cx: &App) -> Div {
    div()
        .px_2()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text)
}

fn account_item(account: &AccountEntry, cx: &App) -> Div {
    let theme = cx.theme();
    let login = account.login.trim_start_matches('@').to_string();
    h_flex()
        .px_2()
        .gap_2()
        .items_center()
        .child(avatar(&login, ACCOUNT_AVATAR_SIZE))
        .child(
            v_flex()
                .min_w_0()
                .child(
                    div()
                        .truncate()
                        .text_color(theme.foreground)
                        .child(account.login.clone()),
                )
                .child(
                    div()
                        .truncate()
                        .font_family(theme.mono_font_family.clone())
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(account.host.clone()),
                ),
        )
}

impl EventEmitter<PanelEvent> for ViewsPanel {}

impl Focusable for ViewsPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BasePanel for ViewsPanel {
    fn panel_name(&self) -> &'static str {
        VIEWS_PANEL
    }

    fn closable(&self, _: &App) -> bool {
        false
    }

    fn zoomable(&self, _: &App) -> bool {
        false
    }
}

impl Panel for ViewsPanel {
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        labels::PANEL_TITLE
    }

    fn zoom_control(&self, _: &App) -> Option<PanelControl> {
        None
    }
}

impl Render for ViewsPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let groups = GROUPS.iter().map(|(title, views)| {
            group(title, cx).children(views.iter().map(|view| self.view_item(*view, cx)))
        });
        v_flex()
            .track_focus(&self.focus_handle)
            .size_full()
            .child(
                v_flex()
                    .id("views-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_2()
                    .gap_3()
                    .children(groups)
                    .child(self.orgs(cx)),
            )
            .child(div().p_2().child(self.accounts(cx)))
    }
}

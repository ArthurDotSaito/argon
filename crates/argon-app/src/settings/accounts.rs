use argon_github::{Account, discover_accounts, token_for};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::clipboard::Clipboard;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::{
    App, Context, Div, IntoElement, ParentElement as _, Render, SharedString, Styled as _, Task,
    Window, div,
};

use super::labels;
use super::ui::{card, mono, note, section, services};

#[derive(Debug, Clone, PartialEq, Eq)]
enum HostState {
    Checking,
    Connected,
    Authenticated,
    Missing,
}

#[derive(Debug, Clone)]
struct HostRow {
    host: String,
    login: String,
    state: HostState,
}

enum Load {
    Loading,
    Ready(Vec<HostRow>),
    Failed(SharedString),
}

pub struct AccountsSection {
    hosts: Load,
    task: Option<Task<()>>,
}

impl AccountsSection {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            hosts: Load::Loading,
            task: None,
        };
        this.refresh(cx);
        this
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        let Some(services) = services(cx) else {
            self.hosts = Load::Failed(labels::SERVICES_UNAVAILABLE.into());
            return;
        };
        let connected = services.accounts();
        let job = services.spawn_runtime(async move {
            let discovered = discover_accounts().map_err(|error| error.to_string())?;
            let mut rows = Vec::new();
            for account in discovered {
                let mut row = host_row(account, &connected);
                if row.state == HostState::Checking {
                    row.state = if token_for(&row.host).await.is_ok() {
                        HostState::Authenticated
                    } else {
                        HostState::Missing
                    };
                }
                rows.push(row);
            }
            Ok::<_, String>(rows)
        });
        self.task = Some(cx.spawn(async move |this, cx| {
            let result = job
                .await
                .map_err(|error| error.to_string())
                .and_then(|rows| rows);
            _ = this.update(cx, |this, cx| {
                this.hosts = match result {
                    Ok(rows) => Load::Ready(rows),
                    Err(error) => Load::Failed(labels::accounts_failed(&error).into()),
                };
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn row(&self, ix: usize, row: &HostRow, cx: &App) -> Div {
        let theme = cx.theme();
        let (icon, color, status): (IconName, _, SharedString) = match row.state {
            HostState::Checking => (
                IconName::LoaderCircle,
                theme.muted_foreground,
                labels::CHECKING.into(),
            ),
            HostState::Connected => (
                IconName::CircleCheck,
                theme.success,
                labels::CONNECTED.into(),
            ),
            HostState::Authenticated => (
                IconName::CircleCheck,
                theme.muted_foreground,
                labels::AUTHENTICATED.into(),
            ),
            HostState::Missing => (
                IconName::CircleAlert,
                theme.warning,
                labels::not_authenticated(&row.host).into(),
            ),
        };
        let copy = (row.state == HostState::Missing).then(|| {
            Clipboard::new(("settings-copy-login", ix))
                .value(SharedString::from(labels::gh_login(&row.host)))
                .xsmall()
        });
        h_flex()
            .w_full()
            .gap_2()
            .items_center()
            .child(Icon::new(icon).small().text_color(color))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        h_flex()
                            .gap_1p5()
                            .child(div().text_sm().font_semibold().child(row.host.clone()))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme.muted_foreground)
                                    .child(format!("@{}", row.login)),
                            ),
                    )
                    .child(note(status, color)),
            )
            .children(copy)
    }
}

fn host_row(account: Account, connected: &[Account]) -> HostRow {
    let is_connected = connected
        .iter()
        .any(|known| known.host.eq_ignore_ascii_case(&account.host));
    HostRow {
        state: if is_connected {
            HostState::Connected
        } else {
            HostState::Checking
        },
        host: account.host,
        login: account.login,
    }
}

impl Render for AccountsSection {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let body: Div = match &self.hosts {
            Load::Loading => card(cx).child(note(labels::CHECKING, theme.muted_foreground)),
            Load::Failed(error) => card(cx).child(note(error.clone(), theme.danger)),
            Load::Ready(rows) if rows.is_empty() => card(cx)
                .child(note(labels::NO_ACCOUNTS, theme.warning))
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(mono(labels::LOGIN_FIRST, cx))
                        .child(
                            Clipboard::new("settings-copy-first-login")
                                .value(SharedString::from(labels::LOGIN_FIRST))
                                .xsmall(),
                        ),
                ),
            Load::Ready(rows) => card(cx).children(
                rows.iter()
                    .enumerate()
                    .map(|(ix, row)| self.row(ix, row, cx)),
            ),
        };
        let recheck = Button::new("settings-accounts-recheck")
            .icon(Icon::new(IconName::RefreshCw))
            .label(labels::RECHECK)
            .xsmall()
            .ghost()
            .on_click(cx.listener(|this, _, _, cx| this.refresh(cx)));
        section(labels::ACCOUNTS, labels::ACCOUNTS_HINT, cx)
            .child(body)
            .child(h_flex().child(recheck))
    }
}

use argon_core::Config;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Icon, Sizable as _, h_flex};
use gpui_kit::{
    Context, ElementId, IntoElement, ParentElement as _, Render, Styled as _, Window, div,
};

use super::labels;
use super::ui::{badge, card, note, section, services};
use crate::config_watch;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownOrg {
    pub key: String,
    pub admin: bool,
}

pub fn known_orgs(admin: &[String], tracked: &[String]) -> Vec<KnownOrg> {
    let mut orgs: Vec<KnownOrg> = Vec::new();
    for key in admin.iter().chain(tracked) {
        let is_admin = admin.iter().any(|entry| entry.eq_ignore_ascii_case(key));
        match orgs
            .iter_mut()
            .find(|org| org.key.eq_ignore_ascii_case(key))
        {
            Some(org) => org.admin |= is_admin,
            None => orgs.push(KnownOrg {
                key: key.clone(),
                admin: is_admin,
            }),
        }
    }
    orgs.sort_by_key(|org| org.key.to_lowercase());
    orgs
}

fn is_tracked(tracked: &[String], key: &str) -> bool {
    tracked.iter().any(|entry| entry.eq_ignore_ascii_case(key))
}

pub struct OrgsSection {
    orgs: Vec<KnownOrg>,
    saved: Vec<String>,
    draft: Vec<String>,
}

impl OrgsSection {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            orgs: Vec::new(),
            saved: Vec::new(),
            draft: Vec::new(),
        };
        this.reload(&config_watch::current(cx), cx);
        this
    }

    fn reload(&mut self, config: &Config, cx: &mut Context<Self>) {
        let admin = services(cx).map(|s| s.admin_orgs()).unwrap_or_default();
        self.orgs = known_orgs(&admin, &config.orgs.tracked);
        self.saved = config.orgs.tracked.clone();
        self.draft = self.saved.clone();
        cx.notify();
    }

    pub fn config_changed(&mut self, config: &Config, cx: &mut Context<Self>) {
        if !self.is_dirty() {
            self.reload(config, cx);
        }
    }

    fn is_dirty(&self) -> bool {
        self.orgs
            .iter()
            .any(|org| is_tracked(&self.saved, &org.key) != is_tracked(&self.draft, &org.key))
    }

    fn toggle(&mut self, key: &str, on: bool, cx: &mut Context<Self>) {
        self.draft.retain(|entry| !entry.eq_ignore_ascii_case(key));
        if on {
            self.draft.push(key.to_owned());
        }
        cx.notify();
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let tracked: Vec<String> = self
            .orgs
            .iter()
            .filter(|org| is_tracked(&self.draft, &org.key))
            .map(|org| org.key.clone())
            .collect();
        let config = config_watch::update(cx, |config| config.orgs.tracked = tracked);
        if let Some(services) = services(cx) {
            services.sync_now();
        }
        self.reload(&config, cx);
    }

    fn discard(&mut self, cx: &mut Context<Self>) {
        self.draft = self.saved.clone();
        cx.notify();
    }
}

impl Render for OrgsSection {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let rows: Vec<_> = self
            .orgs
            .iter()
            .enumerate()
            .map(|(ix, org)| {
                let key = org.key.clone();
                let admin = org.admin.then(|| badge(labels::ADMIN, theme.primary));
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        Checkbox::new(ElementId::NamedInteger("settings-org".into(), ix as u64))
                            .label(org.key.clone())
                            .checked(is_tracked(&self.draft, &org.key))
                            .small()
                            .on_click(cx.listener(move |this, on: &bool, _, cx| {
                                this.toggle(&key, *on, cx)
                            })),
                    )
                    .children(admin)
            })
            .collect();
        let dirty = self.is_dirty();
        let body = if rows.is_empty() {
            card(cx).child(note(labels::NO_ORGS, theme.muted_foreground))
        } else {
            card(cx).children(rows)
        };
        let actions = h_flex()
            .gap_2()
            .items_center()
            .child(
                Button::new("settings-orgs-save")
                    .icon(Icon::new(IconName::RefreshCw))
                    .label(labels::SAVE_AND_SYNC)
                    .small()
                    .primary()
                    .disabled(!dirty)
                    .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
            )
            .child(
                Button::new("settings-orgs-discard")
                    .label(labels::DISCARD)
                    .small()
                    .ghost()
                    .disabled(!dirty)
                    .on_click(cx.listener(|this, _, _, cx| this.discard(cx))),
            )
            .children(dirty.then(|| div().child(note(labels::UNSAVED, theme.warning))));
        section(labels::ORGS, labels::ORGS_HINT, cx)
            .child(body)
            .child(actions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_orgs_merge_admin_and_tracked_ignoring_case() {
        let admin = vec!["github.com/nuria-tech".to_owned()];
        let tracked = vec![
            "github.com/Nuria-Tech".to_owned(),
            "ghe.example.com/platform".to_owned(),
        ];
        let orgs = known_orgs(&admin, &tracked);
        assert_eq!(
            orgs,
            vec![
                KnownOrg {
                    key: "ghe.example.com/platform".into(),
                    admin: false
                },
                KnownOrg {
                    key: "github.com/nuria-tech".into(),
                    admin: true
                },
            ]
        );
        assert!(is_tracked(&tracked, "GITHUB.COM/nuria-tech"));
    }
}

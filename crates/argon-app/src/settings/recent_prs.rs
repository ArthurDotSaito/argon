use std::collections::HashSet;

use argon_core::{PrKey, PullRequest};
use gpui_kit::{App, Entity, Global, WeakEntity};

use crate::inbox::InboxModel;
use crate::pr::PrView;

#[derive(Default)]
struct RecentPrs(Vec<(PrKey, WeakEntity<PrView>)>);

impl Global for RecentPrs {}

pub fn note_opened(key: PrKey, view: &Entity<PrView>, cx: &mut App) {
    let recent = cx.default_global::<RecentPrs>();
    recent.0.retain(|(_, weak)| weak.upgrade().is_some());
    recent.0.push((key, view.downgrade()));
}

pub fn open_prs(cx: &App) -> Vec<PullRequest> {
    let (Some(recent), Some(inbox)) = (cx.try_global::<RecentPrs>(), InboxModel::global(cx)) else {
        return Vec::new();
    };
    let inbox = inbox.read(cx);
    let mut seen = HashSet::new();
    recent
        .0
        .iter()
        .rev()
        .filter(|(_, weak)| weak.upgrade().is_some())
        .filter(|(key, _)| seen.insert(key.clone()))
        .filter_map(|(key, _)| inbox.pr(key).cloned())
        .collect()
}

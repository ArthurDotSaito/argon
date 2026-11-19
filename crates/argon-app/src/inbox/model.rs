use std::collections::HashMap;

use argon_core::{PrKey, PullRequest, ViewSet};
use argon_store::Store;
use chrono::{Local, Utc};
use gpui_kit::{App, Context, Entity, EventEmitter, Global, SharedString, Subscription, Task};

use super::InboxView;
use super::items::{AccountEntry, InboxItem, LoadContext, OrgEntry, OrgKey, load_items};
use super::labels;
use super::sections::{InboxRow, Sections};
use crate::services::{Services, SyncEvent, flatten};
use crate::sync_status::SyncStatus;

pub enum InboxEvent {
    OpenPr { key: PrKey, preview: bool },
    ShowList,
}

pub struct InboxHandle(pub Entity<InboxModel>);

impl Global for InboxHandle {}

pub struct InboxModel {
    services: Services,
    items: Vec<InboxItem>,
    index: HashMap<PrKey, usize>,
    counts: [usize; InboxView::ALL.len()],
    orgs: Vec<OrgEntry>,
    accounts: Vec<AccountEntry>,
    view: InboxView,
    org: Option<OrgKey>,
    query: String,
    sections: Sections,
    selected: usize,
    scroll_generation: u64,
    rows_revision: u64,
    loaded: bool,
    load_error: Option<SharedString>,
    action_error: Option<SharedString>,
    load_task: Option<Task<()>>,
    local_epoch: u64,
    orgs_with_more: Vec<OrgKey>,
    awaiting_more: Option<u64>,
    _sync_subscription: Subscription,
}

impl InboxModel {
    pub fn new(
        services: Services,
        sync_status: &Entity<SyncStatus>,
        cx: &mut Context<Self>,
    ) -> Self {
        let sync_subscription =
            cx.subscribe(sync_status, |this, _, event: &SyncEvent, cx| match event {
                SyncEvent::Partial | SyncEvent::Finished { .. } => this.reload(cx),
                SyncEvent::Failed { .. } | SyncEvent::AuthMissing { .. } => {
                    this.stop_loading_more(cx)
                }
                SyncEvent::Started => {}
            });
        let mut this = Self {
            services,
            items: Vec::new(),
            index: HashMap::new(),
            counts: [0; InboxView::ALL.len()],
            orgs: Vec::new(),
            accounts: Vec::new(),
            view: InboxView::Inbox,
            org: None,
            query: String::new(),
            sections: Sections::default(),
            selected: 0,
            scroll_generation: 0,
            rows_revision: 0,
            loaded: false,
            load_error: None,
            action_error: None,
            load_task: None,
            local_epoch: 0,
            orgs_with_more: Vec::new(),
            awaiting_more: None,
            _sync_subscription: sync_subscription,
        };
        this.refresh_meta();
        this.reload(cx);
        this
    }

    pub fn global(cx: &App) -> Option<Entity<InboxModel>> {
        cx.try_global::<InboxHandle>()
            .map(|handle| handle.0.clone())
    }

    pub fn view(&self) -> InboxView {
        self.view
    }

    pub fn count(&self, view: InboxView) -> usize {
        self.counts[view.index()]
    }

    pub fn orgs(&self) -> &[OrgEntry] {
        &self.orgs
    }

    pub fn accounts(&self) -> &[AccountEntry] {
        &self.accounts
    }

    pub fn org_filter(&self) -> Option<&OrgEntry> {
        let key = self.org.as_ref()?;
        self.orgs.iter().find(|org| &org.key == key)
    }

    pub fn is_org_selected(&self, key: &OrgKey) -> bool {
        self.org.as_ref() == Some(key)
    }

    pub fn has_query(&self) -> bool {
        !self.query.is_empty()
    }

    pub fn rows(&self) -> &[InboxRow] {
        &self.sections.rows
    }

    pub fn visible_count(&self) -> usize {
        self.sections.visible.len()
    }

    pub fn items(&self) -> &[InboxItem] {
        &self.items
    }

    pub fn item_at(&self, position: usize) -> Option<&InboxItem> {
        let ix = *self.sections.visible.get(position)?;
        self.items.get(ix)
    }

    pub fn is_selected(&self, position: usize) -> bool {
        position == self.selected && position < self.visible_count()
    }

    pub fn selected_row(&self) -> Option<usize> {
        self.sections.row_of.get(self.selected).copied()
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    pub fn error(&self) -> Option<&SharedString> {
        self.action_error.as_ref().or(self.load_error.as_ref())
    }

    pub fn scroll_generation(&self) -> u64 {
        self.scroll_generation
    }

    pub fn rows_revision(&self) -> u64 {
        self.rows_revision
    }

    pub fn item(&self, key: &PrKey) -> Option<&InboxItem> {
        let ix = *self.index.get(key)?;
        self.items.get(ix)
    }

    pub fn pr(&self, key: &PrKey) -> Option<&PullRequest> {
        self.item(key).map(|item| &item.pr)
    }

    pub fn is_loading_more(&self) -> bool {
        self.awaiting_more.is_some()
    }

    pub fn load_more(&mut self, cx: &mut Context<Self>) {
        if self.awaiting_more.is_some() {
            return;
        }
        self.awaiting_more = Some(self.services.load_more_orgs());
        self.refilter(false);
        cx.notify();
    }

    fn stop_loading_more(&mut self, cx: &mut Context<Self>) {
        if self.awaiting_more.take().is_some() {
            self.refilter(false);
            cx.notify();
        }
    }

    fn refresh_more(&mut self) {
        self.orgs_with_more = self
            .services
            .orgs_with_more()
            .iter()
            .filter_map(|entry| entry.split_once('/'))
            .map(|(host, owner)| OrgKey::new(host, owner))
            .collect();
        let served = self.services.more_orgs_served();
        if self
            .awaiting_more
            .is_some_and(|generation| generation <= served)
        {
            self.awaiting_more = None;
        }
    }

    fn shows_load_more(&self) -> bool {
        if self.view != InboxView::Inbox {
            return false;
        }
        self.awaiting_more.is_some()
            || match &self.org {
                Some(org) => self.orgs_with_more.contains(org),
                None => !self.orgs_with_more.is_empty(),
            }
    }

    pub fn show_view(&mut self, view: InboxView, cx: &mut Context<Self>) {
        if self.view != view {
            self.view = view;
            self.refilter(true);
        }
        cx.emit(InboxEvent::ShowList);
        cx.notify();
    }

    pub fn toggle_org(&mut self, key: OrgKey, cx: &mut Context<Self>) {
        self.org = if self.org.as_ref() == Some(&key) {
            None
        } else {
            Some(key)
        };
        self.refilter(true);
        cx.emit(InboxEvent::ShowList);
        cx.notify();
    }

    pub fn clear_org(&mut self, cx: &mut Context<Self>) {
        if self.org.take().is_some() {
            self.refilter(true);
            cx.notify();
        }
    }

    pub fn set_query(&mut self, query: &str, cx: &mut Context<Self>) {
        let query = query.trim().to_lowercase();
        if query != self.query {
            self.query = query;
            self.refilter(true);
            cx.notify();
        }
    }

    pub fn select_next(&mut self, cx: &mut Context<Self>) {
        if self.selected + 1 < self.visible_count() {
            self.selected += 1;
            cx.notify();
        }
    }

    pub fn select_previous(&mut self, cx: &mut Context<Self>) {
        if self.selected > 0 {
            self.selected -= 1;
            cx.notify();
        }
    }

    pub fn select(&mut self, position: usize, cx: &mut Context<Self>) {
        if position < self.visible_count() && position != self.selected {
            self.selected = position;
            cx.notify();
        }
    }

    pub fn open_selected(&mut self, preview: bool, cx: &mut Context<Self>) {
        self.open(self.selected, preview, cx);
    }

    pub fn open_pr(&mut self, key: &PrKey, preview: bool, cx: &mut Context<Self>) {
        if let Some(position) = self.position_of(key) {
            self.open(position, preview, cx);
        }
    }

    fn position_of(&self, key: &PrKey) -> Option<usize> {
        self.sections
            .visible
            .iter()
            .position(|ix| self.index.get(key) == Some(ix))
    }

    pub fn open(&mut self, position: usize, preview: bool, cx: &mut Context<Self>) {
        let Some(&ix) = self.sections.visible.get(position) else {
            return;
        };
        self.selected = position;
        let item = &mut self.items[ix];
        let key = item.pr.key();
        if item.unread {
            item.unread = false;
            let (seen, updated_at) = (key.clone(), item.pr.updated_at);
            self.persist(move |store| store.mark_seen(&seen, updated_at), cx);
        }
        cx.emit(InboxEvent::OpenPr { key, preview });
        cx.notify();
    }

    pub fn archive_selected(&mut self, cx: &mut Context<Self>) {
        let Some(&ix) = self.sections.visible.get(self.selected) else {
            return;
        };
        let neighbour = [self.selected + 1, self.selected.wrapping_sub(1)]
            .into_iter()
            .find_map(|position| self.item_at(position))
            .map(|item| item.pr.key());
        let item = self.items.remove(ix);
        let (key, updated_at) = (item.pr.key(), item.pr.updated_at);
        self.reindex();
        self.recount();
        self.rebuild(neighbour, false);
        self.persist(move |store| store.archive(&key, updated_at), cx);
        cx.notify();
    }

    pub fn toggle_read_selected(&mut self, cx: &mut Context<Self>) {
        let Some(&ix) = self.sections.visible.get(self.selected) else {
            return;
        };
        let item = &mut self.items[ix];
        item.unread = !item.unread;
        let (key, updated_at) = (item.pr.key(), item.pr.updated_at);
        if item.unread {
            self.persist(move |store| store.mark_unread(&key), cx);
        } else {
            self.persist(move |store| store.mark_seen(&key, updated_at), cx);
        }
        cx.notify();
    }

    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let mut context = self.load_context();
        let store = self.services.store();
        let epoch = self.local_epoch;
        self.load_task = Some(cx.spawn(async move |this, cx| {
            let loaded = store
                .call(move |store| load_items(store, &mut context))
                .await;
            _ = this.update(cx, |model, cx| {
                if model.local_epoch == epoch {
                    model.apply(flatten(loaded), cx);
                }
            });
        }));
    }

    fn load_context(&self) -> LoadContext {
        let logins = self
            .services
            .accounts()
            .into_iter()
            .map(|account| (account.host.to_lowercase(), account.login))
            .collect();
        LoadContext {
            logins,
            now: Utc::now(),
            closed_window_days: self.services.config().sync.closed_window_days,
        }
    }

    fn apply(&mut self, loaded: Result<Vec<InboxItem>, String>, cx: &mut Context<Self>) {
        match loaded {
            Ok(items) => {
                self.items = items;
                self.reindex();
                self.loaded = true;
                self.load_error = None;
                self.refresh_meta();
                self.refresh_more();
                self.recount();
                self.refilter(false);
            }
            Err(error) => {
                tracing::warn!(%error, "failed to load inbox");
                self.load_error = Some(labels::load_failed(&error).into());
            }
        }
        cx.notify();
    }

    fn persist(
        &mut self,
        job: impl FnOnce(&mut Store) -> argon_store::Result<()> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        self.local_epoch += 1;
        self.load_task = None;
        let store = self.services.store();
        cx.spawn(async move |this, cx| {
            let result = flatten(store.call(job).await);
            _ = this.update(cx, |model, cx| {
                model.action_error = match result {
                    Ok(()) => None,
                    Err(error) => {
                        tracing::warn!(%error, "failed to persist inbox action");
                        Some(labels::action_failed(&error).into())
                    }
                };
                model.reload(cx);
            });
        })
        .detach();
    }

    fn refresh_meta(&mut self) {
        let admin_orgs = self.services.admin_orgs();
        self.orgs = self
            .services
            .config()
            .orgs
            .tracked
            .iter()
            .filter_map(|entry| {
                let (host, owner) = entry.split_once('/')?;
                Some(OrgEntry {
                    key: OrgKey::new(host, owner),
                    name: owner.to_string().into(),
                    count: 0,
                    admin: admin_orgs
                        .iter()
                        .any(|admin| admin.eq_ignore_ascii_case(entry)),
                })
            })
            .collect();
        if let Some(org) = &self.org
            && !self.orgs.iter().any(|entry| &entry.key == org)
        {
            self.org = None;
        }
        self.accounts = self
            .services
            .accounts()
            .into_iter()
            .map(|account| AccountEntry {
                login: format!("@{}", account.login).into(),
                host: account.host.into(),
            })
            .collect();
    }

    fn reindex(&mut self) {
        self.index = self
            .items
            .iter()
            .enumerate()
            .map(|(ix, item)| (item.pr.key(), ix))
            .collect();
    }

    fn recount(&mut self) {
        self.counts = InboxView::ALL.map(|view| {
            let set = view.view_set();
            self.items
                .iter()
                .filter(|item| item.views.contains(set))
                .count()
        });
        for org in &mut self.orgs {
            org.count = self
                .items
                .iter()
                .filter(|item| item.views.contains(ViewSet::INBOX) && item.belongs_to(&org.key))
                .count();
        }
    }

    fn refilter(&mut self, reset_scroll: bool) {
        let kept = self.item_at(self.selected).map(|item| item.pr.key());
        self.rebuild(kept, reset_scroll);
    }

    fn rebuild(&mut self, kept: Option<PrKey>, reset_scroll: bool) {
        let set = self.view.view_set();
        let matches = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.views.contains(set))
            .filter(|(_, item)| self.org.as_ref().is_none_or(|org| item.belongs_to(org)))
            .filter(|(_, item)| item.matches(&self.query))
            .map(|(ix, _)| ix)
            .collect();
        self.sections = Sections::build(&self.items, matches, self.view, Local::now());
        if !self.sections.rows.is_empty() && self.shows_load_more() {
            self.sections.rows.push(InboxRow::LoadMore);
        }
        let last = self.visible_count().saturating_sub(1);
        self.selected = if reset_scroll {
            0
        } else {
            kept.and_then(|key| self.position_of(&key))
                .unwrap_or(self.selected)
                .min(last)
        };
        if reset_scroll {
            self.scroll_generation += 1;
        }
        self.rows_revision += 1;
    }
}

impl EventEmitter<InboxEvent> for InboxModel {}

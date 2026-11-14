use std::collections::{HashMap, HashSet};

use argon_core::{Issue, PrKey, PullRequest, SubjectKind, ViewSet};
use argon_store::Store;
use chrono::{DateTime, Local, Utc};
use gpui_kit::{App, Context, Entity, EventEmitter, Global, SharedString, Subscription, Task};

use super::InboxView;
use super::browse::{
    BrowseKey, BrowseList, BrowsePage, IssueScope, PAGE_SIZE, SeenByHost, browse_host,
    browse_query, last_seen_by_host, load_page,
};
use super::filter::{self, KindFilter};
use super::items::{AccountEntry, InboxItem, LoadContext, OrgEntry, OrgKey, Subject, load_items};
use super::labels;
use super::sections::{InboxRow, Sections};
use crate::services::{Services, SyncEvent, flatten};
use crate::sync_status::SyncStatus;

pub enum InboxEvent {
    OpenPr { key: PrKey, preview: bool },
    OpenIssue { key: PrKey, preview: bool },
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
    kind: KindFilter,
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
    scope: IssueScope,
    target: Option<BrowseKey>,
    browse: HashMap<BrowseKey, BrowseList>,
    browse_inflight: Option<BrowseKey>,
    browse_epoch: u64,
    browse_task: Option<Task<()>>,
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
            kind: KindFilter::All,
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
            scope: IssueScope::Recent,
            target: None,
            browse: HashMap::new(),
            browse_inflight: None,
            browse_epoch: 0,
            browse_task: None,
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
        self.shown_items().get(ix)
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
        match self.index.get(key) {
            Some(&ix) => self.items.get(ix),
            None => self
                .browse
                .values()
                .flat_map(|list| &list.items)
                .find(|item| item.subject.has_key(key)),
        }
    }

    pub(super) fn subject(&self, key: &PrKey) -> Option<&Subject> {
        Some(&self.item(key)?.subject)
    }

    pub fn pr(&self, key: &PrKey) -> Option<&PullRequest> {
        self.subject(key)?.as_pr()
    }

    pub fn issue(&self, key: &PrKey) -> Option<&Issue> {
        self.subject(key)?.as_issue()
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
        if self.view != InboxView::Inbox || self.active_key().is_some() {
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
            self.retarget(true, cx);
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
        self.retarget(true, cx);
        cx.emit(InboxEvent::ShowList);
        cx.notify();
    }

    pub fn kind_filter(&self) -> KindFilter {
        self.kind
    }

    pub fn set_kind_filter(&mut self, filter: KindFilter, cx: &mut Context<Self>) {
        if self.kind != filter {
            self.kind = filter;
            self.retarget(true, cx);
            cx.notify();
        }
    }

    pub fn clear_org(&mut self, cx: &mut Context<Self>) {
        if self.org.take().is_some() {
            self.retarget(true, cx);
            cx.notify();
        }
    }

    pub fn issue_scope(&self) -> IssueScope {
        self.scope
    }

    pub fn set_issue_scope(&mut self, scope: IssueScope, cx: &mut Context<Self>) {
        if self.scope != scope {
            self.scope = scope;
            self.retarget(true, cx);
            cx.notify();
        }
    }

    pub fn closed_window_days(&self) -> i64 {
        self.services.config().sync.closed_window_days
    }

    pub fn can_browse(&self) -> bool {
        self.target.is_some()
    }

    pub(super) fn browse_list(&self) -> Option<&BrowseList> {
        self.browse.get(self.active_key()?)
    }

    pub fn is_browse_loading(&self) -> bool {
        self.browse_inflight.is_some() && self.browse_inflight.as_ref() == self.active_key()
    }

    pub fn browse_more(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.active_key().cloned() else {
            return;
        };
        let after = self.browse.get(&key).and_then(|list| list.next.clone());
        if self.browse_inflight.is_some() || after.is_none() {
            return;
        }
        self.fetch_page(key, after, cx);
        cx.notify();
    }

    pub fn browse_refresh(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.active_key().cloned() else {
            return;
        };
        self.fetch_page(key, None, cx);
        cx.notify();
    }

    pub fn browse_retry(&mut self, cx: &mut Context<Self>) {
        if self.browse_list().is_some_and(|list| list.failed_more) {
            self.browse_more(cx);
        } else {
            self.browse_refresh(cx);
        }
    }

    fn browse_has_more(&self) -> bool {
        self.browse_list().is_some_and(|list| list.next.is_some())
    }

    fn active_key(&self) -> Option<&BrowseKey> {
        self.target
            .as_ref()
            .filter(|_| self.scope == IssueScope::AllOpen)
    }

    fn shown_items(&self) -> &[InboxItem] {
        match self.browse_list() {
            Some(list) => &list.items,
            None => &self.items,
        }
    }

    fn browse_target(&self) -> Option<BrowseKey> {
        let hosts: Vec<String> = self
            .services
            .accounts()
            .into_iter()
            .map(|account| account.host)
            .collect();
        let host = browse_host(self.org.as_ref(), &hosts)?;
        let tracked: Vec<OrgKey> = self
            .orgs
            .iter()
            .map(|org| org.key.clone())
            .filter(|key| key.host() == host)
            .collect();
        let query = browse_query(self.view, self.kind, self.org.as_ref(), &tracked)?;
        Some(BrowseKey {
            view: self.view,
            host,
            query,
        })
    }

    fn retarget(&mut self, reset_scroll: bool, cx: &mut Context<Self>) {
        self.target = self.browse_target();
        self.ensure_browse(cx);
        self.refilter(reset_scroll);
    }

    fn ensure_browse(&mut self, cx: &mut Context<Self>) {
        let active = self.active_key().cloned();
        if self.browse_inflight.is_some() && self.browse_inflight != active {
            self.browse_inflight = None;
            self.browse_task = None;
            self.browse_epoch += 1;
        }
        let Some(key) = active else {
            return;
        };
        let list = self.browse.entry(key.clone()).or_default();
        if !list.loaded && list.error.is_none() && self.browse_inflight.is_none() {
            self.fetch_page(key, None, cx);
        }
    }

    fn fetch_page(&mut self, key: BrowseKey, after: Option<String>, cx: &mut Context<Self>) {
        let first = after.is_none();
        self.browse_epoch += 1;
        let epoch = self.browse_epoch;
        self.browse_inflight = Some(key.clone());
        self.browse.entry(key.clone()).or_default().error = None;
        let query = key.query.clone();
        let page = self
            .services
            .spawn_github(cx, key.host.clone(), move |client| async move {
                client
                    .browse_issues(&query, after.as_deref(), PAGE_SIZE)
                    .await
            });
        let store = self.services.store();
        let mut context = self.load_context();
        self.browse_task = Some(cx.spawn(async move |this, cx| {
            let loaded = match page.await {
                Ok(page) => {
                    let target = key.clone();
                    flatten(
                        store
                            .call(move |store| load_page(store, &target, page, &mut context))
                            .await,
                    )
                }
                Err(error) => Err(error.to_string()),
            };
            _ = this.update(cx, |model, cx| {
                if model.browse_epoch == epoch {
                    model.apply_page(key, first, loaded, cx);
                }
            });
        }));
    }

    fn apply_page(
        &mut self,
        key: BrowseKey,
        first: bool,
        loaded: Result<BrowsePage, String>,
        cx: &mut Context<Self>,
    ) {
        self.browse_inflight = None;
        let list = self.browse.entry(key).or_default();
        match loaded {
            Ok(page) if first => list.replace(page),
            Ok(page) => list.append(page),
            Err(error) => {
                tracing::warn!(%error, "failed to browse open issues");
                list.fail(labels::browse_failed(&error).into(), !first);
            }
        }
        self.refilter(false);
        cx.notify();
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

    pub fn open_subject(&mut self, key: &PrKey, preview: bool, cx: &mut Context<Self>) {
        if let Some(position) = self.position_of(key) {
            self.open(position, preview, cx);
        }
    }

    fn position_of(&self, key: &PrKey) -> Option<usize> {
        let items = self.shown_items();
        self.sections
            .visible
            .iter()
            .position(|&ix| items[ix].subject.has_key(key))
    }

    pub fn open(&mut self, position: usize, preview: bool, cx: &mut Context<Self>) {
        let Some(item) = self.item_at(position) else {
            return;
        };
        let (key, kind) = (item.subject.key(), item.subject.kind());
        let (unread, updated_at) = (item.unread, item.subject.updated_at());
        self.selected = position;
        if unread {
            self.set_unread_in_memory(&key, false, updated_at);
            let seen = key.clone();
            self.persist(move |store| store.mark_seen(&seen, updated_at), cx);
        }
        cx.emit(match kind {
            SubjectKind::Pr => InboxEvent::OpenPr { key, preview },
            SubjectKind::Issue => InboxEvent::OpenIssue { key, preview },
        });
        cx.notify();
    }

    pub fn mark_seen(&mut self, key: &PrKey, updated_at: DateTime<Utc>, cx: &mut Context<Self>) {
        self.set_unread_in_memory(key, false, updated_at);
        let key = key.clone();
        self.persist(move |store| store.mark_seen(&key, updated_at), cx);
        cx.notify();
    }

    fn set_unread_in_memory(&mut self, key: &PrKey, unread: bool, updated_at: DateTime<Utc>) {
        let synced = self.index.get(key).and_then(|&ix| self.items.get_mut(ix));
        let browsed = self
            .browse
            .values_mut()
            .flat_map(|list| list.items.iter_mut())
            .filter(|item| item.subject.has_key(key));
        for item in synced.into_iter().chain(browsed) {
            if unread || item.subject.updated_at() <= updated_at {
                item.unread = unread;
            }
        }
    }

    pub fn archive_selected(&mut self, cx: &mut Context<Self>) {
        if self.active_key().is_some() {
            return;
        }
        let Some(&ix) = self.sections.visible.get(self.selected) else {
            return;
        };
        let neighbour = [self.selected + 1, self.selected.wrapping_sub(1)]
            .into_iter()
            .find_map(|position| self.item_at(position))
            .map(|item| item.subject.key());
        let item = self.items.remove(ix);
        let (key, updated_at) = (item.subject.key(), item.subject.updated_at());
        self.reindex();
        self.recount();
        self.rebuild(neighbour, false);
        self.persist(move |store| store.archive(&key, updated_at), cx);
        cx.notify();
    }

    pub fn toggle_read_selected(&mut self, cx: &mut Context<Self>) {
        let Some(item) = self.item_at(self.selected) else {
            return;
        };
        let (key, updated_at, unread) =
            (item.subject.key(), item.subject.updated_at(), !item.unread);
        self.set_unread_in_memory(&key, unread, updated_at);
        if unread {
            self.persist(move |store| store.mark_unread(&key), cx);
        } else {
            self.persist(move |store| store.mark_seen(&key, updated_at), cx);
        }
        cx.notify();
    }

    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let mut context = self.load_context();
        let hosts: HashSet<String> = self.browse.keys().map(|key| key.host.clone()).collect();
        let store = self.services.store();
        let epoch = self.local_epoch;
        self.load_task = Some(cx.spawn(async move |this, cx| {
            let loaded = store
                .call(move |store| -> argon_store::Result<_> {
                    let items = load_items(store, &mut context)?;
                    Ok((items, last_seen_by_host(store, hosts)?))
                })
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
            closed_window_days: self.closed_window_days(),
        }
    }

    fn apply(
        &mut self,
        loaded: Result<(Vec<InboxItem>, SeenByHost), String>,
        cx: &mut Context<Self>,
    ) {
        match loaded {
            Ok((items, seen)) => {
                self.items = items;
                self.reindex();
                self.refresh_browse_unread(&seen);
                self.loaded = true;
                self.load_error = None;
                self.refresh_meta();
                self.refresh_more();
                self.recount();
                self.retarget(false, cx);
            }
            Err(error) => {
                tracing::warn!(%error, "failed to load inbox");
                self.load_error = Some(labels::load_failed(&error).into());
            }
        }
        cx.notify();
    }

    fn refresh_browse_unread(&mut self, seen: &SeenByHost) {
        for (key, list) in &mut self.browse {
            if let Some(last_seen) = seen.get(&key.host) {
                list.refresh_unread(last_seen);
            }
        }
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
            .map(|(ix, item)| (item.subject.key(), ix))
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
        let kept = self.item_at(self.selected).map(|item| item.subject.key());
        self.rebuild(kept, reset_scroll);
    }

    fn rebuild(&mut self, kept: Option<PrKey>, reset_scroll: bool) {
        let matches = match self.browse_list() {
            Some(list) => list
                .items
                .iter()
                .enumerate()
                .filter(|(_, item)| item.matches(&self.query))
                .map(|(ix, _)| ix)
                .collect(),
            None => self.synced_matches(),
        };
        self.sections = Sections::build(self.shown_items(), matches, self.view, Local::now());
        if !self.sections.rows.is_empty() && self.shows_load_more() {
            self.sections.rows.push(InboxRow::LoadMore);
        }
        if !self.sections.rows.is_empty() && self.browse_has_more() {
            self.sections.rows.push(InboxRow::BrowseMore);
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

    fn synced_matches(&self) -> Vec<usize> {
        let set = self.view.view_set();
        self.items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.views.contains(set))
            .filter(|(_, item)| filter::passes(item.subject.kind(), self.kind, self.view))
            .filter(|(_, item)| self.org.as_ref().is_none_or(|org| item.belongs_to(org)))
            .filter(|(_, item)| item.matches(&self.query))
            .map(|(ix, _)| ix)
            .collect()
    }
}

impl EventEmitter<InboxEvent> for InboxModel {}

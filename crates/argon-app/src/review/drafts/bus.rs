use argon_core::PrKey;
use gpui_kit::{App, AppContext as _, Entity, EventEmitter, Global};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftsChanged {
    pub pr: PrKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectsChanged;

pub struct DraftsBus;

struct DraftsBusHandle(Entity<DraftsBus>);

impl Global for DraftsBusHandle {}

impl EventEmitter<DraftsChanged> for DraftsBus {}

impl EventEmitter<ProjectsChanged> for DraftsBus {}

impl DraftsBus {
    pub fn global(cx: &mut App) -> Entity<Self> {
        if let Some(handle) = cx.try_global::<DraftsBusHandle>() {
            return handle.0.clone();
        }
        let bus = cx.new(|_| DraftsBus);
        cx.set_global(DraftsBusHandle(bus.clone()));
        bus
    }

    pub fn notify(pr: PrKey, cx: &mut App) {
        Self::global(cx).update(cx, |_, cx| cx.emit(DraftsChanged { pr }));
    }

    pub fn notify_projects(cx: &mut App) {
        Self::global(cx).update(cx, |_, cx| cx.emit(ProjectsChanged));
    }
}

use crate::inbox::InboxView;

gpui_kit::actions!(
    argon,
    [
        ToggleLeftDock,
        ToggleRightDock,
        ToggleBottomDock,
        CloseActiveItem,
        SelectNext,
        SelectPrevious,
        OpenSelected,
        ArchiveSelected,
        ToggleRead,
        FocusFilter,
        ClearFilter,
        OpenPalette,
        SyncNow,
        OpenSettings,
        GoToAgents,
        CheckForUpdates,
        ToggleDiffMode,
        CancelComment,
        StartReview,
        ReviewWithAi,
        ToggleTheme
    ]
);

#[derive(Clone, Debug, PartialEq, gpui_kit::Action)]
#[action(namespace = argon, no_json)]
pub struct GoToView(pub InboxView);

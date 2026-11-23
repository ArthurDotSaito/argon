use argon_core::{PermissionMode, SessionKind};
use gpui_kit::SharedString;
use gpui_kit::component::select::SelectItem;

use super::labels;

const PERMISSION_MODES: [PermissionMode; 3] = [
    PermissionMode::Supervised,
    PermissionMode::AcceptEdits,
    PermissionMode::FullAccess,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub id: String,
    pub label: SharedString,
}

impl Choice {
    pub fn new(id: impl Into<String>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
        }
    }
}

const SUMMARY_PERMISSION_MODES: [PermissionMode; 2] =
    [PermissionMode::Supervised, PermissionMode::FullAccess];

fn choices_of(modes: &[PermissionMode]) -> Vec<Choice> {
    modes
        .iter()
        .map(|&mode| Choice::new(mode.as_str(), labels::permission_mode(mode)))
        .collect()
}

pub fn permission_choices() -> Vec<Choice> {
    choices_of(&PERMISSION_MODES)
}

pub fn permission_choices_for(kind: SessionKind) -> Vec<Choice> {
    match kind {
        SessionKind::Summary => choices_of(&SUMMARY_PERMISSION_MODES),
        SessionKind::Review | SessionKind::Plan => permission_choices(),
    }
}

impl SelectItem for Choice {
    type Value = String;

    fn title(&self) -> SharedString {
        self.label.clone()
    }

    fn value(&self) -> &String {
        &self.id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(choices: Vec<Choice>) -> Vec<String> {
        choices.into_iter().map(|choice| choice.id).collect()
    }

    #[test]
    fn summary_offers_supervised_and_full_access_only() {
        assert_eq!(
            ids(permission_choices_for(SessionKind::Summary)),
            ["supervised", "full-access"]
        );
    }

    #[test]
    fn review_and_plan_keep_every_mode() {
        for kind in [SessionKind::Review, SessionKind::Plan] {
            assert_eq!(
                ids(permission_choices_for(kind)),
                ["supervised", "accept-edits", "full-access"]
            );
        }
    }
}

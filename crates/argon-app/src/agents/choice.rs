use argon_core::PermissionMode;
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

pub fn permission_choices() -> Vec<Choice> {
    PERMISSION_MODES
        .into_iter()
        .map(|mode| Choice::new(mode.as_str(), labels::permission_mode(mode)))
        .collect()
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

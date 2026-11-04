use argon_core::PermissionMode;

use crate::registry::ModelInfo;

fn candidates(mode: PermissionMode) -> &'static [&'static str] {
    match mode {
        PermissionMode::Supervised => &["default"],
        PermissionMode::AcceptEdits => &["acceptEdits", "auto-accept"],
        PermissionMode::FullAccess => &["bypassPermissions", "bypass"],
    }
}

pub(super) fn acp_mode_value(mode: PermissionMode, offered: &[ModelInfo]) -> Option<String> {
    candidates(mode)
        .iter()
        .find(|candidate| offered.iter().any(|value| value.id == **candidate))
        .map(|candidate| (*candidate).to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offered(ids: &[&str]) -> Vec<ModelInfo> {
        ids.iter()
            .map(|id| ModelInfo {
                id: (*id).to_owned(),
                label: (*id).to_owned(),
            })
            .collect()
    }

    #[test]
    fn claude_style_modes_map_to_their_values() {
        let modes = offered(&["default", "acceptEdits", "plan", "bypassPermissions"]);
        assert_eq!(
            acp_mode_value(PermissionMode::Supervised, &modes).as_deref(),
            Some("default")
        );
        assert_eq!(
            acp_mode_value(PermissionMode::AcceptEdits, &modes).as_deref(),
            Some("acceptEdits")
        );
        assert_eq!(
            acp_mode_value(PermissionMode::FullAccess, &modes).as_deref(),
            Some("bypassPermissions")
        );
    }

    #[test]
    fn alternative_names_are_used_when_offered() {
        let modes = offered(&["auto-accept", "bypass"]);
        assert_eq!(
            acp_mode_value(PermissionMode::AcceptEdits, &modes).as_deref(),
            Some("auto-accept")
        );
        assert_eq!(
            acp_mode_value(PermissionMode::FullAccess, &modes).as_deref(),
            Some("bypass")
        );
    }

    #[test]
    fn first_candidate_wins_when_several_are_offered() {
        let modes = offered(&["bypass", "auto-accept", "acceptEdits", "bypassPermissions"]);
        assert_eq!(
            acp_mode_value(PermissionMode::AcceptEdits, &modes).as_deref(),
            Some("acceptEdits")
        );
        assert_eq!(
            acp_mode_value(PermissionMode::FullAccess, &modes).as_deref(),
            Some("bypassPermissions")
        );
    }

    #[test]
    fn no_matching_value_yields_none() {
        let modes = offered(&["read-only", "auto", "full-access"]);
        for mode in [
            PermissionMode::Supervised,
            PermissionMode::AcceptEdits,
            PermissionMode::FullAccess,
        ] {
            assert_eq!(acp_mode_value(mode, &modes), None, "{mode:?}");
        }
        assert_eq!(acp_mode_value(PermissionMode::Supervised, &[]), None);
    }
}

use argon_core::NotificationsCfg;

use super::toast::Channel;

pub fn allowed(config: NotificationsCfg, channel: Channel, window_focused: bool) -> bool {
    match channel {
        Channel::NeedsYou => config.needs_you && !window_focused,
        Channel::AgentPermission => config.agent_events,
        Channel::AgentOutcome => config.agent_events && !window_focused,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_ON: NotificationsCfg = NotificationsCfg {
        needs_you: true,
        agent_events: true,
    };

    #[test]
    fn focused_window_silences_everything_but_permission_requests() {
        assert!(!allowed(ALL_ON, Channel::NeedsYou, true));
        assert!(!allowed(ALL_ON, Channel::AgentOutcome, true));
        assert!(allowed(ALL_ON, Channel::AgentPermission, true));
    }

    #[test]
    fn unfocused_window_notifies_enabled_channels() {
        assert!(allowed(ALL_ON, Channel::NeedsYou, false));
        assert!(allowed(ALL_ON, Channel::AgentOutcome, false));
        assert!(allowed(ALL_ON, Channel::AgentPermission, false));
    }

    #[test]
    fn disabled_flags_silence_their_channels() {
        let needs_you_off = NotificationsCfg {
            needs_you: false,
            agent_events: true,
        };
        let agents_off = NotificationsCfg {
            needs_you: true,
            agent_events: false,
        };
        assert!(!allowed(needs_you_off, Channel::NeedsYou, false));
        assert!(allowed(needs_you_off, Channel::AgentOutcome, false));
        assert!(!allowed(agents_off, Channel::AgentPermission, false));
        assert!(!allowed(agents_off, Channel::AgentOutcome, false));
        assert!(allowed(agents_off, Channel::NeedsYou, false));
    }
}

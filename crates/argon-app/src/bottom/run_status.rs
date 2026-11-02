use argon_github::RunStatus;
use chrono::TimeDelta;
use gpui_kit::assets::IconName;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _};
use gpui_kit::{AnyElement, App, Hsla, IntoElement, Styled as _};

pub fn is_running(status: RunStatus) -> bool {
    matches!(status, RunStatus::Queued | RunStatus::InProgress)
}

pub fn run_status_label(status: RunStatus) -> &'static str {
    match status {
        RunStatus::Queued => "Queued",
        RunStatus::InProgress => "In progress",
        RunStatus::Success => "Success",
        RunStatus::Failure => "Failure",
        RunStatus::Cancelled => "Cancelled",
        RunStatus::Skipped => "Skipped",
    }
}

pub fn run_status_color(status: RunStatus, cx: &App) -> Hsla {
    let theme = cx.theme();
    match status {
        RunStatus::Success => theme.success,
        RunStatus::Failure => theme.danger,
        RunStatus::Queued | RunStatus::InProgress => theme.warning,
        RunStatus::Cancelled | RunStatus::Skipped => theme.muted_foreground,
    }
}

pub fn run_status_icon(status: RunStatus, cx: &App) -> AnyElement {
    let color = run_status_color(status, cx);
    let icon = match status {
        RunStatus::Queued | RunStatus::InProgress => {
            return Spinner::new()
                .icon(Icon::new(IconName::LoaderCircle))
                .color(color)
                .small()
                .into_any_element();
        }
        RunStatus::Success => IconName::CircleCheck,
        RunStatus::Failure => IconName::CircleX,
        RunStatus::Cancelled | RunStatus::Skipped => IconName::Ban,
    };
    Icon::new(icon).small().text_color(color).into_any_element()
}

pub fn duration_label(duration: TimeDelta) -> String {
    let seconds = duration.num_seconds().max(0);
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    match (hours, minutes) {
        (0, 0) => format!("{seconds}s"),
        (0, _) => format!("{minutes}m {seconds}s"),
        _ => format!("{hours}h {minutes}m"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_pick_the_two_largest_units() {
        assert_eq!(duration_label(TimeDelta::seconds(0)), "0s");
        assert_eq!(duration_label(TimeDelta::seconds(45)), "45s");
        assert_eq!(duration_label(TimeDelta::seconds(83)), "1m 23s");
        assert_eq!(duration_label(TimeDelta::seconds(3720)), "1h 2m");
        assert_eq!(duration_label(TimeDelta::seconds(-5)), "0s");
    }
}

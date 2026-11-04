use std::time::{Duration, SystemTime};

use argon_agents::{AccountUsage, UsageSeverity, UsageWindow};

use super::labels;
use super::usage_cache::UsageState;

const UNAVAILABLE_MAX_CHARS: usize = 44;
const SECS_PER_MINUTE: u64 = 60;
const MINUTES_PER_HOUR: u64 = 60;
const HOURS_PER_DAY: u64 = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Muted,
    Warning,
    Danger,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageLine {
    pub text: String,
    pub tone: Tone,
    pub tooltip: Option<String>,
    pub link: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WindowRow {
    pub label: String,
    pub percent: u32,
    pub fraction: f32,
    pub tone: Tone,
    pub renews: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UsageDetail {
    Line(UsageLine),
    Windows { plan: String, rows: Vec<WindowRow> },
}

fn line(text: impl Into<String>, tone: Tone) -> UsageLine {
    UsageLine {
        text: text.into(),
        tone,
        tooltip: None,
        link: None,
    }
}

pub fn tone_for(severity: UsageSeverity) -> Tone {
    match severity {
        UsageSeverity::Normal => Tone::Muted,
        UsageSeverity::Warning => Tone::Warning,
        UsageSeverity::Blocked => Tone::Danger,
    }
}

fn severity_rank(severity: UsageSeverity) -> u8 {
    match severity {
        UsageSeverity::Normal => 0,
        UsageSeverity::Warning => 1,
        UsageSeverity::Blocked => 2,
    }
}

pub fn tightest_window(windows: &[UsageWindow]) -> Option<&UsageWindow> {
    windows.iter().reduce(|best, window| {
        let rank = severity_rank(window.severity);
        let best_rank = severity_rank(best.severity);
        if rank > best_rank || (rank == best_rank && window.percent > best.percent) {
            window
        } else {
            best
        }
    })
}

pub fn whole_percent(percent: f64) -> u32 {
    if percent >= 100. {
        100
    } else {
        percent.floor().max(0.) as u32
    }
}

pub fn relative_time(resets_at: SystemTime, now: SystemTime) -> String {
    let remaining = resets_at.duration_since(now).unwrap_or(Duration::ZERO);
    let seconds = remaining.as_secs() + u64::from(remaining.subsec_nanos() > 0);
    let minutes = seconds.div_ceil(SECS_PER_MINUTE);
    let hours = minutes.div_ceil(MINUTES_PER_HOUR);
    if seconds == 0 {
        labels::RESETS_SOON.to_owned()
    } else if minutes < MINUTES_PER_HOUR {
        labels::in_minutes(minutes)
    } else if hours < HOURS_PER_DAY {
        labels::in_hours(hours)
    } else {
        labels::in_days(hours.div_ceil(HOURS_PER_DAY))
    }
}

fn renews_text(window: &UsageWindow, now: SystemTime) -> Option<String> {
    window
        .resets_at
        .map(|at| labels::renews(&relative_time(at, now)))
}

fn truncated(text: &str) -> String {
    if text.chars().count() <= UNAVAILABLE_MAX_CHARS {
        return text.to_owned();
    }
    let kept: String = text.chars().take(UNAVAILABLE_MAX_CHARS - 1).collect();
    format!("{}…", kept.trim_end())
}

fn unavailable_line(message: &str) -> UsageLine {
    UsageLine {
        tooltip: Some(message.to_owned()),
        ..line(truncated(message), Tone::Muted)
    }
}

fn link_line(url: &str) -> UsageLine {
    UsageLine {
        link: Some(url.to_owned()),
        ..line(labels::USAGE_LINK, Tone::Muted)
    }
}

fn summary_line(plan: &str, windows: &[UsageWindow], now: SystemTime) -> UsageLine {
    let Some(window) = tightest_window(windows) else {
        let text = if plan.is_empty() {
            labels::NO_USAGE_WINDOWS
        } else {
            plan
        };
        return line(text, Tone::Muted);
    };
    let text = labels::usage_summary(plan, whole_percent(window.percent), &window.label);
    let tooltip = (window.severity == UsageSeverity::Blocked).then(|| {
        window.resets_at.map_or_else(
            || labels::LIMIT_REACHED.to_owned(),
            |at| labels::limit_reached_renews(&relative_time(at, now)),
        )
    });
    UsageLine {
        tooltip,
        ..line(text, tone_for(window.severity))
    }
}

pub fn card_line(state: UsageState<'_>, now: SystemTime) -> Option<UsageLine> {
    status_line(state, now).map(|line| {
        let tooltip = line
            .tooltip
            .clone()
            .or_else(|| line.link.is_none().then(|| line.text.clone()));
        UsageLine { tooltip, ..line }
    })
}

fn status_line(state: UsageState<'_>, now: SystemTime) -> Option<UsageLine> {
    match state {
        UsageState::Hidden => None,
        UsageState::Loading => Some(line(labels::LOADING_USAGE, Tone::Muted)),
        UsageState::NeedsConsent => Some(line(labels::USAGE_CONSENT_HINT, Tone::Muted)),
        UsageState::Shown(AccountUsage::Ok { plan, windows }) => {
            Some(summary_line(plan, windows, now))
        }
        UsageState::Shown(AccountUsage::Unavailable(message)) => Some(unavailable_line(message)),
        UsageState::Shown(AccountUsage::Link(url)) => Some(link_line(url)),
    }
}

fn window_row(window: &UsageWindow, now: SystemTime) -> WindowRow {
    WindowRow {
        label: window.label.clone(),
        percent: whole_percent(window.percent),
        fraction: (window.percent / 100.).clamp(0., 1.) as f32,
        tone: tone_for(window.severity),
        renews: renews_text(window, now),
    }
}

pub fn detail(state: UsageState<'_>, now: SystemTime) -> Option<UsageDetail> {
    match state {
        UsageState::Shown(AccountUsage::Ok { plan, windows }) if !windows.is_empty() => {
            Some(UsageDetail::Windows {
                plan: plan.clone(),
                rows: windows
                    .iter()
                    .map(|window| window_row(window, now))
                    .collect(),
            })
        }
        other => card_line(other, now).map(UsageDetail::Line),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(label: &str, percent: f64, severity: UsageSeverity) -> UsageWindow {
        UsageWindow {
            label: label.into(),
            percent,
            resets_at: None,
            severity,
        }
    }

    fn at(base: SystemTime, secs: u64) -> SystemTime {
        base + Duration::from_secs(secs)
    }

    #[test]
    fn the_tightest_window_is_the_highest_percent_and_the_first_on_ties() {
        assert!(tightest_window(&[]).is_none());
        let windows = [
            window("5 horas", 12., UsageSeverity::Normal),
            window("Semanal", 49., UsageSeverity::Normal),
            window("Mensal", 49., UsageSeverity::Normal),
        ];
        assert_eq!(tightest_window(&windows).unwrap().label, "Semanal");
    }

    #[test]
    fn a_more_severe_window_wins_over_a_higher_percent_one() {
        let windows = [
            window("Semanal", 95., UsageSeverity::Warning),
            window("5 horas", 70., UsageSeverity::Blocked),
            window("Mensal", 99., UsageSeverity::Warning),
        ];
        assert_eq!(tightest_window(&windows).unwrap().label, "5 horas");
        let warned = [
            window("Semanal", 30., UsageSeverity::Normal),
            window("Mensal", 81., UsageSeverity::Warning),
        ];
        assert_eq!(tightest_window(&warned).unwrap().label, "Mensal");
    }

    #[test]
    fn whole_percent_floors_below_one_hundred() {
        assert_eq!(whole_percent(0.), 0);
        assert_eq!(whole_percent(48.6), 48);
        assert_eq!(whole_percent(99.6), 99);
        assert_eq!(whole_percent(99.999), 99);
        assert_eq!(whole_percent(100.), 100);
        assert_eq!(whole_percent(130.), 100);
        assert_eq!(whole_percent(-5.), 0);
    }

    #[test]
    fn relative_time_picks_the_coarsest_unit_and_rounds_up() {
        let now = SystemTime::now();
        assert_eq!(relative_time(at(now, 15 * 60), now), "em 15 min");
        assert_eq!(relative_time(at(now, 15 * 60 + 30), now), "em 16 min");
        assert_eq!(relative_time(at(now, 2 * 3600), now), "em 2 h");
        assert_eq!(relative_time(at(now, 2 * 3600 + 1), now), "em 3 h");
        assert_eq!(relative_time(at(now, 2 * 3600 + 59 * 60), now), "em 3 h");
        assert_eq!(relative_time(at(now, 3 * 86_400), now), "em 3 dias");
        assert_eq!(relative_time(at(now, 3 * 86_400 + 5), now), "em 4 dias");
        assert_eq!(relative_time(at(now, 86_400), now), "em 1 dia");
        assert_eq!(relative_time(at(now, 3599), now), "em 1 h");
        assert_eq!(relative_time(at(now, 3600), now), "em 1 h");
    }

    #[test]
    fn relative_time_is_instant_when_due_or_already_past() {
        let now = SystemTime::now();
        assert_eq!(relative_time(at(now, 59), now), "em 1 min");
        assert_eq!(relative_time(now, now), labels::RESETS_SOON);
        assert_eq!(
            relative_time(now - Duration::from_secs(600), now),
            labels::RESETS_SOON
        );
    }

    #[test]
    fn the_card_shows_plan_and_the_tightest_window() {
        let now = SystemTime::now();
        let usage = AccountUsage::Ok {
            plan: "Team".into(),
            windows: vec![
                window("5 horas", 12., UsageSeverity::Normal),
                window("Semanal", 49.6, UsageSeverity::Normal),
            ],
        };
        let line = card_line(UsageState::Shown(&usage), now).unwrap();
        assert_eq!(line.text, "Team · 49% semanal");
        assert_eq!(line.tone, Tone::Muted);
        assert_eq!(line.tooltip.as_deref(), Some("Team · 49% semanal"));
    }

    #[test]
    fn every_non_link_line_carries_its_full_text_as_tooltip() {
        let now = SystemTime::now();
        for state in [UsageState::Loading, UsageState::NeedsConsent] {
            let line = card_line(state, now).unwrap();
            assert_eq!(line.tooltip.as_deref(), Some(line.text.as_str()));
        }
        let link = AccountUsage::Link("https://example.com/usage".into());
        assert_eq!(
            card_line(UsageState::Shown(&link), now).unwrap().tooltip,
            None
        );
    }

    #[test]
    fn numeric_window_labels_read_as_a_duration() {
        let now = SystemTime::now();
        let usage = AccountUsage::Ok {
            plan: "Plus".into(),
            windows: vec![window("5 horas", 30., UsageSeverity::Normal)],
        };
        let line = card_line(UsageState::Shown(&usage), now).unwrap();
        assert_eq!(line.text, "Plus · 30% em 5 horas");
    }

    #[test]
    fn a_warning_window_is_amber_and_a_blocked_one_is_red_with_the_reset_tooltip() {
        let now = SystemTime::now();
        let warning = AccountUsage::Ok {
            plan: "Pro".into(),
            windows: vec![window("Semanal", 85., UsageSeverity::Warning)],
        };
        assert_eq!(
            card_line(UsageState::Shown(&warning), now).unwrap().tone,
            Tone::Warning
        );
        let mut blocked_window = window("Semanal", 100., UsageSeverity::Blocked);
        blocked_window.resets_at = Some(at(now, 2 * 3600 + 120));
        let blocked = AccountUsage::Ok {
            plan: "Pro".into(),
            windows: vec![blocked_window],
        };
        let line = card_line(UsageState::Shown(&blocked), now).unwrap();
        assert_eq!(line.tone, Tone::Danger);
        assert_eq!(
            line.tooltip.as_deref(),
            Some("Limite atingido — renova em 3 h")
        );
    }

    #[test]
    fn a_blocked_window_without_a_reset_only_says_the_limit_was_reached() {
        let usage = AccountUsage::Ok {
            plan: String::new(),
            windows: vec![window("Mensal", 100., UsageSeverity::Blocked)],
        };
        let line = card_line(UsageState::Shown(&usage), SystemTime::now()).unwrap();
        assert_eq!(line.text, "100% mensal");
        assert_eq!(line.tooltip.as_deref(), Some("Limite atingido"));
    }

    #[test]
    fn plans_without_windows_show_the_plan_alone() {
        let usage = AccountUsage::Ok {
            plan: "Team".into(),
            windows: Vec::new(),
        };
        let line = card_line(UsageState::Shown(&usage), SystemTime::now()).unwrap();
        assert_eq!(line.text, "Team");
    }

    #[test]
    fn unavailable_messages_are_truncated_with_the_full_text_in_the_tooltip() {
        let message = "Não consegui falar com o serviço de uso do agente agora";
        let usage = AccountUsage::Unavailable(message.into());
        let line = card_line(UsageState::Shown(&usage), SystemTime::now()).unwrap();
        assert!(line.text.ends_with('…'));
        assert!(line.text.chars().count() <= UNAVAILABLE_MAX_CHARS);
        assert_eq!(line.tooltip.as_deref(), Some(message));
        let short = AccountUsage::Unavailable("Sem suporte".into());
        let line = card_line(UsageState::Shown(&short), SystemTime::now()).unwrap();
        assert_eq!(line.text, "Sem suporte");
    }

    #[test]
    fn links_loading_consent_and_hidden_states_have_their_own_lines() {
        let now = SystemTime::now();
        let link = AccountUsage::Link("https://example.com/usage".into());
        let line = card_line(UsageState::Shown(&link), now).unwrap();
        assert_eq!(line.text, labels::USAGE_LINK);
        assert_eq!(line.link.as_deref(), Some("https://example.com/usage"));
        assert_eq!(
            card_line(UsageState::Loading, now).unwrap().text,
            labels::LOADING_USAGE
        );
        assert_eq!(
            card_line(UsageState::NeedsConsent, now).unwrap().text,
            labels::USAGE_CONSENT_HINT
        );
        assert!(card_line(UsageState::Hidden, now).is_none());
    }

    #[test]
    fn the_detail_lists_every_window_with_integer_percent_and_reset() {
        let now = SystemTime::now();
        let mut weekly = window("Semanal", 49.4, UsageSeverity::Normal);
        weekly.resets_at = Some(at(now, 3 * 86_400));
        let usage = AccountUsage::Ok {
            plan: "Team".into(),
            windows: vec![window("5 horas", 100., UsageSeverity::Blocked), weekly],
        };
        let Some(UsageDetail::Windows { plan, rows }) = detail(UsageState::Shown(&usage), now)
        else {
            panic!("expected the windows detail");
        };
        assert_eq!(plan, "Team");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].percent, 100);
        assert_eq!(rows[0].tone, Tone::Danger);
        assert_eq!(rows[0].renews, None);
        assert_eq!(rows[1].percent, 49);
        assert_eq!(rows[1].renews.as_deref(), Some("renova em 3 dias"));
        assert!((rows[1].fraction - 0.494).abs() < 1e-6);
    }

    #[test]
    fn the_detail_falls_back_to_the_status_line_without_windows() {
        let now = SystemTime::now();
        let usage = AccountUsage::Ok {
            plan: "Team".into(),
            windows: Vec::new(),
        };
        assert_eq!(
            detail(UsageState::Shown(&usage), now),
            Some(UsageDetail::Line(UsageLine {
                tooltip: Some("Team".into()),
                ..line("Team", Tone::Muted)
            }))
        );
        assert_eq!(detail(UsageState::Hidden, now), None);
    }
}

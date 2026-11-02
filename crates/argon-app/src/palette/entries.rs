use argon_core::{PrKey, PrState, PullRequest};
use gpui_kit::assets::IconName;
use gpui_kit::{Action, SharedString};

use super::ranking::Candidate;
use crate::actions::{
    CloseActiveItem, GoToView, OpenSettings, SyncNow, ToggleBottomDock, ToggleLeftDock,
    ToggleRightDock,
};
use crate::inbox::InboxView;

const REVIEW_WITH_AI_TITLE: &str = "Revisar com IA no PR atual";
pub const COMING_SOON: &str = "em breve";

pub struct PrEntry {
    pub key: PrKey,
    pub title: SharedString,
    pub reference: SharedString,
    pub state: PrState,
    pub is_draft: bool,
    number: u64,
    haystack: String,
}

impl PrEntry {
    pub fn new(pr: &PullRequest) -> Self {
        let reference = format!("{} #{}", pr.repo, pr.number);
        let haystack = [
            pr.title.as_str(),
            reference.as_str(),
            pr.author.as_str(),
            pr.head.as_str(),
        ]
        .join("\n");
        Self {
            key: pr.key(),
            title: pr.title.clone().into(),
            reference: reference.into(),
            state: pr.state,
            is_draft: pr.is_draft,
            number: pr.number,
            haystack,
        }
    }

    pub fn candidate(&self) -> Candidate<'_> {
        Candidate {
            text: &self.haystack,
            number: Some(self.number),
        }
    }
}

pub struct CommandSpec {
    pub title: &'static str,
    pub icon: IconName,
    pub kind: CommandKind,
}

pub enum CommandKind {
    Run(Box<dyn Action>),
    Soon,
}

impl CommandSpec {
    fn new(title: &'static str, icon: IconName, action: impl Action) -> Self {
        Self {
            title,
            icon,
            kind: CommandKind::Run(Box::new(action)),
        }
    }

    pub fn candidate(&self) -> Candidate<'_> {
        Candidate {
            text: self.title,
            number: None,
        }
    }
}

fn go_to(view: InboxView) -> CommandSpec {
    let title = match view {
        InboxView::Inbox => "Ir para Inbox",
        InboxView::NeedsYou => "Ir para Needs you",
        InboxView::YourPrs => "Ir para Your PRs",
        InboxView::WaitingOnOthers => "Ir para Waiting on others",
        InboxView::ChangesRequested => "Ir para Changes requested",
        InboxView::ReadyToMerge => "Ir para Ready to merge",
        InboxView::AlreadyDone => "Ir para Already done",
    };
    CommandSpec::new(title, IconName::ArrowRight, GoToView(view))
}

pub fn command_specs() -> Vec<CommandSpec> {
    InboxView::ALL
        .into_iter()
        .map(go_to)
        .chain([
            CommandSpec::new("Sincronizar agora", IconName::RefreshCw, SyncNow),
            CommandSpec::new(
                "Alternar painel esquerdo",
                IconName::PanelLeft,
                ToggleLeftDock,
            ),
            CommandSpec::new(
                "Alternar painel inferior",
                IconName::PanelBottom,
                ToggleBottomDock,
            ),
            CommandSpec::new(
                "Alternar painel direito",
                IconName::PanelRight,
                ToggleRightDock,
            ),
            CommandSpec::new("Fechar aba", IconName::X, CloseActiveItem),
            CommandSpec {
                title: REVIEW_WITH_AI_TITLE,
                icon: IconName::Sparkles,
                kind: CommandKind::Soon,
            },
            CommandSpec::new("Abrir configurações", IconName::Settings, OpenSettings),
        ])
        .collect()
}

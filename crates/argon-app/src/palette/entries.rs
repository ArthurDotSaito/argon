use argon_core::{PrKey, PrState, PullRequest};
use gpui_kit::assets::IconName;
use gpui_kit::{Action, SharedString};

use super::ranking::Candidate;
use crate::actions::{
    CheckForUpdates, CloseActiveItem, GoToAgents, GoToView, OpenSettings, ReviewWithAi,
    StartReview, SyncNow, ToggleBottomDock, ToggleLeftDock, ToggleRightDock, ToggleTheme,
};
use crate::inbox::InboxView;

const REVIEW_WITH_AI_TITLE: &str = "Agent Review no PR atual";

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
    pub action: Box<dyn Action>,
}

impl CommandSpec {
    fn new(title: &'static str, icon: IconName, action: impl Action) -> Self {
        Self {
            title,
            icon,
            action: Box::new(action),
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
            CommandSpec::new("Ir para Agentes", IconName::Bot, GoToAgents),
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
            CommandSpec::new("Enviar review", IconName::Send, StartReview),
            CommandSpec::new(REVIEW_WITH_AI_TITLE, IconName::Sparkles, ReviewWithAi),
            CommandSpec::new("Abrir configurações", IconName::Settings, OpenSettings),
            CommandSpec::new(
                "Verificar atualizações",
                IconName::RefreshCw,
                CheckForUpdates,
            ),
            CommandSpec::new("Alternar tema", IconName::Palette, ToggleTheme),
        ])
        .collect()
}

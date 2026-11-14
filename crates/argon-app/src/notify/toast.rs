use argon_core::{PrKey, SessionKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    NeedsYou,
    AgentPermission,
    AgentOutcome,
    AppUpdate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToastTarget {
    NeedsYou,
    Pr(PrKey),
    Issue(PrKey),
    AgentSession(PrKey, SessionKind),
    AppUpdate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToastSpec {
    pub channel: Channel,
    pub body: String,
    pub target: ToastTarget,
}

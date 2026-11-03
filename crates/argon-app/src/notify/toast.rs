use argon_core::PrKey;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    NeedsYou,
    AgentPermission,
    AgentOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToastTarget {
    NeedsYou,
    Pr(PrKey),
    AgentReview(PrKey),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToastSpec {
    pub channel: Channel,
    pub body: String,
    pub target: ToastTarget,
}

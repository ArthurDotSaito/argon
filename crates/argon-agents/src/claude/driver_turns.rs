const END_TURN: &str = "end_turn";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Close {
    Completed,
    Cancelled,
    Failed(String),
}

#[derive(Debug, Default)]
struct Turn {
    cancelled: bool,
    api_error: Option<String>,
}

#[derive(Debug, Default)]
pub(super) struct Turns {
    current: Option<Turn>,
}

impl Turns {
    pub fn in_turn(&self) -> bool {
        self.current.is_some()
    }

    pub fn is_cancelled(&self) -> bool {
        self.current.as_ref().is_some_and(|turn| turn.cancelled)
    }

    pub fn start(&mut self) {
        self.current = Some(Turn::default());
    }

    pub fn cancel(&mut self) -> bool {
        match &mut self.current {
            Some(turn) if !turn.cancelled => {
                turn.cancelled = true;
                true
            }
            _ => false,
        }
    }

    pub fn api_error(&mut self, text: String) {
        if let Some(turn) = &mut self.current {
            turn.api_error = Some(text);
        }
    }

    pub fn finish(&mut self, stop_reason: &str, error: Option<String>) -> Option<Close> {
        let turn = self.current.take()?;
        Some(if turn.cancelled {
            Close::Cancelled
        } else if stop_reason == END_TURN && turn.api_error.is_none() {
            Close::Completed
        } else {
            Close::Failed(
                error
                    .or(turn.api_error)
                    .unwrap_or_else(|| stop_reason.to_owned()),
            )
        })
    }

    pub fn abort(&mut self, error: String) -> Option<Close> {
        let turn = self.current.take()?;
        Some(if turn.cancelled {
            Close::Cancelled
        } else {
            Close::Failed(error)
        })
    }

    pub fn expire(&mut self) -> Option<Close> {
        if !self.is_cancelled() {
            return None;
        }
        self.current = None;
        Some(Close::Cancelled)
    }
}

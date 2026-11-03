use super::events::AgentEvent;

const CHUNK_RECORD_LIMIT: usize = 4 * 1024;
const STORED_OUTPUT_LIMIT: usize = 16 * 1024;
const TRUNCATED_NOTICE: &str = "\n[saída truncada no histórico]";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChunkKind {
    Message,
    Thought,
}

#[derive(Debug, Clone)]
struct OpenChunk {
    seq: u64,
    kind: ChunkKind,
    text: String,
}

impl OpenChunk {
    fn event(&self) -> AgentEvent {
        match self.kind {
            ChunkKind::Message => AgentEvent::MessageChunk(self.text.clone()),
            ChunkKind::Thought => AgentEvent::ThoughtChunk(self.text.clone()),
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct Recorded {
    pub seq: u64,
    pub closed: Vec<(u64, AgentEvent)>,
}

#[derive(Debug, Default)]
pub(super) struct Recorder {
    last_seq: u64,
    open: Option<OpenChunk>,
}

fn chunk(event: &AgentEvent) -> Option<(ChunkKind, &str)> {
    match event {
        AgentEvent::MessageChunk(text) => Some((ChunkKind::Message, text)),
        AgentEvent::ThoughtChunk(text) => Some((ChunkKind::Thought, text)),
        _ => None,
    }
}

fn truncate(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_owned();
    }
    let mut cut = limit;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}{TRUNCATED_NOTICE}", &text[..cut])
}

pub(super) fn for_store(event: &AgentEvent) -> AgentEvent {
    match event {
        AgentEvent::ToolCallUpdate {
            id,
            status,
            output: Some(output),
            title,
        } if output.len() > STORED_OUTPUT_LIMIT => AgentEvent::ToolCallUpdate {
            id: id.clone(),
            status: *status,
            output: Some(truncate(output, STORED_OUTPUT_LIMIT)),
            title: title.clone(),
        },
        other => other.clone(),
    }
}

impl Recorder {
    pub fn starting_after(last_seq: u64) -> Recorder {
        Recorder {
            last_seq,
            open: None,
        }
    }

    pub fn close(&mut self) -> Option<(u64, AgentEvent)> {
        self.open.take().map(|open| (open.seq, open.event()))
    }

    pub fn last_closed(&self) -> u64 {
        match &self.open {
            Some(open) => open.seq - 1,
            None => self.last_seq,
        }
    }

    pub fn last_seq(&self) -> u64 {
        self.last_seq
    }

    pub fn open_record(&self) -> Option<(u64, AgentEvent)> {
        self.open.as_ref().map(|open| (open.seq, open.event()))
    }

    pub fn record(&mut self, event: &AgentEvent) -> Recorded {
        let mut closed = Vec::new();
        let Some((kind, text)) = chunk(event) else {
            closed.extend(self.close());
            self.last_seq += 1;
            closed.push((self.last_seq, for_store(event)));
            return Recorded {
                seq: self.last_seq,
                closed,
            };
        };
        let continues = self.open.as_ref().is_some_and(|open| open.kind == kind);
        if !continues {
            closed.extend(self.close());
            self.last_seq += 1;
            self.open = Some(OpenChunk {
                seq: self.last_seq,
                kind,
                text: String::new(),
            });
        }
        let seq = self.last_seq;
        let full = self.open.as_mut().is_some_and(|open| {
            open.text.push_str(text);
            open.text.len() >= CHUNK_RECORD_LIMIT
        });
        if full {
            closed.extend(self.close());
        }
        Recorded { seq, closed }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(text: &str) -> AgentEvent {
        AgentEvent::MessageChunk(text.into())
    }

    #[test]
    fn consecutive_chunks_share_one_record() {
        let mut recorder = Recorder::default();
        assert_eq!(recorder.record(&message("a")).seq, 1);
        assert_eq!(recorder.record(&message("b")).seq, 1);
        assert_eq!(recorder.last_closed(), 0);
        assert_eq!(recorder.open_record(), Some((1, message("ab"))));
        let thought = recorder.record(&AgentEvent::ThoughtChunk("t".into()));
        assert_eq!(thought.seq, 2);
        assert_eq!(thought.closed, [(1, message("ab"))]);
        let error = recorder.record(&AgentEvent::Error("x".into()));
        assert_eq!(error.seq, 3);
        assert_eq!(
            error.closed,
            [
                (2, AgentEvent::ThoughtChunk("t".into())),
                (3, AgentEvent::Error("x".into()))
            ]
        );
        assert_eq!(recorder.last_closed(), 3);
        assert!(recorder.close().is_none());
    }

    #[test]
    fn chunk_record_closes_at_four_kilobytes() {
        let mut recorder = Recorder::starting_after(10);
        let piece = "x".repeat(1024);
        let mut closed = Vec::new();
        for _ in 0..5 {
            closed.extend(recorder.record(&message(&piece)).closed);
        }
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].0, 11);
        assert!(matches!(&closed[0].1, AgentEvent::MessageChunk(text) if text.len() == 4096));
        assert_eq!(recorder.open_record().map(|(seq, _)| seq), Some(12));
        assert_eq!(recorder.last_seq(), 12);
    }

    #[test]
    fn long_tool_output_is_truncated_for_storage_only() {
        let output = "é".repeat(10_000);
        let event = AgentEvent::ToolCallUpdate {
            id: "t".into(),
            status: None,
            output: Some(output.clone()),
            title: None,
        };
        let mut recorder = Recorder::default();
        let recorded = recorder.record(&event);
        let AgentEvent::ToolCallUpdate {
            output: Some(stored),
            ..
        } = &recorded.closed[0].1
        else {
            panic!("expected tool update");
        };
        assert!(stored.len() < output.len());
        assert!(stored.ends_with("[saída truncada no histórico]"));
        assert_eq!(for_store(&message("curto")), message("curto"));
    }
}

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use argon_agents::SessionWriter;

#[derive(Clone)]
pub struct SessionStore {
    db_file: PathBuf,
    writer: Arc<Mutex<Option<SessionWriter>>>,
}

impl SessionStore {
    pub fn new(db_file: PathBuf) -> Self {
        Self {
            db_file,
            writer: Arc::default(),
        }
    }

    pub fn writer(&self) -> Result<SessionWriter, String> {
        let mut slot = self.writer.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(writer) = slot.as_ref() {
            return Ok(writer.clone());
        }
        let writer = SessionWriter::open(&self.db_file)?;
        *slot = Some(writer.clone());
        Ok(writer)
    }
}

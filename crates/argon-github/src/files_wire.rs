use serde::Deserialize;

use crate::wire::null_as_default;

#[derive(Deserialize)]
pub(crate) struct WireFile {
    pub(crate) filename: String,
    pub(crate) previous_filename: Option<String>,
    pub(crate) status: Option<String>,
    #[serde(default)]
    pub(crate) additions: u32,
    #[serde(default)]
    pub(crate) deletions: u32,
    pub(crate) patch: Option<String>,
    pub(crate) sha: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct CommitResponse {
    #[serde(default, deserialize_with = "null_as_default")]
    pub(crate) files: Vec<WireFile>,
}

use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::Path;

use sha2::{Digest, Sha256};

use super::PromptError;
use super::template::{TEMPLATE_EXTENSION, io_error};

#[cfg(test)]
mod tests;

pub(super) const DEFAULT_TEMPLATES: [(&str, &str); 5] = [
    (
        "review-padrao",
        include_str!("../../templates/review-padrao.md"),
    ),
    ("seguranca", include_str!("../../templates/seguranca.md")),
    (
        "performance",
        include_str!("../../templates/performance.md"),
    ),
    (
        "plan-padrao",
        include_str!("../../templates/plan-padrao.md"),
    ),
    (
        "resumo-padrao",
        include_str!("../../templates/resumo-padrao.md"),
    ),
];

const PREVIOUS_VERSION_HASHES: [(&str, &[&str]); 5] = [
    (
        "review-padrao",
        &[
            "72ce4cf7ab3244c035c9bff06eae6f55292c43b600c5b5096450b99b13c0b0da",
            "4ee28d1f7461110d20df8556e2b2ed149b8ea5177d3a9b3d30d5a67e98875135",
            "ac307efe3dd13d85417f904aaa333640bf0898250bfd27de8a0970ba89110748",
            "bd53ac457bafdf5151e463924e2afc8547ec4366c1569760f2d87ac33321fd33",
            "ce3136abc8d38a5ef118ae38151d5a6fbf910dbfa6dce6cb6abc35d141bb5615",
        ],
    ),
    (
        "seguranca",
        &[
            "de14dc5bfb0a9a84d04a94fcf28fc053d370f7a4d10a3138ef247b5cb32e18b7",
            "03360ac0fc600db0c00c8123f5de3898c258054d8529f5681d2165c7cfb412a4",
            "38c669ab354baf1ea88bebc6fb2d9e4db767e0da6795297d3d900bff95b2193b",
            "02c0cdedc3e357355acc097b32b1a3ddee2f04ae787de1155c1d64e2a1ca8414",
        ],
    ),
    (
        "performance",
        &[
            "c7559ba71ac9c83cd84f859ba59d08e28755f12ffc236931365e168fb2f704c2",
            "38a7adbd7cb7934fa5cdfcd5ac08e08ebdeb3a5c73ed251a6bcdd85c5cda5eed",
            "33f5c35ae93f4f038331cc44c510742725246a0e226a02d8f0690b2b86a9873f",
            "74782eafa90f6c443baa3d0f5f555396fa3d2c67beafb2b95645927b5f434eec",
        ],
    ),
    ("plan-padrao", &[]),
    (
        "resumo-padrao",
        &[
            "bfde08b631c1e26095680accd8794d769e01f575faca70705a3bdac98e2e96a0",
            "517f97e4d2064776d0573425e0eaab94e78bfb5890e71ad30ec6bf60a9a643d1",
        ],
    ),
];

fn content_hash(content: &str) -> String {
    let normalized = content.replace("\r\n", "\n");
    Sha256::digest(normalized.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn is_previous_version(id: &str, hash: &str) -> bool {
    PREVIOUS_VERSION_HASHES
        .iter()
        .any(|(candidate, hashes)| *candidate == id && hashes.contains(&hash))
}

fn refresh_existing(id: &str, path: &Path, current: &str) -> Result<(), PromptError> {
    let Ok(existing) = fs::read_to_string(path) else {
        tracing::info!(path = %path.display(), "keeping unreadable prompt template untouched");
        return Ok(());
    };
    let hash = content_hash(&existing);
    if hash == content_hash(current) {
        return Ok(());
    }
    if is_previous_version(id, &hash) {
        fs::write(path, current).map_err(io_error(path))?;
        tracing::info!(path = %path.display(), "upgraded built-in prompt template");
    } else {
        tracing::info!(path = %path.display(), "keeping user-edited prompt template");
    }
    Ok(())
}

pub fn ensure_default_templates(dir: &Path) -> Result<(), PromptError> {
    fs::create_dir_all(dir).map_err(io_error(dir))?;
    for (id, content) in DEFAULT_TEMPLATES {
        let path = dir.join(format!("{id}.{TEMPLATE_EXTENSION}"));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => file
                .write_all(content.as_bytes())
                .map_err(io_error(&path))?,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                refresh_existing(id, &path, content)?;
            }
            Err(error) => return Err(io_error(&path)(error)),
        }
    }
    Ok(())
}

use std::fmt::Write as _;
use std::io;
use std::path::{Path, PathBuf};

use argon_core::{PromptError, TemplateSet, ensure_default_templates, scan_templates};

const HEADER_FENCE: &str = "+++";

pub fn template_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.md"))
}

fn toml_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            ch if ch.is_control() => {
                _ = write!(out, "\\u{:04X}", u32::from(ch));
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

pub fn compose(name: &str, description: Option<&str>, body: &str) -> String {
    let mut source = format!("{HEADER_FENCE}\nname = {}\n", toml_string(name));
    if let Some(description) = description.filter(|text| !text.trim().is_empty()) {
        source.push_str(&format!("description = {}\n", toml_string(description)));
    }
    source.push_str(HEADER_FENCE);
    source.push('\n');
    source.push_str(body);
    if !body.ends_with('\n') {
        source.push('\n');
    }
    source
}

pub fn write_atomic(path: &Path, contents: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut temp_name = path.file_name().unwrap_or_default().to_os_string();
    temp_name.push(format!(".{}.tmp", std::process::id()));
    let temp_path = path.with_file_name(temp_name);
    let written =
        std::fs::write(&temp_path, contents).and_then(|()| std::fs::rename(&temp_path, path));
    if written.is_err() {
        _ = std::fs::remove_file(&temp_path);
    }
    written
}

pub fn load(dir: &Path) -> Result<TemplateSet, PromptError> {
    ensure_default_templates(dir)?;
    scan_templates(dir)
}

pub fn delete(dir: &Path, id: &str) -> io::Result<()> {
    std::fs::remove_file(template_path(dir, id))
}

pub fn restore_default(dir: &Path, id: &str) -> Result<(), PromptError> {
    let path = template_path(dir, id);
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(source) => return Err(PromptError::Io { path, source }),
    }
    ensure_default_templates(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("argon-settings-{name}-{}", std::process::id()));
            _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn composed_source_round_trips_through_the_loader() {
        let dir = TempDir::new("compose");
        let name = "Revisão \"rápida\" \\ v2";
        let source = compose(name, Some("Só o essencial"), "Olá {{ pr.title }} {{nonce}}");
        write_atomic(&template_path(&dir.0, "rapida"), &source).unwrap();
        let set = scan_templates(&dir.0).unwrap();
        let template = &set.templates[0];
        assert_eq!(template.id, "rapida");
        assert_eq!(template.name, name);
        assert_eq!(template.description.as_deref(), Some("Só o essencial"));
        assert_eq!(template.body, "Olá {{ pr.title }} {{nonce}}\n");
        assert!(set.invalid.is_empty());
    }

    #[test]
    fn compose_skips_blank_description() {
        let source = compose("x", Some("  "), "body\n");
        assert_eq!(source, "+++\nname = \"x\"\n+++\nbody\n");
    }

    #[test]
    fn write_atomic_leaves_no_temp_files() {
        let dir = TempDir::new("atomic");
        let path = template_path(&dir.0, "a");
        write_atomic(&path, "one").unwrap();
        write_atomic(&path, "two").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "two");
        assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 1);
    }

    #[test]
    fn restore_default_rewrites_the_built_in() {
        let dir = TempDir::new("restore");
        load(&dir.0).unwrap();
        let path = template_path(&dir.0, "seguranca");
        let original = std::fs::read_to_string(&path).unwrap();
        write_atomic(&path, "edited").unwrap();
        restore_default(&dir.0, "seguranca").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    }
}

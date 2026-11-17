use std::fmt::Write as _;
use std::io;
use std::path::{Path, PathBuf};

use argon_core::{
    PromptError, PromptTemplate, TemplateKind, TemplateSet, ensure_default_templates,
    scan_templates,
};

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

pub fn compose(template: &PromptTemplate) -> String {
    let mut source = format!("{HEADER_FENCE}\nname = {}\n", toml_string(&template.name));
    if let Some(description) = template
        .description
        .as_deref()
        .filter(|text| !text.trim().is_empty())
    {
        source.push_str(&format!("description = {}\n", toml_string(description)));
    }
    if template.kind != TemplateKind::Review {
        source.push_str(&format!("kind = {}\n", toml_string(template.kind.as_str())));
    }
    if let Some(base) = &template.base {
        source.push_str(&format!("base = {}\n", toml_string(base)));
    }
    if template.detached {
        source.push_str("detached = true\n");
    }
    if let Some(version) = template.version {
        source.push_str(&format!("argon_template_version = {version}\n"));
    }
    source.push_str(HEADER_FENCE);
    source.push('\n');
    source.push_str(&template.body);
    if !template.body.ends_with('\n') {
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

    fn draft(name: &str, description: Option<&str>, body: &str) -> PromptTemplate {
        PromptTemplate {
            id: "draft".into(),
            name: name.into(),
            description: description.map(str::to_owned),
            body: body.into(),
            ..PromptTemplate::default()
        }
    }

    #[test]
    fn composed_source_round_trips_through_the_loader() {
        let dir = TempDir::new("compose");
        let name = "Revisão \"rápida\" \\ v2";
        let source = compose(&draft(
            name,
            Some("Só o essencial"),
            "Olá {{ pr.title }} {{nonce}}",
        ));
        write_atomic(&template_path(&dir.0, "rapida"), &source).unwrap();
        let set = scan_templates(&dir.0).unwrap();
        let template = &set.templates[0];
        assert_eq!(template.id, "rapida");
        assert_eq!(template.name, name);
        assert_eq!(template.description.as_deref(), Some("Só o essencial"));
        assert_eq!(template.kind, TemplateKind::Review);
        assert_eq!(template.version, None);
        assert_eq!(template.body, "Olá {{ pr.title }} {{nonce}}\n");
        assert!(set.invalid.is_empty());
    }

    #[test]
    fn composed_profile_keeps_base_and_detached_through_the_loader() {
        let dir = TempDir::new("compose-profile");
        let following = PromptTemplate {
            base: Some("review-padrao".into()),
            ..draft("Perfil API", None, "Olhe as migrações.\n")
        };
        let detached = PromptTemplate {
            base: Some("seguranca".into()),
            detached: true,
            ..draft("Perfil web", None, "Início\n\nFim\n")
        };
        let plain = compose(&draft("Comum", None, "{{nonce}}\n"));
        assert!(!plain.contains("base =") && !plain.contains("detached ="));
        write_atomic(&template_path(&dir.0, "perfil-api"), &compose(&following)).unwrap();
        write_atomic(&template_path(&dir.0, "perfil-web"), &compose(&detached)).unwrap();
        write_atomic(&template_path(&dir.0, "comum"), &plain).unwrap();
        let set = scan_templates(&dir.0).unwrap();
        let find = |id: &str| set.templates.iter().find(|t| t.id == id).unwrap();
        assert_eq!(
            find("perfil-api"),
            &PromptTemplate {
                id: "perfil-api".into(),
                ..following
            }
        );
        assert_eq!(
            find("perfil-web"),
            &PromptTemplate {
                id: "perfil-web".into(),
                ..detached
            }
        );
        assert_eq!(find("comum").base, None);
        assert!(!find("comum").detached);
    }

    #[test]
    fn compose_skips_blank_description() {
        let source = compose(&draft("x", Some("  "), "body\n"));
        assert_eq!(source, "+++\nname = \"x\"\n+++\nbody\n");
    }

    #[test]
    fn compose_preserves_kind_and_template_version() {
        let dir = TempDir::new("compose-kind");
        let plan = PromptTemplate {
            kind: TemplateKind::Plan,
            version: Some(7),
            ..draft(
                "Meu plano",
                Some("Plano curto"),
                "Plano da #{{issue.number}}\n",
            )
        };
        let source = compose(&plan);
        assert_eq!(
            source,
            "+++\nname = \"Meu plano\"\ndescription = \"Plano curto\"\nkind = \"plan\"\nargon_template_version = 7\n+++\nPlano da #{{issue.number}}\n"
        );
        write_atomic(&template_path(&dir.0, "meu-plano"), &source).unwrap();
        let set = scan_templates(&dir.0).unwrap();
        assert_eq!(
            set.templates,
            [PromptTemplate {
                id: "meu-plano".into(),
                ..plan
            }]
        );
    }

    #[test]
    fn composing_a_saved_built_in_reproduces_its_file() {
        let dir = TempDir::new("compose-built-ins");
        let set = load(&dir.0).unwrap();
        assert_eq!(set.templates.len(), 4);
        for template in &set.templates {
            let saved = std::fs::read_to_string(template_path(&dir.0, &template.id)).unwrap();
            assert_eq!(compose(template), saved, "{}", template.id);
        }
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

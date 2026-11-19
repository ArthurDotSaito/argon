use argon_core::TemplateKind;

const MAX_NAME_CHARS: usize = 80;
const MAX_ID_CHARS: usize = 48;
const FALLBACK_ID: &str = "template";
const RESERVED_IDS: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

pub const BUILT_IN_IDS: [&str; 5] = [
    "review-padrao",
    "seguranca",
    "performance",
    "plan-padrao",
    "resumo-padrao",
];

const REVIEW_VARIABLES: [&str; 19] = [
    "pr.title",
    "pr.number",
    "pr.url",
    "pr.body",
    "pr.repo",
    "pr.base",
    "pr.head",
    "pr.sha",
    "pr.threads",
    "repo",
    "base",
    "base_sha",
    "head",
    "diff",
    "diff_mode",
    "files",
    "previous_reviews",
    "ci_failures",
    "nonce",
];

const PLAN_VARIABLES: [&str; 12] = [
    "issue.title",
    "issue.number",
    "issue.url",
    "issue.body",
    "issue.repo",
    "issue.labels",
    "issue.assignees",
    "issue.milestone",
    "issue.comments",
    "base",
    "base_sha",
    "nonce",
];

const SUMMARY_VARIABLES: [&str; 13] = [
    "pr.title",
    "pr.number",
    "pr.url",
    "pr.body",
    "pr.repo",
    "base",
    "base_sha",
    "head",
    "head_sha",
    "related",
    "project_overview",
    "refresh_project",
    "nonce",
];

pub fn variables(kind: TemplateKind) -> &'static [&'static str] {
    match kind {
        TemplateKind::Review => &REVIEW_VARIABLES,
        TemplateKind::Plan => &PLAN_VARIABLES,
        TemplateKind::Summary => &SUMMARY_VARIABLES,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameError {
    Empty,
    TooLong,
    Multiline,
}

impl NameError {
    pub fn message(self) -> &'static str {
        match self {
            Self::Empty => "O nome não pode ficar vazio.",
            Self::TooLong => "O nome passa de 80 caracteres.",
            Self::Multiline => "O nome deve ter uma linha só.",
        }
    }
}

pub fn validate_name(name: &str) -> Result<String, NameError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(NameError::Empty);
    }
    if trimmed.contains(['\n', '\r']) {
        return Err(NameError::Multiline);
    }
    if trimmed.chars().count() > MAX_NAME_CHARS {
        return Err(NameError::TooLong);
    }
    Ok(trimmed.to_owned())
}

fn fold_accent(ch: char) -> char {
    match ch {
        'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'í' | 'ì' | 'î' | 'ï' => 'i',
        'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
        'ú' | 'ù' | 'û' | 'ü' => 'u',
        'ç' => 'c',
        'ñ' => 'n',
        other => other,
    }
}

pub fn slugify(name: &str) -> String {
    let mut slug = String::new();
    for ch in name.to_lowercase().chars().map(fold_accent) {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug: String = slug.chars().take(MAX_ID_CHARS).collect();
    let slug = slug.trim_end_matches('-');
    if is_valid_id(slug) {
        slug.to_owned()
    } else {
        FALLBACK_ID.to_owned()
    }
}

pub fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_CHARS
        && id
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_')
        && !id.starts_with(['-', '_'])
        && !RESERVED_IDS.contains(&id)
}

pub fn unique_id<'a>(base: &str, existing: impl IntoIterator<Item = &'a str> + Clone) -> String {
    let taken = |candidate: &str| {
        existing
            .clone()
            .into_iter()
            .any(|id| id.eq_ignore_ascii_case(candidate))
    };
    if !taken(base) {
        return base.to_owned();
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|candidate| !taken(candidate))
        .unwrap_or_else(|| base.to_owned())
}

pub fn is_built_in(id: &str) -> bool {
    BUILT_IN_IDS.contains(&id)
}

pub fn variable_token(name: &str) -> String {
    format!("{{{{{name}}}}}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_trimmed_and_validated() {
        assert_eq!(validate_name("  Segurança  ").as_deref(), Ok("Segurança"));
        assert_eq!(validate_name("   "), Err(NameError::Empty));
        assert_eq!(validate_name("a\nb"), Err(NameError::Multiline));
        assert_eq!(validate_name(&"x".repeat(81)), Err(NameError::TooLong));
        assert!(validate_name(&"ç".repeat(80)).is_ok());
    }

    #[test]
    fn slugify_folds_accents_and_separators() {
        assert_eq!(slugify("Revisão de Segurança"), "revisao-de-seguranca");
        assert_eq!(slugify("  API -- v2!! "), "api-v2");
        assert_eq!(slugify("***"), "template");
        assert_eq!(slugify("CON"), "template");
        assert!(slugify(&"a".repeat(100)).len() <= 48);
    }

    #[test]
    fn ids_only_accept_safe_file_names() {
        assert!(is_valid_id("review-padrao"));
        assert!(is_valid_id("perf_2"));
        assert!(!is_valid_id(""));
        assert!(!is_valid_id("-x"));
        assert!(!is_valid_id("Upper"));
        assert!(!is_valid_id("a/b"));
        assert!(!is_valid_id("..\\x"));
        assert!(!is_valid_id("nul"));
    }

    #[test]
    fn unique_id_appends_a_counter_ignoring_case() {
        let existing = ["seguranca", "Seguranca-2", "outro"];
        assert_eq!(unique_id("seguranca", existing), "seguranca-3");
        assert_eq!(unique_id("novo", existing), "novo");
    }

    #[test]
    fn built_ins_are_known() {
        assert!(BUILT_IN_IDS.iter().all(|id| is_built_in(id)));
        assert!(is_built_in("plan-padrao"));
        assert!(!is_built_in("meu-template"));
    }

    #[test]
    fn variable_token_wraps_in_double_braces() {
        assert_eq!(variable_token("pr.title"), "{{pr.title}}");
        assert_eq!(variable_token("nonce"), "{{nonce}}");
    }

    #[test]
    fn variable_chips_follow_the_template_kind() {
        let review = variables(TemplateKind::Review);
        let plan = variables(TemplateKind::Plan);
        assert!(review.contains(&"pr.title") && !review.contains(&"issue.title"));
        assert!(plan.contains(&"issue.title") && !plan.contains(&"pr.title"));
        assert!(plan.contains(&"issue.comments") && !plan.contains(&"diff"));
        assert!(review.contains(&"nonce") && plan.contains(&"nonce"));
    }
}

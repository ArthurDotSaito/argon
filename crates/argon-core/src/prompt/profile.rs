use super::{PromptError, PromptTemplate};

pub const PROFILE_SLOT: &str = "{# argon:perfil #}";
pub const PROFILE_END: &str = "{# argon:fim-perfil #}";

const NEWLINES: [char; 2] = ['\r', '\n'];
const BLOCK_SEPARATOR: &str = "\n\n";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProfileParts {
    pub head: String,
    pub middle: String,
    pub tail: String,
}

pub fn base_parts(body: &str) -> (String, String) {
    match body.split_once(PROFILE_SLOT) {
        Some((head, tail)) => (
            head.trim_end().to_owned(),
            tail.trim_start_matches(NEWLINES).to_owned(),
        ),
        None => (body.trim_end().to_owned(), String::new()),
    }
}

pub fn base_body(head: &str, tail: &str) -> String {
    let head = head.trim_end();
    let tail = tail.trim_start_matches(NEWLINES);
    if tail.is_empty() {
        return format!("{head}{BLOCK_SEPARATOR}{PROFILE_SLOT}\n");
    }
    with_final_newline(format!(
        "{head}{BLOCK_SEPARATOR}{PROFILE_SLOT}{BLOCK_SEPARATOR}{tail}"
    ))
}

pub fn profile_parts(
    template: &PromptTemplate,
    all: &[PromptTemplate],
) -> Result<ProfileParts, PromptError> {
    let Some(base_id) = template.base.as_deref() else {
        let (head, tail) = base_parts(&template.body);
        return Ok(ProfileParts {
            head,
            middle: String::new(),
            tail,
        });
    };
    if template.detached {
        return Ok(detached_parts(&template.body));
    }
    let base = all
        .iter()
        .find(|candidate| candidate.id == base_id)
        .ok_or_else(|| base_error(template, |id, base| PromptError::UnknownBase { id, base }))?;
    if base.base.is_some() {
        return Err(base_error(template, |id, base| PromptError::NestedBase {
            id,
            base,
        }));
    }
    if base.kind != template.kind {
        return Err(base_error(template, |id, base| {
            PromptError::BaseKindMismatch { id, base }
        }));
    }
    let (head, tail) = base_parts(&base.body);
    Ok(ProfileParts {
        head,
        middle: template.body.trim().to_owned(),
        tail,
    })
}

pub fn detached_body(parts: &ProfileParts) -> String {
    let head = parts.head.trim_end();
    let middle = parts.middle.trim();
    let tail = parts.tail.trim_start_matches(NEWLINES);
    let section = format!("{head}{BLOCK_SEPARATOR}{PROFILE_SLOT}\n{middle}\n{PROFILE_END}\n");
    if tail.is_empty() {
        section
    } else {
        with_final_newline(format!("{section}\n{tail}"))
    }
}

pub fn resolve_template(
    template: &PromptTemplate,
    all: &[PromptTemplate],
) -> Result<PromptTemplate, PromptError> {
    let mut resolved = template.clone();
    if template.base.is_none() && !template.body.contains(PROFILE_SLOT) {
        return Ok(resolved);
    }
    resolved.body = compose(&profile_parts(template, all)?);
    resolved.base = None;
    resolved.detached = false;
    Ok(resolved)
}

fn base_error(
    template: &PromptTemplate,
    build: impl FnOnce(String, String) -> PromptError,
) -> PromptError {
    build(
        template.id.clone(),
        template.base.clone().unwrap_or_default(),
    )
}

fn detached_parts(body: &str) -> ProfileParts {
    let Some((head, after_slot)) = body.split_once(PROFILE_SLOT) else {
        return ProfileParts {
            head: body.trim_end().to_owned(),
            ..ProfileParts::default()
        };
    };
    let (middle, tail) = after_slot
        .split_once(PROFILE_END)
        .unwrap_or((after_slot, ""));
    ProfileParts {
        head: head.trim_end().to_owned(),
        middle: middle.trim().to_owned(),
        tail: tail.trim_start_matches(NEWLINES).to_owned(),
    }
}

fn compose(parts: &ProfileParts) -> String {
    let pieces = [
        parts.head.trim_end(),
        parts.middle.trim(),
        parts.tail.trim_start_matches(NEWLINES),
    ];
    let joined = pieces
        .into_iter()
        .filter(|piece| !piece.is_empty())
        .collect::<Vec<_>>()
        .join(BLOCK_SEPARATOR);
    with_final_newline(joined)
}

fn with_final_newline(mut text: String) -> String {
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

#[cfg(test)]
mod tests;

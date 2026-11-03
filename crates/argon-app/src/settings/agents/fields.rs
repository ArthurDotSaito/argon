use std::collections::BTreeMap;

const MASK: &str = "••••••";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldError {
    UnclosedQuote,
    EmptyKey,
    InvalidKey(String),
    DuplicateKey(String),
}

impl FieldError {
    pub fn message(&self) -> String {
        match self {
            Self::UnclosedQuote => "Args: aspas sem fechar.".to_owned(),
            Self::EmptyKey => "Env: uma variável com valor está sem nome.".to_owned(),
            Self::InvalidKey(key) => {
                format!("Env: `{key}` não é um nome válido (letras, números e _).")
            }
            Self::DuplicateKey(key) => format!("Env: `{key}` aparece mais de uma vez."),
        }
    }
}

pub fn split_args(text: &str) -> Result<Vec<String>, FieldError> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_arg = false;
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' if quoted && chars.peek() == Some(&'"') => {
                current.push('"');
                chars.next();
            }
            '"' => {
                quoted = !quoted;
                in_arg = true;
            }
            ch if ch.is_whitespace() && !quoted => {
                if in_arg {
                    args.push(std::mem::take(&mut current));
                    in_arg = false;
                }
            }
            ch => {
                current.push(ch);
                in_arg = true;
            }
        }
    }
    if quoted {
        return Err(FieldError::UnclosedQuote);
    }
    if in_arg {
        args.push(current);
    }
    Ok(args)
}

fn quote_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains(|ch: char| ch.is_whitespace() || ch == '"') {
        return arg.to_owned();
    }
    format!("\"{}\"", arg.replace('"', "\\\""))
}

pub fn join_args(args: &[String]) -> String {
    args.iter()
        .map(|arg| quote_arg(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_env_key(key: &str) -> bool {
    let mut chars = key.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

pub fn parse_env(rows: &[(String, String)]) -> Result<BTreeMap<String, String>, FieldError> {
    let mut env = BTreeMap::new();
    for (key, value) in rows {
        let key = key.trim();
        if key.is_empty() {
            if value.is_empty() {
                continue;
            }
            return Err(FieldError::EmptyKey);
        }
        if !is_env_key(key) {
            return Err(FieldError::InvalidKey(key.to_owned()));
        }
        if env
            .keys()
            .any(|existing: &String| existing.eq_ignore_ascii_case(key))
        {
            return Err(FieldError::DuplicateKey(key.to_owned()));
        }
        env.insert(key.to_owned(), value.clone());
    }
    Ok(env)
}

pub fn mask(value: &str) -> &'static str {
    if value.is_empty() { "" } else { MASK }
}

pub fn env_summary(env: &BTreeMap<String, String>) -> String {
    env.iter()
        .map(|(key, value)| format!("{key}={}", mask(value)))
        .collect::<Vec<_>>()
        .join("  ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    #[test]
    fn split_args_honours_quotes() {
        assert_eq!(
            split_args(r#"-y "C:\Program Files\x" --flag="a b"  acp"#).unwrap(),
            strings(&["-y", r"C:\Program Files\x", "--flag=a b", "acp"])
        );
        assert_eq!(split_args("  ").unwrap(), Vec::<String>::new());
        assert_eq!(split_args(r#""""#).unwrap(), strings(&[""]));
        assert_eq!(
            split_args(r#""say \"hi\"""#).unwrap(),
            strings(&[r#"say "hi""#])
        );
        assert_eq!(split_args(r#"-m "open"#), Err(FieldError::UnclosedQuote));
    }

    #[test]
    fn join_args_round_trips() {
        let args = strings(&["-y", r"C:\Program Files\x", "", r#"say "hi""#, "acp"]);
        assert_eq!(split_args(&join_args(&args)).unwrap(), args);
        assert_eq!(join_args(&strings(&["acp", "--port=0"])), "acp --port=0");
    }

    #[test]
    fn parse_env_validates_keys() {
        let row = |key: &str, value: &str| (key.to_owned(), value.to_owned());
        let env = parse_env(&[row(" API_KEY ", "s3cr3t"), row("", ""), row("_X1", "")]).unwrap();
        assert_eq!(env.len(), 2);
        assert_eq!(env["API_KEY"], "s3cr3t");
        assert_eq!(parse_env(&[row("", "v")]), Err(FieldError::EmptyKey));
        assert_eq!(
            parse_env(&[row("1ABC", "v")]),
            Err(FieldError::InvalidKey("1ABC".into()))
        );
        assert_eq!(
            parse_env(&[row("A-B", "v")]),
            Err(FieldError::InvalidKey("A-B".into()))
        );
        assert_eq!(
            parse_env(&[row("Path", "a"), row("PATH", "b")]),
            Err(FieldError::DuplicateKey("PATH".into()))
        );
    }

    #[test]
    fn env_values_are_masked_without_leaking_length() {
        assert_eq!(mask(""), "");
        assert_eq!(mask("x"), mask("a-very-long-secret-token"));
        let env = BTreeMap::from([
            ("TOKEN".to_owned(), "abc123".to_owned()),
            ("EMPTY".to_owned(), String::new()),
        ]);
        let summary = env_summary(&env);
        assert!(!summary.contains("abc123"));
        assert_eq!(summary, format!("EMPTY=  TOKEN={MASK}"));
    }
}

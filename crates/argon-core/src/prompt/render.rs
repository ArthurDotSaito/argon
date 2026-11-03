use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;

use minijinja::value::{Enumerator, Object, ObjectRepr, Value};
use minijinja::{Environment, Template, UndefinedBehavior, context};

use super::{PromptError, PromptTemplate};

const KNOWN_VARIABLES: [&str; 12] = [
    "pr",
    "pr.title",
    "pr.number",
    "pr.url",
    "pr.body",
    "repo",
    "base",
    "head",
    "diff",
    "files",
    "previous_reviews",
    "ci_failures",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptContext {
    pub pr_title: String,
    pub pr_number: u64,
    pub pr_url: String,
    pub pr_body: String,
    pub repo: String,
    pub base: String,
    pub head: String,
    pub diff: String,
    pub files: Vec<String>,
    pub previous_reviews: String,
    pub ci_failures: String,
}

#[derive(Debug)]
struct FileList(Vec<String>);

impl Object for FileList {
    fn repr(self: &Arc<Self>) -> ObjectRepr {
        ObjectRepr::Seq
    }

    fn get_value(self: &Arc<Self>, key: &Value) -> Option<Value> {
        let file = self.0.get(key.as_usize()?)?;
        Some(Value::from(file.as_str()))
    }

    fn enumerate(self: &Arc<Self>) -> Enumerator {
        Enumerator::Seq(self.0.len())
    }

    fn render(self: &Arc<Self>, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.join("\n"))
    }
}

fn truncate_diff(diff: &str, base: &str, max_chars: usize) -> String {
    let Some((cut, _)) = diff.char_indices().nth(max_chars) else {
        return diff.to_owned();
    };
    let head = &diff[..cut];
    let kept = if diff[cut..].starts_with('\n') {
        head
    } else {
        head.rfind('\n').map_or("", |newline| &head[..newline])
    };
    let notice =
        format!("[diff truncado — rode `git diff {base}...HEAD` no worktree para ver o resto]");
    if kept.is_empty() {
        notice
    } else {
        format!("{kept}\n{notice}")
    }
}

fn unknown_variable(template: &Template<'_, '_>) -> Option<String> {
    let known: HashSet<&str> = KNOWN_VARIABLES.into_iter().collect();
    template
        .undeclared_variables(true)
        .into_iter()
        .filter(|name| !known.contains(name.as_str()))
        .min()
}

pub fn render_prompt(
    template: &PromptTemplate,
    ctx: &PromptContext,
    max_diff_chars: usize,
) -> Result<String, PromptError> {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.set_keep_trailing_newline(true);
    let compiled = env.template_from_str(&template.body)?;
    if let Some(name) = unknown_variable(&compiled) {
        return Err(PromptError::UnknownVariable { name });
    }
    let values = context! {
        pr => context! {
            title => ctx.pr_title,
            number => ctx.pr_number,
            url => ctx.pr_url,
            body => ctx.pr_body,
        },
        repo => ctx.repo,
        base => ctx.base,
        head => ctx.head,
        diff => truncate_diff(&ctx.diff, &ctx.base, max_diff_chars),
        files => Value::from_object(FileList(ctx.files.clone())),
        previous_reviews => ctx.previous_reviews,
        ci_failures => ctx.ci_failures,
    };
    Ok(compiled.render(values)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template(body: &str) -> PromptTemplate {
        PromptTemplate {
            id: "t".into(),
            name: "T".into(),
            description: None,
            body: body.into(),
        }
    }

    fn context() -> PromptContext {
        PromptContext {
            pr_title: "Fix login".into(),
            pr_number: 42,
            pr_url: "https://github.com/acme/api/pull/42".into(),
            pr_body: "Corrige o login".into(),
            repo: "acme/api".into(),
            base: "main".into(),
            head: "fix/login".into(),
            diff: "+a\n-b\n".into(),
            files: vec!["src/a.rs".into(), "src/b.rs".into()],
            previous_reviews: "alguém pediu testes".into(),
            ci_failures: "build falhou".into(),
        }
    }

    #[test]
    fn renders_every_variable() {
        let body = "{{pr.title}}|{{pr.number}}|{{pr.url}}|{{pr.body}}|{{repo}}|{{base}}|{{head}}|{{diff}}|{{files}}|{{previous_reviews}}|{{ci_failures}}";

        let rendered = render_prompt(&template(body), &context(), 1000).unwrap();

        assert_eq!(
            rendered,
            "Fix login|42|https://github.com/acme/api/pull/42|Corrige o login|acme/api|main|fix/login|+a\n-b\n|src/a.rs\nsrc/b.rs|alguém pediu testes|build falhou"
        );
    }

    #[test]
    fn files_can_be_iterated_and_checked() {
        let body =
            "{% for f in files %}- {{f}}\n{% endfor %}{% if previous_reviews %}tem{% endif %}";

        let rendered = render_prompt(&template(body), &context(), 1000).unwrap();

        assert_eq!(rendered, "- src/a.rs\n- src/b.rs\ntem");
    }

    #[test]
    fn empty_optional_sections_are_falsy() {
        let mut ctx = context();
        ctx.ci_failures.clear();
        ctx.files.clear();
        let body = "{% if ci_failures %}ci{% endif %}{% if files %}files{% endif %}fim";

        assert_eq!(render_prompt(&template(body), &ctx, 1000).unwrap(), "fim");
    }

    #[test]
    fn keeps_trailing_newline() {
        let rendered = render_prompt(&template("{{repo}}\n"), &context(), 1000).unwrap();

        assert_eq!(rendered, "acme/api\n");
    }

    #[test]
    fn truncates_diff_at_line_boundary_with_notice() {
        let mut ctx = context();
        ctx.diff = "aaa\nbbb\nccc\nddd\n".into();

        let rendered = render_prompt(&template("{{diff}}"), &ctx, 10).unwrap();

        assert_eq!(
            rendered,
            "aaa\nbbb\n[diff truncado — rode `git diff main...HEAD` no worktree para ver o resto]"
        );
    }

    #[test]
    fn keeps_whole_line_when_limit_ends_exactly_on_line_end() {
        let mut ctx = context();
        ctx.diff = "aaa\nbbb\nccc\n".into();

        let rendered = render_prompt(&template("{{diff}}"), &ctx, 7).unwrap();

        assert!(rendered.starts_with("aaa\nbbb\n[diff truncado"));
    }

    #[test]
    fn truncation_is_safe_with_multibyte_characters() {
        let mut ctx = context();
        ctx.diff = "ação\nção\nnão\n".into();

        let rendered = render_prompt(&template("{{diff}}"), &ctx, 6).unwrap();

        assert!(rendered.starts_with("ação\n[diff truncado"));
    }

    #[test]
    fn truncation_notice_alone_when_first_line_exceeds_limit() {
        let mut ctx = context();
        ctx.diff = "uma linha muito longa\n".into();

        let rendered = render_prompt(&template("{{diff}}"), &ctx, 5).unwrap();

        assert!(rendered.starts_with("[diff truncado"));
    }

    #[test]
    fn leaves_diff_untouched_within_limit() {
        let ctx = context();
        let limit = ctx.diff.chars().count();

        let rendered = render_prompt(&template("{{diff}}"), &ctx, limit).unwrap();

        assert_eq!(rendered, ctx.diff);
    }

    #[test]
    fn unknown_variable_reports_its_name() {
        let error =
            render_prompt(&template("{{repo}} {{pr.titulo}}"), &context(), 1000).unwrap_err();

        assert!(matches!(&error, PromptError::UnknownVariable { name } if name == "pr.titulo"));
        assert!(error.to_string().contains("pr.titulo"));
    }

    #[test]
    fn unknown_top_level_variable_reports_its_name() {
        let error = render_prompt(
            &template("{% if x %}{{autor}}{% endif %}"),
            &context(),
            1000,
        )
        .unwrap_err();

        assert!(matches!(&error, PromptError::UnknownVariable { name } if name == "autor"));
    }

    #[test]
    fn syntax_error_is_a_render_error() {
        let error = render_prompt(&template("{{repo"), &context(), 1000).unwrap_err();

        assert!(matches!(error, PromptError::Render(_)));
    }
}

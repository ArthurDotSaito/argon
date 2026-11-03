use std::path::Path;

use argon_core::{
    PrId, PromptContext, RenderedPrompt, ReviewCfg, ensure_default_templates, load_templates,
    render_prompt,
};
use argon_github::{PrDetail, PrFile};
use tokio::process::Command;

use crate::{BoxError, TEMPLATE_ID};
pub async fn node_id(repo: &str, number: u64) -> Result<PrId, BoxError> {
    let output = Command::new("gh")
        .args([
            "api",
            &format!("repos/{repo}/pulls/{number}"),
            "--jq",
            ".node_id",
        ])
        .output()
        .await?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr)
            .trim()
            .to_owned()
            .into());
    }
    Ok(PrId(String::from_utf8(output.stdout)?.trim().to_owned()))
}

fn unified_diff(files: &[PrFile]) -> String {
    files
        .iter()
        .filter_map(|file| {
            let patch = file.patch.as_ref()?;
            let old = file.previous_path.as_deref().unwrap_or(&file.path);
            Some(format!(
                "diff --git a/{old} b/{path}\n--- a/{old}\n+++ b/{path}\n{patch}\n",
                path = file.path
            ))
        })
        .collect()
}

pub fn render(
    detail: &PrDetail,
    files: &[PrFile],
    templates_dir: &Path,
) -> Result<RenderedPrompt, BoxError> {
    ensure_default_templates(templates_dir)?;
    let template = load_templates(templates_dir)?
        .into_iter()
        .find(|template| template.id == TEMPLATE_ID)
        .ok_or("template review-padrao not found")?;
    let ctx = PromptContext {
        pr_title: detail.pr.title.clone(),
        pr_number: detail.pr.number,
        pr_url: detail.pr.url.clone(),
        pr_body: detail.body.clone(),
        repo: detail.pr.repo.clone(),
        base: detail.pr.base.clone(),
        base_sha: None,
        head: detail.pr.head.clone(),
        head_sha: detail.head_oid.clone(),
        diff: unified_diff(files),
        files: files.iter().map(|file| file.path.clone()).collect(),
        file_stats: files
            .iter()
            .map(|file| (file.path.clone(), file.additions, file.deletions))
            .collect(),
        previous_reviews: String::new(),
        ci_failures: String::new(),
    };
    Ok(render_prompt(
        &template,
        &ctx,
        ReviewCfg::default().max_diff_chars,
    )?)
}

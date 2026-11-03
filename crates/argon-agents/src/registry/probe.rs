use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;

use crate::process::spawn_in_job;

pub(super) struct Captured {
    pub success: bool,
    pub text: String,
}

pub(super) async fn capture(
    program: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<Captured, String> {
    let description = format!("{} {}", program.display(), args.join(" "));
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = spawn_in_job(&mut command)
        .map_err(|error| format!("failed to run `{description}`: {error}"))?;
    let output = tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .map_err(|_| format!("`{description}` timed out after {}s", timeout.as_secs()))?
        .map_err(|error| format!("failed to run `{description}`: {error}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    Ok(Captured {
        success: output.status.success(),
        text: format!("{stdout}\n{stderr}"),
    })
}

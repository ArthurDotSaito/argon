use std::collections::BTreeMap;
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
    capture_with_env(program, args, &BTreeMap::new(), timeout).await
}

pub(super) async fn capture_with_env(
    program: &Path,
    args: &[&str],
    env: &BTreeMap<String, String>,
    timeout: Duration,
) -> Result<Captured, String> {
    let description = format!("{} {}", program.display(), args.join(" "));
    let mut command = Command::new(program);
    command
        .args(args)
        .envs(env)
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

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn capture_with_env_passes_the_environment() {
        let env = BTreeMap::from([("ARGON_TEST_ENV".to_owned(), "from-the-spec".to_owned())]);
        let captured = capture_with_env(
            Path::new("cmd"),
            &["/c", "echo", "%ARGON_TEST_ENV%"],
            &env,
            Duration::from_secs(10),
        )
        .await
        .unwrap();
        assert!(captured.success);
        assert!(captured.text.contains("from-the-spec"), "{}", captured.text);
    }

    #[tokio::test]
    async fn capture_leaves_the_environment_alone() {
        let captured = capture(
            Path::new("cmd"),
            &["/c", "echo", "%ARGON_TEST_ENV_UNSET%"],
            Duration::from_secs(10),
        )
        .await
        .unwrap();
        assert!(captured.text.contains("%ARGON_TEST_ENV_UNSET%"));
    }
}

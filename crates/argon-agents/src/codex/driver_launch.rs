use std::path::{Path, PathBuf};
use std::sync::Arc;

use argon_core::{PermissionMode, SessionKind};
use serde_json::{Value, json};
use tokio::sync::mpsc;

use super::state::{
    ModelPages, Sandbox, common_dir, effective_mode, gitdir_target, is_archived, project_roots,
    readiness, trust_keys,
};
use crate::codex::protocol::{
    initialize_params, parse_models, resume_params, thread_id, thread_model, thread_params,
    with_untrusted_projects,
};
use crate::jsonrpc::{Incoming, RpcClient};
use crate::process::KillOnDropChild;
use crate::registry::{AgentSpec, ModelInfo};
use crate::session::{Result, SessionError, StderrTail, start_agent, terminate};
use crate::util::{Resolved, resolve_command};

const APP_SERVER_ARG: &str = "app-server";
const GIT_MARKER: &str = ".git";
const COMMONDIR_FILE: &str = "commondir";
const MISSING_THREAD: &str = "o Codex não devolveu o id da conversa";

pub(super) type Tagged = (u64, Feed);

pub(super) enum Feed {
    Message(Incoming),
    Closed,
}

pub(super) enum Open<'a> {
    Start,
    Resume(&'a str),
}

pub(super) struct ThreadSetup<'a> {
    pub cwd: &'a Path,
    pub model: Option<&'a str>,
    pub mode: PermissionMode,
    pub kind: SessionKind,
    pub untrusted: &'a [String],
}

pub(super) struct Started {
    pub thread: String,
    pub model: Option<String>,
    pub mode: PermissionMode,
    pub sandbox: Sandbox,
}

pub(super) struct Workspace {
    pub cwd: PathBuf,
    pub untrusted: Vec<String>,
}

fn main_worktree_root(root: &Path) -> Option<PathBuf> {
    let gitfile = std::fs::read_to_string(root.join(GIT_MARKER)).ok()?;
    let gitdir = gitdir_target(&gitfile, root)?;
    let commondir = std::fs::read_to_string(gitdir.join(COMMONDIR_FILE)).ok()?;
    let common = dunce::canonicalize(common_dir(&gitdir, &commondir)?).ok()?;
    common.parent().map(Path::to_path_buf)
}

pub(super) fn workspace(cwd: &Path) -> Workspace {
    let canonical = dunce::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let mut roots: Vec<PathBuf> = project_roots(&canonical, |dir| dir.join(GIT_MARKER).exists())
        .into_iter()
        .map(Path::to_path_buf)
        .collect();
    if let Some(main) = roots.last().and_then(|root| main_worktree_root(root)) {
        roots.push(main);
    }
    Workspace {
        untrusted: trust_keys(&roots),
        cwd: canonical,
    }
}

fn forward(
    mut incoming: mpsc::UnboundedReceiver<Incoming>,
    generation: u64,
    feed: mpsc::UnboundedSender<Tagged>,
) {
    tokio::spawn(async move {
        while let Some(message) = incoming.recv().await {
            if feed.send((generation, Feed::Message(message))).is_err() {
                return;
            }
        }
        let _ = feed.send((generation, Feed::Closed));
    });
}

fn check_command(spec: &AgentSpec) -> Result<()> {
    match resolve_command(&spec.command) {
        Resolved::Path(_) => Ok(()),
        Resolved::Missing => Err(SessionError::CommandNotFound(
            spec.command.display().to_string(),
        )),
        Resolved::Failed(message) => Err(SessionError::Agent(message)),
    }
}

pub(super) struct Codex {
    child: KillOnDropChild,
    pub rpc: RpcClient,
    pub generation: u64,
}

impl Codex {
    pub fn spawn(
        spec: &AgentSpec,
        cwd: &Path,
        tail: Arc<StderrTail>,
        generation: u64,
        feed: &mpsc::UnboundedSender<Tagged>,
    ) -> Result<Codex> {
        check_command(spec)?;
        let mut args = spec.args.clone();
        args.push(APP_SERVER_ARG.to_owned());
        let spec = AgentSpec {
            args,
            ..spec.clone()
        };
        let (child, stdin, stdout) = start_agent(&spec, cwd, tail)?;
        let (rpc, incoming) = RpcClient::new(stdin, stdout);
        forward(incoming, generation, feed.clone());
        Ok(Codex {
            child,
            rpc,
            generation,
        })
    }

    pub async fn stop(self) -> Option<i32> {
        let Codex { mut child, rpc, .. } = self;
        drop(rpc);
        terminate(&mut child).await;
        child.wait().await.ok().and_then(|status| status.code())
    }
}

async fn handshake(rpc: &RpcClient) -> std::result::Result<(), String> {
    rpc.call("initialize", initialize_params(env!("CARGO_PKG_VERSION")))
        .await?;
    rpc.notify("initialized", None)
}

async fn sandbox_readiness(rpc: &RpcClient) -> Sandbox {
    if !cfg!(windows) {
        return Ok(());
    }
    readiness(rpc.call("windowsSandbox/readiness", Value::Null).await)
}

async fn resume(rpc: &RpcClient, id: &str, params: Value) -> std::result::Result<Value, String> {
    match rpc.call("thread/resume", params.clone()).await {
        Err(error) if is_archived(&error) => {
            rpc.call("thread/unarchive", json!({"threadId": id}))
                .await?;
            rpc.call("thread/resume", params).await
        }
        resumed => resumed,
    }
}

pub(super) async fn open_thread(
    rpc: &RpcClient,
    open: Open<'_>,
    setup: &ThreadSetup<'_>,
) -> std::result::Result<Started, String> {
    handshake(rpc).await?;
    let sandbox = sandbox_readiness(rpc).await;
    let mode = effective_mode(setup.mode, &sandbox);
    let (thread, result) = match open {
        Open::Start => {
            let params = thread_params(setup.cwd, setup.model, mode, setup.kind);
            let result = rpc
                .call(
                    "thread/start",
                    with_untrusted_projects(params, setup.untrusted),
                )
                .await?;
            let thread = thread_id(&result).ok_or_else(|| MISSING_THREAD.to_owned())?;
            (thread, result)
        }
        Open::Resume(id) => {
            let params = resume_params(id, setup.cwd, setup.model, mode, setup.kind);
            let result = resume(rpc, id, with_untrusted_projects(params, setup.untrusted)).await?;
            (id.to_owned(), result)
        }
    };
    Ok(Started {
        thread,
        model: thread_model(&result),
        mode,
        sandbox,
    })
}

pub(super) async fn fetch_models(spec: &AgentSpec) -> std::result::Result<Vec<ModelInfo>, String> {
    let (feed, _fed) = mpsc::unbounded_channel();
    let tail = Arc::new(StderrTail::default());
    let codex = Codex::spawn(spec, &std::env::temp_dir(), tail, 0, &feed)
        .map_err(|error| error.to_string())?;
    let listed = match handshake(&codex.rpc).await {
        Ok(()) => list_model_pages(&codex.rpc).await,
        Err(error) => Err(error),
    };
    codex.stop().await;
    listed.map(|result| parse_models(&result))
}

pub(super) async fn list_model_pages(rpc: &RpcClient) -> std::result::Result<Value, String> {
    let mut pages = ModelPages::default();
    let mut params = json!({});
    loop {
        let page = rpc.call("model/list", params).await?;
        match pages.add(&page) {
            Some(cursor) => params = json!({"cursor": cursor}),
            None => return Ok(pages.result()),
        }
    }
}

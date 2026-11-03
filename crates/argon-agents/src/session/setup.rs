use std::sync::Arc;

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::v1::ConnectionTo;
use agent_client_protocol::v1::schema::{
    CancelNotification, ClientCapabilities, ContentBlock, Implementation, InitializeRequest,
    PromptRequest, PromptResponse, SessionId, TextContent,
};
use agent_client_protocol::{Agent, UntypedMessage};
use futures::future::BoxFuture;
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;

use super::driver::Shared;
use super::events::{AgentEvent, config_option, config_values, current_value};
use super::failure::{STDERR_LINES, classify, turn_failure};
use super::turn::stop_reason_name;
use super::{Command, Result, SessionContext, SessionError, SessionOptions};
use crate::defaults::{ModelSelection, agent_defaults};
use crate::registry::{AgentSpec, ModelInfo};

const MODE_CONFIG_ID: &str = "mode";
type Turn = BoxFuture<'static, agent_client_protocol::Result<PromptResponse>>;

pub(super) struct Ready {
    pub acp_session_id: String,
    pub model: String,
    pub models: Vec<ModelInfo>,
    pub warnings: Vec<String>,
}

async fn call(
    cx: &ConnectionTo<Agent>,
    method: &str,
    params: Value,
) -> std::result::Result<Value, String> {
    let message = UntypedMessage::new(method, params).map_err(|error| error.to_string())?;
    cx.send_request(message)
        .block_task()
        .await
        .map_err(|error| error.to_string())
}

fn model_config_id(spec: &AgentSpec) -> Result<&'static str> {
    match spec.model_selection {
        ModelSelection::AcpConfigOption { config_id } => Ok(config_id),
        ModelSelection::CliFlag { .. } => Err(SessionError::NotAcp),
    }
}

async fn change_model(
    cx: &ConnectionTo<Agent>,
    acp_id: &str,
    session: &Value,
    spec: &AgentSpec,
    model: &str,
) -> Result<()> {
    let config_id = model_config_id(spec)?;
    let failed = |reason: String| SessionError::SetModel {
        model: model.to_owned(),
        reason,
    };
    let option = config_option(session, config_id)
        .ok_or_else(|| failed("o agente não expõe seleção de modelo".to_owned()))?;
    let models = config_values(option);
    if !models.is_empty() && !models.iter().any(|known| known.id == model) {
        return Err(SessionError::ModelUnavailable {
            model: model.to_owned(),
        });
    }
    let params = json!({ "sessionId": acp_id, "configId": config_id, "value": model });
    call(cx, "session/set_config_option", params)
        .await
        .map(|_| ())
        .map_err(failed)
}

struct Opened {
    ready: Ready,
    session: Value,
}

async fn open(
    cx: &ConnectionTo<Agent>,
    spec: &AgentSpec,
    ctx: &SessionContext,
    load: Option<&str>,
) -> Result<Opened> {
    let init = cx
        .send_request(
            InitializeRequest::new(ProtocolVersion::V1)
                .client_capabilities(ClientCapabilities::default())
                .client_info(Implementation::new("argon", env!("CARGO_PKG_VERSION"))),
        )
        .block_task()
        .await
        .map_err(|error| classify(spec, error.to_string()))?;
    let params = json!({ "cwd": ctx.cwd, "mcpServers": [] });
    let (session, acp_session_id) = match load {
        Some(id) => {
            if !init.agent_capabilities.load_session {
                return Err(SessionError::LoadUnsupported);
            }
            let mut params = params;
            params["sessionId"] = json!(id);
            let session = call(cx, "session/load", params)
                .await
                .map_err(|error| classify(spec, error))?;
            (session, id.to_owned())
        }
        None => {
            let session = call(cx, "session/new", params)
                .await
                .map_err(|error| classify(spec, error))?;
            let id = session
                .get("sessionId")
                .and_then(Value::as_str)
                .ok_or_else(|| SessionError::Acp("session/new sem sessionId".to_owned()))?
                .to_owned();
            (session, id)
        }
    };

    let mut warnings = Vec::new();
    if let Some(mode) = agent_defaults(spec.kind).permission_mode
        && let Some(option) = config_option(&session, MODE_CONFIG_ID)
        && current_value(option).as_deref() != Some(mode)
    {
        let params =
            json!({ "sessionId": acp_session_id, "configId": MODE_CONFIG_ID, "value": mode });
        if let Err(error) = call(cx, "session/set_config_option", params).await {
            warnings.push(format!(
                "não consegui ativar o modo `{mode}` do agente: {error}"
            ));
        }
    }

    let config_id = model_config_id(spec)?;
    let option = config_option(&session, config_id);
    let models = option.map(config_values).unwrap_or_default();
    let reported = option.and_then(current_value).unwrap_or_default();
    let model = if ctx.model.is_empty() || ctx.model == reported {
        reported
    } else {
        change_model(cx, &acp_session_id, &session, spec, &ctx.model).await?;
        ctx.model.clone()
    };
    Ok(Opened {
        ready: Ready {
            acp_session_id,
            model,
            models,
            warnings,
        },
        session,
    })
}

async fn next_turn(turn: &mut Option<Turn>) -> agent_client_protocol::Result<PromptResponse> {
    match turn {
        Some(future) => future.await,
        None => std::future::pending().await,
    }
}

async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

fn start_turn(cx: &ConnectionTo<Agent>, acp_id: &str, text: String) -> Turn {
    let cx = cx.clone();
    let request = PromptRequest::new(
        SessionId::new(acp_id.to_owned()),
        vec![ContentBlock::Text(TextContent::new(text))],
    );
    Box::pin(async move { cx.send_request(request).block_task().await })
}

struct ModelChange {
    cx: ConnectionTo<Agent>,
    shared: Arc<Shared>,
    spec: AgentSpec,
    session: Arc<Value>,
    acp_id: String,
    timeout: std::time::Duration,
}

impl ModelChange {
    fn spawn(self, model: String, reply: oneshot::Sender<Result<()>>) {
        tokio::spawn(async move {
            let change = change_model(&self.cx, &self.acp_id, &self.session, &self.spec, &model);
            let result = match tokio::time::timeout(self.timeout, change).await {
                Ok(result) => result,
                Err(_) => Err(SessionError::SetModel {
                    model: model.clone(),
                    reason: format!("sem resposta em {}s", self.timeout.as_secs()),
                }),
            };
            match &result {
                Ok(()) => self.shared.set_model(&model),
                Err(error) => self.shared.emit(AgentEvent::Error(error.to_string())),
            }
            self.shared.release();
            let _ = reply.send(result);
        });
    }
}

pub(super) struct Connection {
    pub shared: Arc<Shared>,
    pub spec: AgentSpec,
    pub ctx: SessionContext,
    pub load: Option<String>,
    pub options: SessionOptions,
    pub commands: mpsc::UnboundedReceiver<Command>,
    pub ready: oneshot::Sender<Result<Ready>>,
}

pub(super) async fn run_connection(
    cx: ConnectionTo<Agent>,
    connection: Connection,
) -> agent_client_protocol::Result<bool> {
    let Connection {
        shared,
        spec,
        ctx,
        load,
        options,
        mut commands,
        ready,
    } = connection;
    let Opened {
        ready: opened,
        session,
    } = match open(&cx, &spec, &ctx, load.as_deref()).await {
        Ok(opened) => opened,
        Err(error) => {
            let _ = ready.send(Err(error));
            return Ok(false);
        }
    };
    let acp_id = opened.acp_session_id.clone();
    let session = Arc::new(session);
    if ready.send(Ok(opened)).is_err() {
        return Ok(false);
    }
    let mut awaiting = shared.awaiting();
    let mut turn: Option<Turn> = None;
    let mut remaining = options.turn_timeout;
    let mut running_since: Option<Instant> = None;
    loop {
        let deadline = running_since.map(|since| since + remaining);
        tokio::select! {
            command = commands.recv() => match command {
                None | Some(Command::Shutdown) => return Ok(true),
                Some(Command::Prompt(text)) => {
                    if turn.is_some() {
                        shared.emit(AgentEvent::Error("já existe um turno em andamento".to_owned()));
                        continue;
                    }
                    shared.begin_turn(&text);
                    turn = Some(start_turn(&cx, &acp_id, text));
                    remaining = options.turn_timeout;
                    running_since = (!*awaiting.borrow()).then(Instant::now);
                }
                Some(Command::Cancel) => {
                    if let Err(error) = cx.send_notification(CancelNotification::new(SessionId::new(acp_id.clone()))) {
                        tracing::warn!(%error, "failed to send session/cancel");
                    }
                    shared.cancel_pending();
                }
                Some(Command::SetModel { model, reply }) => ModelChange {
                    cx: cx.clone(),
                    shared: Arc::clone(&shared),
                    spec: spec.clone(),
                    session: Arc::clone(&session),
                    acp_id: acp_id.clone(),
                    timeout: options.set_model_timeout,
                }
                .spawn(model, reply),
            },
            changed = awaiting.changed(), if turn.is_some() => {
                if changed.is_ok() {
                    if *awaiting.borrow_and_update() {
                        if let Some(since) = running_since.take() {
                            remaining = remaining.saturating_sub(since.elapsed());
                        }
                    } else if running_since.is_none() {
                        running_since = Some(Instant::now());
                    }
                }
            }
            result = next_turn(&mut turn) => {
                turn = None;
                running_since = None;
                match result {
                    Ok(response) => shared.end_turn(&stop_reason_name(&response.stop_reason)).await,
                    Err(error) => shared.fail(turn_failure(&spec, &shared.stderr_tail(STDERR_LINES), &error.to_string())),
                }
            }
            () = sleep_until(deadline) => {
                shared.fail(format!("o turno passou de {} minutos e foi encerrado", options.turn_timeout.as_secs() / 60));
                return Ok(true);
            }
        }
    }
}

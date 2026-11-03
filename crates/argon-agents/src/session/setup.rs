use std::sync::Arc;
use std::time::Duration;

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
use super::turn::stop_reason_name;
use super::{Command, Result, SessionContext, SessionError};
use crate::defaults::{ModelSelection, agent_defaults};
use crate::registry::{AgentSpec, ModelInfo};

const TURN_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const MODE_CONFIG_ID: &str = "mode";

type Turn = BoxFuture<'static, agent_client_protocol::Result<PromptResponse>>;

pub(super) struct Ready {
    pub acp_session_id: String,
    pub model: String,
    pub models: Vec<ModelInfo>,
    pub warnings: Vec<String>,
}

fn acp(error: agent_client_protocol::Error) -> SessionError {
    SessionError::Acp(error.to_string())
}

async fn call(cx: &ConnectionTo<Agent>, method: &str, params: Value) -> Result<Value> {
    cx.send_request(UntypedMessage::new(method, params).map_err(acp)?)
        .block_task()
        .await
        .map_err(acp)
}

async fn set_option(
    cx: &ConnectionTo<Agent>,
    acp_id: &str,
    config_id: &str,
    value: &str,
) -> Result<Value> {
    call(
        cx,
        "session/set_config_option",
        json!({ "sessionId": acp_id, "configId": config_id, "value": value }),
    )
    .await
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
    let Some(option) = config_option(session, config_id) else {
        return Err(SessionError::SetModel {
            model: model.to_owned(),
            reason: "o agente não expõe seleção de modelo".to_owned(),
        });
    };
    let models = config_values(option);
    if !models.is_empty() && !models.iter().any(|known| known.id == model) {
        return Err(SessionError::ModelUnavailable {
            model: model.to_owned(),
        });
    }
    set_option(cx, acp_id, config_id, model)
        .await
        .map(|_| ())
        .map_err(|error| SessionError::SetModel {
            model: model.to_owned(),
            reason: error.to_string(),
        })
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
        .map_err(acp)?;
    let params = json!({ "cwd": ctx.cwd, "mcpServers": [] });
    let (session, acp_session_id) = match load {
        Some(id) => {
            if !init.agent_capabilities.load_session {
                return Err(SessionError::LoadUnsupported);
            }
            let mut params = params;
            params["sessionId"] = json!(id);
            (call(cx, "session/load", params).await?, id.to_owned())
        }
        None => {
            let session = call(cx, "session/new", params).await?;
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
        && let Err(error) = set_option(cx, &acp_session_id, MODE_CONFIG_ID, mode).await
    {
        warnings.push(format!(
            "não consegui ativar o modo `{mode}` do agente: {error}"
        ));
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

pub(super) async fn run_connection(
    cx: ConnectionTo<Agent>,
    shared: Arc<Shared>,
    spec: AgentSpec,
    ctx: SessionContext,
    load: Option<String>,
    mut commands: mpsc::UnboundedReceiver<Command>,
    ready: oneshot::Sender<Result<Ready>>,
) -> agent_client_protocol::Result<bool> {
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
    if ready.send(Ok(opened)).is_err() {
        return Ok(false);
    }
    let mut turn: Option<Turn> = None;
    let mut deadline: Option<Instant> = None;
    loop {
        tokio::select! {
            command = commands.recv() => match command {
                None | Some(Command::Shutdown) => return Ok(true),
                Some(Command::Prompt(text)) => {
                    if turn.is_some() {
                        shared.emit(AgentEvent::Error("já existe um turno em andamento".to_owned()));
                    } else {
                        shared.begin_turn(&text);
                        turn = Some(start_turn(&cx, &acp_id, text));
                        deadline = Some(Instant::now() + TURN_TIMEOUT);
                    }
                }
                Some(Command::Cancel) => {
                    if let Err(error) = cx.send_notification(CancelNotification::new(SessionId::new(acp_id.clone()))) {
                        tracing::warn!(%error, "failed to send session/cancel");
                    }
                    shared.cancel_pending();
                }
                Some(Command::RespondPermission { request_id, option_id, reply }) => {
                    let _ = reply.send(shared.respond(&request_id, &option_id));
                }
                Some(Command::SetModel { model, reply }) => {
                    let result = change_model(&cx, &acp_id, &session, &spec, &model).await;
                    match &result {
                        Ok(()) => shared.store().set_model(shared.id(), &model),
                        Err(error) => shared.emit(AgentEvent::Error(error.to_string())),
                    }
                    let _ = reply.send(result);
                }
            },
            result = next_turn(&mut turn) => {
                turn = None;
                deadline = None;
                match result {
                    Ok(response) => shared.end_turn(&stop_reason_name(&response.stop_reason)),
                    Err(error) => shared.fail(format!("o agente falhou no turno: {error}")),
                }
            }
            () = sleep_until(deadline) => {
                shared.fail(format!("o turno passou de {} minutos e foi encerrado", TURN_TIMEOUT.as_secs() / 60));
                return Ok(true);
            }
        }
    }
}

use agent_client_protocol::v1::Responder;
use agent_client_protocol::v1::schema::{
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome,
};
use serde_json::Value;

use super::driver::Shared;
use super::permissions::{PermissionAnswer, PermissionReply, acp_ask};

fn response(answer: PermissionAnswer) -> RequestPermissionResponse {
    let outcome = match answer {
        PermissionAnswer::Selected(option_id) => {
            RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(option_id))
        }
        PermissionAnswer::Cancelled => RequestPermissionOutcome::Cancelled,
    };
    RequestPermissionResponse::new(outcome)
}

fn reply(responder: Responder<RequestPermissionResponse>) -> PermissionReply {
    Box::new(move |answer| {
        responder
            .respond(response(answer))
            .map_err(|error| error.to_string())
    })
}

pub(super) fn handle_request(
    shared: &Shared,
    request: &RequestPermissionRequest,
    responder: Responder<RequestPermissionResponse>,
) {
    let value = serde_json::to_value(request).unwrap_or(Value::Null);
    let tool_call_id = value
        .pointer("/toolCall/toolCallId")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let known_kind = shared.known_kind(tool_call_id);
    let ask = acp_ask(&value, known_kind.as_deref());
    shared.on_permission(ask, reply(responder));
}

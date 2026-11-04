use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{Notify, mpsc, oneshot};

use crate::util::{lock, read_line_lossy};

const CALL_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_LINE_BYTES: usize = 8 * 1024 * 1024;
const CLOSED: &str = "conexão encerrada";

pub(crate) enum Incoming {
    Notification {
        method: String,
        params: Value,
    },
    Request {
        id: Value,
        method: String,
        params: Value,
    },
}

enum Failure {
    Rpc(String),
    Closed,
}

type Reply = Result<Value, Failure>;
type PendingCalls = HashMap<u64, oneshot::Sender<Reply>>;

struct Shared {
    pending: Mutex<Option<PendingCalls>>,
    stop_reader: Notify,
}

impl Shared {
    fn close(&self) {
        let drained = lock(&self.pending).take();
        for (_, reply) in drained.into_iter().flatten() {
            let _ = reply.send(Err(Failure::Closed));
        }
        self.stop_reader.notify_one();
    }

    fn take_pending(&self, id: u64) -> Option<oneshot::Sender<Reply>> {
        lock(&self.pending).as_mut()?.remove(&id)
    }
}

struct PendingGuard<'a> {
    shared: &'a Shared,
    id: u64,
}

impl Drop for PendingGuard<'_> {
    fn drop(&mut self) {
        self.shared.take_pending(self.id);
    }
}

struct Inner {
    outgoing: mpsc::UnboundedSender<String>,
    shared: Arc<Shared>,
    next_id: AtomicU64,
    timeout: Duration,
}

#[derive(Clone)]
pub(crate) struct RpcClient {
    inner: Arc<Inner>,
}

impl RpcClient {
    pub fn new<W, R>(stdin: W, stdout: R) -> (RpcClient, mpsc::UnboundedReceiver<Incoming>)
    where
        W: AsyncWrite + Unpin + Send + 'static,
        R: AsyncRead + Unpin + Send + 'static,
    {
        Self::with_timeout(stdin, stdout, CALL_TIMEOUT)
    }

    pub fn with_timeout<W, R>(
        stdin: W,
        stdout: R,
        timeout: Duration,
    ) -> (RpcClient, mpsc::UnboundedReceiver<Incoming>)
    where
        W: AsyncWrite + Unpin + Send + 'static,
        R: AsyncRead + Unpin + Send + 'static,
    {
        let shared = Arc::new(Shared {
            pending: Mutex::new(Some(HashMap::new())),
            stop_reader: Notify::new(),
        });
        let (outgoing, outgoing_rx) = mpsc::unbounded_channel();
        let (incoming_tx, incoming_rx) = mpsc::unbounded_channel();
        tokio::spawn(write_loop(stdin, outgoing_rx, shared.clone()));
        tokio::spawn(read_loop(stdout, incoming_tx, shared.clone()));
        let client = RpcClient {
            inner: Arc::new(Inner {
                outgoing,
                shared,
                next_id: AtomicU64::new(1),
                timeout,
            }),
        };
        (client, incoming_rx)
    }

    pub async fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        let id = self.enqueue_request(method, params, reply_tx)?;
        let _guard = PendingGuard {
            shared: &self.inner.shared,
            id,
        };
        match tokio::time::timeout(self.inner.timeout, reply_rx).await {
            Ok(Ok(Ok(result))) => Ok(result),
            Ok(Ok(Err(Failure::Rpc(detail)))) => Err(format!("{method}: {detail}")),
            Ok(Ok(Err(Failure::Closed)) | Err(_)) => Err(CLOSED.to_owned()),
            Err(_) => Err(format!(
                "{method}: sem resposta em {}s",
                self.inner.timeout.as_secs()
            )),
        }
    }

    pub fn notify(&self, method: &str, params: Option<Value>) -> Result<(), String> {
        let mut message = json!({ "method": method });
        if let Some(params) = params {
            message["params"] = params;
        }
        self.enqueue(&message)
    }

    pub fn respond(&self, id: Value, result: Value) -> Result<(), String> {
        self.enqueue(&json!({ "id": id, "result": result }))
    }

    pub fn respond_error(&self, id: Value, code: i64, message: &str) -> Result<(), String> {
        self.enqueue(&json!({ "id": id, "error": { "code": code, "message": message } }))
    }

    fn enqueue(&self, message: &Value) -> Result<(), String> {
        let pending = lock(&self.inner.shared.pending);
        if pending.is_none() {
            return Err(CLOSED.to_owned());
        }
        self.inner
            .outgoing
            .send(message.to_string())
            .map_err(|_| CLOSED.to_owned())
    }

    fn enqueue_request(
        &self,
        method: &str,
        params: Value,
        reply: oneshot::Sender<Reply>,
    ) -> Result<u64, String> {
        let mut pending = lock(&self.inner.shared.pending);
        let calls = pending.as_mut().ok_or_else(|| CLOSED.to_owned())?;
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let message = json!({ "method": method, "id": id, "params": params });
        self.inner
            .outgoing
            .send(message.to_string())
            .map_err(|_| CLOSED.to_owned())?;
        calls.insert(id, reply);
        Ok(id)
    }
}

async fn write_loop<W: AsyncWrite + Unpin>(
    mut stdin: W,
    mut outgoing: mpsc::UnboundedReceiver<String>,
    shared: Arc<Shared>,
) {
    while let Some(mut line) = outgoing.recv().await {
        line.push('\n');
        let written = async {
            stdin.write_all(line.as_bytes()).await?;
            stdin.flush().await
        }
        .await;
        if let Err(error) = written {
            tracing::debug!(%error, "JSON-RPC writer failed");
            shared.close();
            return;
        }
    }
}

async fn read_loop<R: AsyncRead + Unpin>(
    stdout: R,
    incoming: mpsc::UnboundedSender<Incoming>,
    shared: Arc<Shared>,
) {
    let mut reader = BufReader::new(stdout);
    loop {
        let line = tokio::select! {
            line = read_line_lossy(&mut reader, MAX_LINE_BYTES) => line,
            () = shared.stop_reader.notified() => break,
        };
        match line {
            Ok(Some(line)) => dispatch(&line, &shared, &incoming),
            Ok(None) => break,
            Err(error) => {
                tracing::debug!(%error, "JSON-RPC reader failed");
                break;
            }
        }
    }
    shared.close();
}

fn dispatch(line: &str, shared: &Shared, incoming: &mpsc::UnboundedSender<Incoming>) {
    let Ok(Value::Object(mut object)) = serde_json::from_str::<Value>(line.trim()) else {
        return;
    };
    let id = object.remove("id");
    let method = object.remove("method");
    let params = object.remove("params").unwrap_or(Value::Null);
    match (method, id) {
        (Some(Value::String(method)), Some(id)) => {
            let _ = incoming.send(Incoming::Request { id, method, params });
        }
        (Some(Value::String(method)), None) => {
            let _ = incoming.send(Incoming::Notification { method, params });
        }
        (None, Some(id)) => {
            let Some(reply) = id.as_u64().and_then(|id| shared.take_pending(id)) else {
                return;
            };
            let outcome = match object.remove("error") {
                Some(error) if !error.is_null() => Err(Failure::Rpc(describe_error(&error))),
                _ => Ok(object.remove("result").unwrap_or(Value::Null)),
            };
            let _ = reply.send(outcome);
        }
        _ => {}
    }
}

fn describe_error(error: &Value) -> String {
    let code = error.get("code").map(Value::to_string).unwrap_or_default();
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default();
    format!("{code} {message}")
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;

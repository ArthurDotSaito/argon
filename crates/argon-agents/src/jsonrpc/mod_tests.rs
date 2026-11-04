use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, Lines, duplex};

use super::*;

const SHORT_TIMEOUT: Duration = Duration::from_millis(50);

struct FakeServer {
    requests: Lines<BufReader<DuplexStream>>,
    responses: DuplexStream,
}

impl FakeServer {
    async fn read(&mut self) -> Value {
        let line = self.requests.next_line().await.unwrap().unwrap();
        serde_json::from_str(&line).unwrap()
    }

    async fn send(&mut self, message: Value) {
        self.send_raw(message.to_string().as_bytes()).await;
    }

    async fn send_raw(&mut self, bytes: &[u8]) {
        self.responses.write_all(bytes).await.unwrap();
        self.responses.write_all(b"\n").await.unwrap();
    }
}

fn connect_with(timeout: Duration) -> (RpcClient, mpsc::UnboundedReceiver<Incoming>, FakeServer) {
    let (client_stdin, server_in) = duplex(64 * 1024);
    let (server_out, client_stdout) = duplex(64 * 1024);
    let (client, incoming) = RpcClient::with_timeout(client_stdin, client_stdout, timeout);
    let server = FakeServer {
        requests: BufReader::new(server_in).lines(),
        responses: server_out,
    };
    (client, incoming, server)
}

fn connect() -> (RpcClient, mpsc::UnboundedReceiver<Incoming>, FakeServer) {
    connect_with(CALL_TIMEOUT)
}

#[tokio::test]
async fn requests_notifications_and_responses_are_written_without_a_jsonrpc_field() {
    let (client, _incoming, mut server) = connect();

    let first = tokio::spawn({
        let client = client.clone();
        async move { client.call("initialize", json!({ "a": 1 })).await }
    });
    assert_eq!(
        server.read().await,
        json!({ "method": "initialize", "id": 1, "params": { "a": 1 } })
    );
    server.send(json!({ "id": 1, "result": {} })).await;
    first.await.unwrap().unwrap();

    client.notify("initialized", None).unwrap();
    assert_eq!(server.read().await, json!({ "method": "initialized" }));

    client
        .notify("progress", Some(json!({ "pct": 5 })))
        .unwrap();
    assert_eq!(
        server.read().await,
        json!({ "method": "progress", "params": { "pct": 5 } })
    );

    client
        .respond(json!("req-9"), json!({ "ok": true }))
        .unwrap();
    assert_eq!(
        server.read().await,
        json!({ "id": "req-9", "result": { "ok": true } })
    );

    let second = tokio::spawn({
        let client = client.clone();
        async move { client.call("thread/start", json!({})).await }
    });
    assert_eq!(
        server.read().await,
        json!({ "method": "thread/start", "id": 2, "params": {} })
    );
    server.send(json!({ "id": 2, "result": { "t": 1 } })).await;
    assert_eq!(second.await.unwrap().unwrap(), json!({ "t": 1 }));
}

#[tokio::test]
async fn out_of_order_responses_resolve_the_matching_calls() {
    let (client, _incoming, mut server) = connect();
    let first = tokio::spawn({
        let client = client.clone();
        async move { client.call("one", json!({})).await }
    });
    assert_eq!(server.read().await["id"], 1);
    let second = tokio::spawn({
        let client = client.clone();
        async move { client.call("two", json!({})).await }
    });
    assert_eq!(server.read().await["id"], 2);

    server.send(json!({ "id": 2, "result": "second" })).await;
    server
        .send(json!({ "id": 99, "result": "unknown id" }))
        .await;
    server.send(json!({ "id": 1, "result": "first" })).await;

    assert_eq!(second.await.unwrap().unwrap(), json!("second"));
    assert_eq!(first.await.unwrap().unwrap(), json!("first"));
}

#[tokio::test]
async fn error_responses_carry_the_method_code_and_message() {
    let (client, _incoming, mut server) = connect();
    let call = tokio::spawn(async move { client.call("turn/start", json!({})).await });
    server.read().await;
    server
        .send(json!({ "id": 1, "error": { "code": -32600, "message": "Invalid request" } }))
        .await;
    assert_eq!(
        call.await.unwrap().unwrap_err(),
        "turn/start: -32600 Invalid request"
    );
}

#[tokio::test]
async fn a_null_error_next_to_a_result_is_a_success() {
    let (client, _incoming, mut server) = connect();
    let call = tokio::spawn(async move { client.call("model/list", json!({})).await });
    server.read().await;
    server
        .send(json!({ "id": 1, "result": { "data": [] }, "error": null }))
        .await;
    assert_eq!(call.await.unwrap().unwrap(), json!({ "data": [] }));
}

#[tokio::test]
async fn error_responses_to_server_requests_carry_code_and_message() {
    let (client, _incoming, mut server) = connect();
    client
        .respond_error(json!(4), -32601, "unsupported: currentTime/read")
        .unwrap();
    assert_eq!(
        server.read().await,
        json!({ "id": 4, "error": { "code": -32601, "message": "unsupported: currentTime/read" } })
    );
}

#[tokio::test]
async fn server_requests_and_notifications_arrive_as_incoming_with_null_params_when_absent() {
    let (_client, mut incoming, mut server) = connect();
    server
        .send(json!({ "method": "item/approve", "id": "abc", "params": { "x": 1 } }))
        .await;
    server.send(json!({ "method": "ping", "id": 7 })).await;
    server
        .send(json!({ "method": "turn/completed", "params": { "ok": true } }))
        .await;
    server.send(json!({ "method": "bare" })).await;

    match incoming.recv().await.unwrap() {
        Incoming::Request { id, method, params } => {
            assert_eq!(id, json!("abc"));
            assert_eq!(method, "item/approve");
            assert_eq!(params, json!({ "x": 1 }));
        }
        Incoming::Notification { .. } => panic!("expected a request"),
    }
    match incoming.recv().await.unwrap() {
        Incoming::Request { id, params, .. } => {
            assert_eq!(id, json!(7));
            assert_eq!(params, Value::Null);
        }
        Incoming::Notification { .. } => panic!("expected a request"),
    }
    match incoming.recv().await.unwrap() {
        Incoming::Notification { method, params } => {
            assert_eq!(method, "turn/completed");
            assert_eq!(params, json!({ "ok": true }));
        }
        Incoming::Request { .. } => panic!("expected a notification"),
    }
    match incoming.recv().await.unwrap() {
        Incoming::Notification { method, params } => {
            assert_eq!(method, "bare");
            assert_eq!(params, Value::Null);
        }
        Incoming::Request { .. } => panic!("expected a notification"),
    }
}

#[tokio::test]
async fn non_json_and_invalid_utf8_lines_are_ignored() {
    let (client, mut incoming, mut server) = connect();
    let call = tokio::spawn(async move { client.call("m", json!({})).await });
    server.read().await;

    server.send_raw(b"not json at all").await;
    server.send_raw(b"").await;
    server.send_raw(b"[1,2,3]").await;
    server.send_raw(b"\xff\xfe broken \xc3\x28").await;
    server.send(json!({ "method": "still-alive" })).await;
    server.send(json!({ "id": 1, "result": "done" })).await;

    assert_eq!(call.await.unwrap().unwrap(), json!("done"));
    assert!(matches!(
        incoming.recv().await,
        Some(Incoming::Notification { method, .. }) if method == "still-alive"
    ));
}

#[tokio::test]
async fn an_overlong_line_does_not_break_the_next_one() {
    let (client, _incoming, mut server) = connect();
    let call = tokio::spawn(async move { client.call("m", json!({})).await });
    server.read().await;

    let junk = vec![b'a'; MAX_LINE_BYTES + 1024 * 1024];
    server.send_raw(&junk).await;
    server.send(json!({ "id": 1, "result": "after" })).await;

    assert_eq!(call.await.unwrap().unwrap(), json!("after"));
}

#[tokio::test]
async fn eof_fails_pending_calls_ends_the_receiver_and_later_calls_fail_fast() {
    let (client, mut incoming, mut server) = connect();
    let pending = tokio::spawn({
        let client = client.clone();
        async move { client.call("slow", json!({})).await }
    });
    server.read().await;
    assert!(!client.is_closed());
    drop(server);

    assert_eq!(pending.await.unwrap().unwrap_err(), CLOSED);
    assert!(client.is_closed());
    assert!(incoming.recv().await.is_none());
    assert_eq!(client.call("later", json!({})).await.unwrap_err(), CLOSED);
    assert_eq!(client.notify("later", None).unwrap_err(), CLOSED);
    assert_eq!(client.respond(json!(1), json!({})).unwrap_err(), CLOSED);
    assert_eq!(
        client.respond_error(json!(1), -32601, "x").unwrap_err(),
        CLOSED
    );
}

struct BrokenWriter;

impl AsyncWrite for BrokenWriter {
    fn poll_write(self: Pin<&mut Self>, _: &mut Context<'_>, _: &[u8]) -> Poll<io::Result<usize>> {
        Poll::Ready(Err(io::Error::from(io::ErrorKind::BrokenPipe)))
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[tokio::test]
async fn a_failing_writer_closes_the_connection_even_while_stdout_stays_open() {
    let (_server_out, client_stdout) = duplex(1024);
    let (client, mut incoming) = RpcClient::new(BrokenWriter, client_stdout);

    assert_eq!(client.call("m", json!({})).await.unwrap_err(), CLOSED);
    assert!(incoming.recv().await.is_none());
    assert_eq!(client.call("later", json!({})).await.unwrap_err(), CLOSED);
}

#[tokio::test]
async fn a_call_without_a_response_times_out() {
    let (client, _incoming, mut server) = connect_with(SHORT_TIMEOUT);
    let error = client.call("silent", json!({})).await.unwrap_err();
    assert!(error.starts_with("silent: sem resposta em "), "{error}");
    server.read().await;

    server.send(json!({ "id": 1, "result": "too late" })).await;
    let call = tokio::spawn({
        let client = client.clone();
        async move { client.call("next", json!({})).await }
    });
    assert_eq!(server.read().await["id"], 2);
    server.send(json!({ "id": 2, "result": "ok" })).await;
    assert_eq!(call.await.unwrap().unwrap(), json!("ok"));
}

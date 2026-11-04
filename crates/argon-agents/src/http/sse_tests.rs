use super::*;
use serde_json::json;

fn feed(chunks: &[&[u8]]) -> Vec<Value> {
    let mut parser = SseParser::new();
    chunks.iter().flat_map(|chunk| parser.push(chunk)).collect()
}

#[test]
fn event_split_across_chunks() {
    let events = feed(&[b"data: {\"type\":\"mess", b"age\",\"n\":1}\n", b"\n"]);
    assert_eq!(events, vec![json!({"type": "message", "n": 1})]);
}

#[test]
fn partial_line_stays_buffered_until_newline() {
    let mut parser = SseParser::new();
    assert!(parser.push(b"data: {\"a\":1}").is_empty());
    assert_eq!(parser.push(b"\n"), vec![json!({"a": 1})]);
}

#[test]
fn data_prefix_without_space() {
    assert_eq!(feed(&[b"data:{\"a\":1}\n"]), vec![json!({"a": 1})]);
}

#[test]
fn crlf_line_endings() {
    let events = feed(&[b"data: {\"a\":1}\r\n\r\ndata: {\"b\":2}\r\n\r\n"]);
    assert_eq!(events, vec![json!({"a": 1}), json!({"b": 2})]);
}

#[test]
fn empty_comment_and_field_lines_are_ignored() {
    let events =
        feed(&[b"\n: keep-alive\nevent: message\nid: 7\nretry: 1000\ndata: {\"a\":1}\n\n"]);
    assert_eq!(events, vec![json!({"a": 1})]);
}

#[test]
fn done_marker_is_ignored() {
    assert!(feed(&[b"data: [DONE]\n\n"]).is_empty());
}

#[test]
fn invalid_json_is_skipped() {
    let events = feed(&[b"data: {not json\n\ndata: {\"a\":1}\n\n"]);
    assert_eq!(events, vec![json!({"a": 1})]);
}

#[test]
fn multibyte_character_split_across_chunks() {
    let expected = vec![json!({"text": "caf\u{e9} \u{1f980}"})];
    let line = "data: {\"text\":\"caf\u{e9} \u{1f980}\"}\n".as_bytes();

    let accent = line.iter().position(|byte| *byte == 0xC3).unwrap() + 1;
    assert_eq!(feed(&[&line[..accent], &line[accent..]]), expected);

    let crab = line.iter().position(|byte| *byte == 0xF0).unwrap() + 2;
    assert_eq!(feed(&[&line[..crab], &line[crab..]]), expected);
}

#[test]
fn several_events_in_one_chunk() {
    let events = feed(&[b"data: {\"a\":1}\n\ndata: {\"b\":2}\n\ndata: {\"c\":3}\n\n"]);
    assert_eq!(
        events,
        vec![json!({"a": 1}), json!({"b": 2}), json!({"c": 3})]
    );
}

#[test]
fn basic_header_encodes_opencode_username_and_password() {
    assert_eq!(
        crate::http::basic_auth("p@ss"),
        "Basic b3BlbmNvZGU6cEBzcw=="
    );
}

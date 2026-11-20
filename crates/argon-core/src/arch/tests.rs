use super::*;

const NONCE: &str = "0123456789abcdef";
const OTHER_NONCE: &str = "ffffffffffffffff";

fn arch_block(nonce: &str, json: &str) -> String {
    format!("Texto.\n\n```argon-arch-{nonce}\n{json}\n```\n")
}

fn touched_block(nonce: &str, body: &str) -> String {
    format!("```argon-touched-{nonce}\n{body}\n```\n")
}

fn node_json(id: &str, label: &str) -> String {
    format!(r#"{{"id":"{id}","label":"{label}","lane":"project","kind":"component"}}"#)
}

fn graph_json(nodes: &[String], edges: &[String]) -> String {
    format!(
        r#"{{"nodes":[{}],"edges":[{}]}}"#,
        nodes.join(","),
        edges.join(",")
    )
}

fn parse(json: &str) -> Option<ArchGraph> {
    parse_arch(&arch_block(NONCE, json), NONCE)
}

fn many_nodes(count: usize) -> Vec<String> {
    (0..count)
        .map(|i| node_json(&format!("n{i}"), "No"))
        .collect()
}

#[test]
fn reads_a_valid_graph_with_every_field() {
    let json = r#"{
      "nodes": [
        { "id": "consultas", "label": "Features/Consultas", "lane": "project", "kind": "component" },
        { "id": "fila-eventos", "label": "consulta-eventos", "lane": "uses", "kind": "queue" },
        { "id": "notificacoes", "label": "notificacoes-worker", "lane": "uses", "kind": "service", "repo": "acme/notificacoes-worker" },
        { "id": "faturamento", "label": "faturamento-api", "lane": "callers", "kind": "external", "read": false }
      ],
      "edges": [
        { "from": "consultas", "to": "fila-eventos", "label": "publica", "via": "queue" },
        { "from": "fila-eventos", "to": "notificacoes", "label": "consome", "via": "package" }
      ]
    }"#;

    let graph = parse(json).unwrap();

    assert_eq!(graph.nodes.len(), 4);
    assert_eq!(graph.nodes[0].lane, Lane::Project);
    assert_eq!(graph.nodes[0].kind, NodeKind::Component);
    assert_eq!(graph.nodes[0].repo, None);
    assert!(graph.nodes[0].read);
    assert_eq!(
        graph.nodes[2].repo.as_deref(),
        Some("acme/notificacoes-worker")
    );
    assert_eq!(graph.nodes[3].lane, Lane::Callers);
    assert_eq!(graph.nodes[3].kind, NodeKind::External);
    assert!(!graph.nodes[3].read);
    assert_eq!(graph.edges.len(), 2);
    assert_eq!(graph.edges[0].via, Via::Queue);
    assert_eq!(graph.edges[1].via, Via::Package);
    assert_eq!(graph.edges[0].label, "publica");
}

#[test]
fn database_cache_and_call_variants_are_read() {
    let json = r#"{"nodes":[
        {"id":"d","label":"D","lane":"uses","kind":"database"},
        {"id":"c","label":"C","lane":"uses","kind":"cache"}],
      "edges":[{"from":"d","to":"c","via":"call"}]}"#;

    let graph = parse(json).unwrap();

    assert_eq!(graph.nodes[0].kind, NodeKind::Database);
    assert_eq!(graph.nodes[1].kind, NodeKind::Cache);
    assert_eq!(graph.edges[0].via, Via::Call);
    assert_eq!(graph.edges[0].label, "");
}

#[test]
fn missing_edges_key_is_an_empty_list() {
    let json = r#"{"nodes":[{"id":"a","label":"A","lane":"project","kind":"component"}]}"#;

    assert!(parse(json).unwrap().edges.is_empty());
}

#[test]
fn graph_round_trips_through_json() {
    let graph = parse(&graph_json(
        &[node_json("a", "A"), node_json("b", "B")],
        &[r#"{"from":"a","to":"b","label":"x","via":"call"}"#.to_owned()],
    ))
    .unwrap();

    let json = serde_json::to_string(&graph).unwrap();
    let back: ArchGraph = serde_json::from_str(&json).unwrap();

    assert_eq!(back, graph);
    assert!(json.contains(r#""lane":"project""#));
    assert!(json.contains(r#""via":"call""#));
}

#[test]
fn wrong_nonce_yields_nothing() {
    let message = arch_block(OTHER_NONCE, &graph_json(&[node_json("a", "A")], &[]));

    assert_eq!(parse_arch(&message, NONCE), None);
}

#[test]
fn block_quoted_inside_another_block_is_ignored() {
    let json = graph_json(&[node_json("a", "A")], &[]);
    let message = format!("````md\n```argon-arch-{NONCE}\n{json}\n```\n````\n");

    assert_eq!(parse_arch(&message, NONCE), None);
}

#[test]
fn four_backtick_fence_is_read() {
    let json = graph_json(&[node_json("a", "A")], &[]);
    let message = format!("````argon-arch-{NONCE}\n{json}\n````\n");

    assert_eq!(parse_arch(&message, NONCE).unwrap().nodes.len(), 1);
}

#[test]
fn invalid_json_yields_nothing() {
    assert_eq!(parse("{ nodes: ["), None);
    assert_eq!(parse("[]"), None);
}

#[test]
fn unknown_lane_kind_or_via_yields_nothing() {
    assert_eq!(
        parse(r#"{"nodes":[{"id":"a","label":"A","lane":"side","kind":"component"}]}"#),
        None
    );
    assert_eq!(
        parse(r#"{"nodes":[{"id":"a","label":"A","lane":"uses","kind":"cloud"}]}"#),
        None
    );
    let edge = r#"{"from":"a","to":"a","via":"smoke"}"#.to_owned();
    assert_eq!(parse(&graph_json(&[node_json("a", "A")], &[edge])), None);
}

#[test]
fn graph_without_nodes_is_rejected() {
    assert_eq!(parse(r#"{"nodes":[],"edges":[]}"#), None);
}

#[test]
fn forty_nodes_pass_and_forty_one_are_rejected() {
    assert_eq!(
        parse(&graph_json(&many_nodes(MAX_ARCH_NODES), &[]))
            .unwrap()
            .nodes
            .len(),
        MAX_ARCH_NODES
    );
    assert_eq!(
        parse(&graph_json(&many_nodes(MAX_ARCH_NODES + 1), &[])),
        None
    );
}

#[test]
fn eighty_edges_pass_and_eighty_one_are_rejected() {
    let edge = r#"{"from":"a","to":"b","via":"call"}"#.to_owned();
    let nodes = [node_json("a", "A"), node_json("b", "B")];

    let at_limit = vec![edge.clone(); MAX_ARCH_EDGES];
    let over = vec![edge; MAX_ARCH_EDGES + 1];

    assert_eq!(
        parse(&graph_json(&nodes, &at_limit)).unwrap().edges.len(),
        MAX_ARCH_EDGES
    );
    assert_eq!(parse(&graph_json(&nodes, &over)), None);
}

#[test]
fn duplicated_id_rejects_the_graph() {
    let nodes = [node_json("a", "A"), node_json("a", "Outro")];

    assert_eq!(parse(&graph_json(&nodes, &[])), None);
}

#[test]
fn invalid_ids_reject_the_graph() {
    let too_long = "a".repeat(41);
    for id in [
        "",
        "Maiusculo",
        "com espaco",
        "sub_linha",
        "acentuaç",
        &too_long,
    ] {
        assert_eq!(
            parse(&graph_json(
                &[node_json("ok", "Ok"), node_json(id, "X")],
                &[]
            )),
            None,
            "{id:?}"
        );
    }
}

#[test]
fn id_of_forty_characters_is_valid() {
    let id = "a".repeat(40);

    assert!(parse(&graph_json(&[node_json(&id, "A")], &[])).is_some());
}

#[test]
fn control_and_invisible_characters_in_labels_become_spaces() {
    let label = "a\\u0000b\\nc\\u202ed\\u200be\\ufefff  g";

    let graph = parse(&graph_json(&[node_json("a", label)], &[])).unwrap();

    assert_eq!(graph.nodes[0].label, "a b c d e f g");
}

#[test]
fn long_label_is_cut_with_an_ellipsis_at_forty_characters() {
    let label = "x".repeat(60);

    let graph = parse(&graph_json(&[node_json("a", &label)], &[])).unwrap();

    assert_eq!(graph.nodes[0].label.chars().count(), 40);
    assert!(graph.nodes[0].label.ends_with('…'));
    assert!(graph.nodes[0].label.starts_with("xxx"));
}

#[test]
fn label_of_exactly_forty_characters_is_kept() {
    let label = "y".repeat(40);

    let graph = parse(&graph_json(&[node_json("a", &label)], &[])).unwrap();

    assert_eq!(graph.nodes[0].label, label);
}

#[test]
fn label_that_is_empty_after_cleaning_rejects_the_graph() {
    for label in ["", "   ", "\\u200b\\u0007"] {
        assert_eq!(
            parse(&graph_json(&[node_json("a", label)], &[])),
            None,
            "{label:?}"
        );
    }
}

#[test]
fn repo_that_is_not_owner_slash_name_becomes_none() {
    let nodes: Vec<String> = [
        ("a", "acme/api"),
        ("b", "acme"),
        ("c", "acme/api/extra"),
        ("d", "ac me/api"),
        ("e", "/api"),
        ("f", "acme/"),
        ("g", "https://x.com/a"),
        ("h", "Ac.me_1/a-p.i"),
        ("i", "../api"),
        ("j", "acme/.."),
    ]
    .iter()
    .map(|(id, repo)| {
        format!(r#"{{"id":"{id}","label":"N","lane":"uses","kind":"service","repo":"{repo}"}}"#)
    })
    .collect();

    let graph = parse(&graph_json(&nodes, &[])).unwrap();

    let repos: Vec<Option<&str>> = graph.nodes.iter().map(|n| n.repo.as_deref()).collect();
    assert_eq!(
        repos,
        [
            Some("acme/api"),
            None,
            None,
            None,
            None,
            None,
            None,
            Some("Ac.me_1/a-p.i"),
            None,
            None
        ]
    );
}

#[test]
fn edge_with_a_missing_endpoint_is_dropped_and_the_graph_stays() {
    let edges = [
        r#"{"from":"a","to":"b","label":"ok","via":"call"}"#.to_owned(),
        r#"{"from":"a","to":"fantasma","via":"call"}"#.to_owned(),
        r#"{"from":"fantasma","to":"b","via":"call"}"#.to_owned(),
    ];

    let graph = parse(&graph_json(
        &[node_json("a", "A"), node_json("b", "B")],
        &edges,
    ))
    .unwrap();

    assert_eq!(graph.edges.len(), 1);
    assert_eq!(graph.edges[0].label, "ok");
}

#[test]
fn edge_label_is_cleaned_and_may_be_empty() {
    let edges = [
        "{\"from\":\"a\",\"to\":\"b\",\"label\":\"x\\u202ey\",\"via\":\"call\"}".to_owned(),
        "{\"from\":\"b\",\"to\":\"a\",\"label\":\"\\u200b\",\"via\":\"call\"}".to_owned(),
    ];

    let graph = parse(&graph_json(
        &[node_json("a", "A"), node_json("b", "B")],
        &edges,
    ))
    .unwrap();

    assert_eq!(graph.edges[0].label, "x y");
    assert_eq!(graph.edges[1].label, "");
}

#[test]
fn touched_reads_one_id_per_line() {
    let message = touched_block(NONCE, "consultas\nfila-eventos\n");

    assert_eq!(
        parse_touched(&message, NONCE),
        ["consultas", "fila-eventos"]
    );
}

#[test]
fn touched_ignores_blank_lines_invalid_ids_and_duplicates() {
    let message = touched_block(NONCE, "a\n\n  b  \nInvalido\nc d\na\n-\nb\n");

    assert_eq!(parse_touched(&message, NONCE), ["a", "b", "-"]);
}

#[test]
fn touched_is_capped_at_forty_ids() {
    let body: Vec<String> = (0..50).map(|i| format!("n{i}")).collect();
    let message = touched_block(NONCE, &body.join("\n"));

    let ids = parse_touched(&message, NONCE);

    assert_eq!(ids.len(), 40);
    assert_eq!(ids[39], "n39");
}

#[test]
fn touched_with_another_nonce_or_without_block_is_empty() {
    assert!(parse_touched(&touched_block(OTHER_NONCE, "a"), NONCE).is_empty());
    assert!(parse_touched("nada", NONCE).is_empty());
}

#[test]
fn touched_quoted_inside_another_block_is_ignored() {
    let message = format!("````md\n```argon-touched-{NONCE}\na\n```\n````\n");

    assert!(parse_touched(&message, NONCE).is_empty());
}

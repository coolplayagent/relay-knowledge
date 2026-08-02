use super::GraphCanvasKind;

#[test]
fn graph_canvas_kind_round_trips_stable_query_values() {
    for (value, expected) in [
        ("knowledge", GraphCanvasKind::Knowledge),
        ("code", GraphCanvasKind::Code),
        ("mixed", GraphCanvasKind::Mixed),
    ] {
        let parsed = GraphCanvasKind::parse(value).expect("stable kind should parse");

        assert_eq!(parsed, expected);
        assert_eq!(parsed.as_str(), value);
    }
}

#[test]
fn graph_canvas_kind_rejects_unknown_query_values() {
    assert_eq!(
        GraphCanvasKind::parse("all"),
        Err("unsupported graph canvas kind 'all'".to_owned())
    );
}

use super::*;

fn bundle() -> AuthoredEvidenceBundle {
    serde_json::from_value(serde_json::json!({
        "schema_version":1,"id":"test","source_scope":"research",
        "graph":{"nodes":[{"id":"a","kind":"concept","label":"Alpha"},{"id":"b","kind":"concept","label":"Beta"}],
        "edges":[{"source":"a","target":"b","relation":"depends_on","evidence":["pin"]}]},
        "evidence":[{"id":"pin","source_scope":"research","artifact":{"path_base":"repository","path":"source.txt","sha256":"a".repeat(64)},"interpretation":"source_statement","span":{"start_byte":0,"end_byte":1,"start_line":1,"end_line":1}}]
    })).unwrap()
}

#[test]
fn rejects_schema_size_span_scope_and_alias_violations() {
    let original = bundle();
    original.validate_shape().unwrap();
    let mut invalid = Vec::new();
    let mut value = original.clone();
    value.schema_version = 2;
    invalid.push(value);
    let mut value = original.clone();
    value.graph.nodes.clear();
    invalid.push(value);
    let mut value = original.clone();
    value.graph.nodes = vec![value.graph.nodes[0].clone(); 513];
    invalid.push(value);
    let mut value = original.clone();
    value.graph.edges = vec![value.graph.edges[0].clone(); 2049];
    invalid.push(value);
    let mut value = original.clone();
    value.evidence = vec![value.evidence[0].clone(); 513];
    invalid.push(value);
    let mut value = original.clone();
    value.supersedes = Some("bad".into());
    invalid.push(value);
    let mut value = original.clone();
    value.source_scope = " ".into();
    invalid.push(value);
    let mut value = original.clone();
    value.graph.edges[0].id = Some("".into());
    invalid.push(value);
    let mut value = original.clone();
    value.graph.edges[0].evidence = vec!["pin".into(); 33];
    invalid.push(value);
    let mut value = original.clone();
    value.evidence[0].span.as_mut().unwrap().end_byte = 0;
    invalid.push(value);
    let mut value = original.clone();
    value.aliases.insert("Old".into(), "missing".into());
    invalid.push(value);
    let mut value = original.clone();
    value
        .graph
        .metadata
        .insert("huge".into(), Value::String("x".repeat(2 * 1024 * 1024)));
    invalid.push(value);
    for (i, value) in invalid.iter().enumerate() {
        assert!(value.validate_shape().is_err(), "case {i}");
    }
    let mut unknown = serde_json::to_value(&original).unwrap();
    unknown["typo"] = true.into();
    assert!(serde_json::from_value::<AuthoredEvidenceBundle>(unknown).is_err());
}

#[test]
fn clarification_preserves_identity_and_rejects_ambiguous_aliases() {
    let original = bundle();
    let (revision, affected) = original
        .revise_label("a", "Clarified", "b".repeat(64))
        .unwrap();
    assert_eq!(affected, [0]);
    assert_eq!(revision.aliases["Alpha"], "a");
    assert_eq!(revision.graph.edges, original.graph.edges);
    assert_eq!(revision.evidence, original.evidence);
    assert_eq!(original.graph.nodes[0].label, "Alpha");
    assert!(
        original
            .revise_label("missing", "Name", "b".repeat(64))
            .is_err()
    );
    assert!(original.revise_label("a", " ", "b".repeat(64)).is_err());
    let mut ambiguous = original;
    ambiguous.aliases.insert("Alpha".into(), "b".into());
    assert!(ambiguous.revise_label("a", "Name", "b".repeat(64)).is_err());
}

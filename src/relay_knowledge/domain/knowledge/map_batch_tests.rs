use super::*;

fn request(operations: Vec<MapSourceOperation>) -> MapBatchRequest {
    MapBatchRequest {
        schema_version: 1,
        transaction_id: "research-1".into(),
        expected_map_version: None,
        expected_digest: None,
        operations,
    }
}

fn add(id: &str) -> MapSourceOperation {
    MapSourceOperation::Add {
        id: id.into(),
        topic: "research".into(),
        kind: KnowledgeMapSourceKind::File,
        uri: format!("sources/{id}.txt"),
        source_scope: Some("repo".into()),
        description: None,
    }
}

#[test]
fn ten_sources_preserve_version_until_the_single_publication() {
    let map = KnowledgeMap::initial("test".into());
    let request = request((0..10).map(|i| add(&format!("source-{i}"))).collect());
    request.validate().unwrap();
    let (candidate, preview) = request.preview(&map, 0);
    assert!(preview.diagnostics.is_empty());
    assert_eq!(candidate.map_version, 1);
    assert_eq!(candidate.sources.len(), map.sources.len() + 10);
    assert_eq!(preview.differences.len(), 10);
    assert_eq!(preview.affected_routes, ["research"]);
}

#[test]
fn invalid_prefix_and_reserved_operations_never_escape_candidate_state() {
    let map = KnowledgeMap::initial("test".into());
    for operation in [
        add("one"),
        MapSourceOperation::Remove {
            id: "repository-software-model".into(),
        },
        MapSourceOperation::Remove {
            id: "absent".into(),
        },
    ] {
        let (candidate, preview) = request(vec![add("one"), operation]).preview(&map, 0);
        assert_eq!(candidate, map);
        assert_eq!(preview.diagnostics[0].operation_index, Some(1));
    }
}

#[test]
fn ordered_changes_report_normalized_before_after_and_route_moves() {
    let map = KnowledgeMap::initial("test".into());
    let change = KnowledgeMapChange {
        id: "one".into(),
        topic: Some("other".into()),
        kind: None,
        uri: None,
        source_scope: None,
        description: Some(" description ".into()),
    };
    let (candidate, preview) = request(vec![
        add("one"),
        MapSourceOperation::Update { change },
        MapSourceOperation::Remove { id: "one".into() },
    ])
    .preview(&map, 0);
    assert!(preview.diagnostics.is_empty());
    assert_eq!(preview.affected_routes, ["other", "research"]);
    assert_eq!(
        preview.differences[1]
            .after
            .as_ref()
            .unwrap()
            .description
            .as_deref(),
        Some("description")
    );
    assert_eq!(preview.differences[1].after.as_ref().unwrap().version, 2);
    assert_eq!(candidate.sources, map.sources);
}

#[test]
fn rejects_unsafe_paths_unknown_fields_and_unbounded_requests() {
    let mut request = request(vec![add("one")]);
    let valid = request.clone();
    request.schema_version = 2;
    assert!(request.validate().is_err());
    request = valid.clone();
    request.transaction_id = "bad id".into();
    assert!(request.validate().is_err());
    request = valid.clone();
    request.operations.clear();
    assert!(request.validate().is_err());
    request = valid.clone();
    request.operations = vec![add("one"); 101];
    assert!(request.validate().is_err());
    request = valid.clone();
    request.expected_map_version = Some(0);
    assert!(request.validate().is_err());
    request = valid.clone();
    request.expected_digest = Some("abc".into());
    assert!(request.validate().is_err());
    request = valid.clone();
    if let MapSourceOperation::Add { description, .. } = &mut request.operations[0] {
        *description = Some("x".repeat(MAX_BATCH_BYTES));
    }
    assert!(request.validate().is_err());
    for path in [
        "../secret",
        "/absolute",
        "C:/outside",
        "nested\\outside",
        " ../secret ",
        " /absolute ",
    ] {
        assert!(validate_local_uri(KnowledgeMapSourceKind::File, path).is_err());
    }
    assert!(validate_local_uri(KnowledgeMapSourceKind::Wiki, "https://example.org").is_ok());
    let mut json = serde_json::to_value(valid).unwrap();
    json["typo"] = true.into();
    assert!(serde_json::from_value::<MapBatchRequest>(json).is_err());
}

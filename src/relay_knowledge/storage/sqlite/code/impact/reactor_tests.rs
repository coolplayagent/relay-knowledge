use super::*;

#[test]
fn empty_changes_do_not_require_reactor_tables() {
    let connection = Connection::open_in_memory().unwrap();
    assert!(
        crate::storage::sqlite::maven::reactor::downstream(
            &connection,
            "scope",
            &BTreeSet::new(),
            &[]
        )
        .unwrap()
        .is_empty()
    );
}

#[test]
fn maven_impact_respects_request_and_indexed_language_and_path_filters() {
    let connection = Connection::open_in_memory().unwrap();
    crate::storage::sqlite::maven::reactor::initialize_schema(&connection).unwrap();
    connection.execute_batch("INSERT INTO maven_reactor_status VALUES ('scope',1);
        INSERT INTO maven_reactor_modules VALUES ('scope','a','a/pom.xml','a','{}'),('scope','b','b/pom.xml','b','{}');").unwrap();
    let edge = crate::domain::SoftwareRelationship::new(crate::domain::SoftwareRelationshipInput {
        repository_id: "repo".into(),
        source_scope: "scope".into(),
        relationship_kind: "depends_on".into(),
        source_id: "a".into(),
        source_kind: "module".into(),
        target_id: "b".into(),
        target_kind: "module".into(),
        target_hint: Some("x:b:1".into()),
        resolution_state: "resolved".into(),
        confidence_basis_points: 10000,
        confidence_tier: "extracted".into(),
        evidence_path: "a/pom.xml".into(),
        evidence_line_range: RepositoryCodeRange { start: 1, end: 1 },
        created_graph_version: crate::domain::GraphVersion::ZERO,
    })
    .unwrap();
    connection.execute("INSERT INTO maven_reactor_edges VALUES ('scope','edge','a','b','depends_on','resolved','compile',NULL,?1)", [serde_json::to_string(&edge).unwrap()]).unwrap();
    let mut status = CodeRepositoryStatus {
        repository_id: "repo".into(),
        alias: "repo".into(),
        root_path: "/repo".into(),
        path_filters: vec![],
        language_filters: vec![],
        last_indexed_scope_id: Some("scope".into()),
        last_indexed_commit: Some("commit".into()),
        tree_hash: Some("tree".into()),
        state: "fresh".into(),
        indexed_file_count: 2,
        symbol_count: 0,
        reference_count: 0,
        chunk_count: 0,
        stale: false,
        degraded_reason: None,
    };
    let mut request = CodeImpactRequest::new(
        crate::domain::CodeRepositorySelector::new("repo", "head", vec![], vec![]).unwrap(),
        "base",
        "head",
        10,
    )
    .unwrap();
    let changed = BTreeSet::from(["b/src/Api.java".into()]);
    assert_eq!(
        module_impacts(&connection, &status, &request, &changed)
            .unwrap()
            .len(),
        1
    );
    request.repository.language_filters = vec!["java".into()];
    assert!(
        module_impacts(&connection, &status, &request, &changed)
            .unwrap()
            .is_empty()
    );
    request.repository.language_filters = vec!["xml".into()];
    assert_eq!(
        module_impacts(&connection, &status, &request, &changed)
            .unwrap()
            .len(),
        1
    );
    status.language_filters = vec!["java".into()];
    assert!(
        module_impacts(&connection, &status, &request, &changed)
            .unwrap()
            .is_empty()
    );
    status.language_filters.clear();
    status.path_filters = vec!["b".into()];
    assert!(
        module_impacts(&connection, &status, &request, &changed)
            .unwrap()
            .is_empty()
    );
    connection
        .execute("UPDATE maven_reactor_status SET complete = 0", [])
        .unwrap();
    status.path_filters.clear();
    request.repository.language_filters = vec!["java".into()];
    assert!(
        module_impacts(&connection, &status, &request, &changed)
            .unwrap()
            .is_empty()
    );
    request.repository.language_filters.clear();
    status.language_filters = vec!["java".into()];
    assert!(
        module_impacts(&connection, &status, &request, &changed)
            .unwrap()
            .is_empty()
    );
    status.language_filters.clear();
    assert!(module_impacts(&connection, &status, &request, &changed).is_err());
}

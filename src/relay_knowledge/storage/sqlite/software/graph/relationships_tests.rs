use super::*;

#[test]
fn relationship_row_mapping_preserves_resolution_and_evidence() {
    let connection = Connection::open_in_memory().expect("database should open");

    let relationship = connection
        .query_row(
            "
            SELECT 'repository', 'scope', 'depends_on',
                   'source', 'file', 'target', 'component', 'serde',
                   'declared', 9000, 'extracted', 'Cargo.toml', 3, 4, 11
            ",
            [],
            relationship_from_row,
        )
        .expect("software relationship should decode");

    assert_eq!(relationship.relationship_kind, "depends_on");
    assert_eq!(relationship.target_hint.as_deref(), Some("serde"));
    assert_eq!(
        relationship.evidence_line_range,
        RepositoryCodeRange { start: 3, end: 4 }
    );
    assert_eq!(relationship.created_graph_version, GraphVersion::new(11));
}

#[test]
fn relationship_language_filter_binds_source_and_component_languages() {
    let filters = vec!["rust".to_owned(), "toml".to_owned()];
    let sql = relationship_language_filter_sql(&filters);
    let mut values = Vec::new();

    push_relationship_language_filter_values(&mut values, &filters);

    assert_eq!(sql.matches("files.language_id = ?").count(), 2);
    assert_eq!(
        sql.matches("relationships.component_language = ?").count(),
        2
    );
    assert_eq!(
        values,
        vec![
            Value::Text("rust".to_owned()),
            Value::Text("rust".to_owned()),
            Value::Text("toml".to_owned()),
            Value::Text("toml".to_owned()),
        ]
    );
}

fn relationship_fixture() -> Connection {
    let connection = Connection::open_in_memory().expect("database");
    super::super::super::schema::initialize_schema(&connection).expect("software schema");
    connection.execute_batch(
        "CREATE TABLE code_repository_feature_flags (
            source_scope TEXT, feature_flag_id TEXT, usage_id TEXT, source_key TEXT,
            edge_kind TEXT, confidence_basis_points INTEGER, confidence_tier TEXT,
            path TEXT, line_start INTEGER, line_end INTEGER
         );
         CREATE INDEX flag_scope ON code_repository_feature_flags(source_scope);
         INSERT INTO software_global_status (
            source_scope, repository_id, projected_graph_version, stale,
            component_count, sdk_usage_count
         ) VALUES ('scope', 'repo', 42, 0, 1, 1);
         INSERT INTO software_files VALUES
            ('file-map', 'repo', 'scope', 'knowledge/knowledge-map.yaml', 'yaml', 'knowledge_map_manifest', 'parsed', 1),
            ('file-code', 'repo', 'scope', 'src/lib.rs', 'rust', 'source', 'parsed', 1),
            ('file-deps', 'repo', 'scope', 'Cargo.toml', 'toml', 'dependency_manifest', 'parsed', 1);
         INSERT INTO software_topics VALUES
            ('topic', 'repo', 'scope', 'architecture', 'knowledge_map_topic', 'knowledge/knowledge-map.yaml', 3, 4, 1),
            ('missing', 'repo', 'scope', 'unauthorized', 'knowledge_map_topic', 'outside.yaml', 1, 1, 1),
            ('other-scope', 'repo', 'other', 'hidden', 'knowledge_map_topic', 'knowledge/knowledge-map.yaml', 1, 1, 1);
         INSERT INTO software_components VALUES
            ('component', 'repo', 'scope', 'cargo', 'dep', '1', NULL, 'normal', 'manifest', 'declared', 'rust', 'Cargo.toml', 5, 5, 10000, 1);
         INSERT INTO software_sdk_usages VALUES
            ('sdk', 'repo', 'scope', 'rust', 'external_sdk', NULL, 'unresolved', 'src/lib.rs', 2, 2, 5000, 1);
         INSERT INTO code_repository_feature_flags (source_scope,feature_flag_id,usage_id,source_key,edge_kind,confidence_basis_points,confidence_tier,path,line_start,line_end) VALUES
            ('scope', 'flag', 'read', 'FEATURE', 'reads_config', 8000, 'inferred', 'src/lib.rs', 8, 8),
            ('scope', 'flag', 'guard', 'FEATURE', 'guards_code', 9000, 'extracted', 'src/lib.rs', 8, 10),
            ('scope', 'flag', 'reference', 'FEATURE', 'other', 7000, 'ambiguous', 'src/lib.rs', 8, 8),
            ('scope', 'internal', 'candidate', 'HELLO', 'declares_string_constant', 9000, 'extracted', 'src/lib.rs', 9, 9),
            ('scope', 'internal', 'getter', 'getHello', 'declares_config_getter', 9000, 'extracted', 'src/lib.rs', 10, 10);
         ALTER TABLE code_repository_feature_flags ADD COLUMN source_kind TEXT DEFAULT 'config_key';"
    ).expect("scoped evidence");
    connection
}

fn relationship_request(paths: Vec<String>, languages: Vec<String>) -> SoftwareGlobalRequest {
    SoftwareGlobalRequest::new(
        crate::domain::CodeRepositorySelector::new("repo", "HEAD", paths, languages)
            .expect("selector"),
        crate::domain::SoftwareGlobalKind::Relationships,
        crate::domain::FreshnessPolicy::AllowStale,
        500,
    )
    .expect("request")
}

#[test]
fn software_relationship_storage_preserves_identity_evidence_and_distinct_edge_kinds() {
    let connection = relationship_fixture();
    let changes = connection.total_changes();
    let request = relationship_request(Vec::new(), Vec::new());
    let edges = relationships_for_scope(&connection, "scope", &request, 1000).expect("edges");
    assert_eq!(edges.len(), 5);
    assert_eq!(
        relationship_count_for_scope(&connection, "scope").unwrap(),
        edges.len()
    );
    assert_eq!(
        connection.total_changes(),
        changes,
        "reads and counts must not persist edges"
    );
    assert_eq!(
        edges
            .iter()
            .map(|edge| edge.relationship_kind.as_str())
            .collect::<Vec<_>>(),
        [
            "depends_on",
            "uses_sdk",
            "documents",
            "configures",
            "references"
        ]
    );
    let expected = SoftwareRelationship::new(SoftwareRelationshipInput {
        repository_id: "repo".into(),
        source_scope: "scope".into(),
        relationship_kind: "documents".into(),
        source_id: "file-map".into(),
        source_kind: "file".into(),
        target_id: "topic".into(),
        target_kind: "topic".into(),
        target_hint: Some("architecture".into()),
        resolution_state: "resolved".into(),
        confidence_basis_points: 10000,
        confidence_tier: "extracted".into(),
        evidence_path: "knowledge/knowledge-map.yaml".into(),
        evidence_line_range: RepositoryCodeRange { start: 3, end: 4 },
        created_graph_version: GraphVersion::new(42),
    })
    .unwrap();
    assert_eq!(
        edges[2], expected,
        "stable ID and complete compatibility payload"
    );
    assert_eq!(edges[1].resolution_state, "unresolved");
    assert_eq!(edges[1].target_hint.as_deref(), Some("external_sdk"));
    assert_eq!(edges[3].confidence_basis_points, 9000);
    assert_eq!(edges[3].evidence_line_range.end, 10);
    assert!(
        edges
            .iter()
            .all(|edge| edge.created_graph_version == GraphVersion::new(42))
    );
    assert_eq!(
        edges,
        relationships_for_scope(&connection, "scope", &request, 1000).unwrap()
    );
}

#[test]
fn software_relationship_storage_filters_before_limit_and_confines_snapshot() {
    let connection = relationship_fixture();
    let request = relationship_request(vec!["knowledge".into()], vec!["yaml".into()]);
    let edges = relationships_for_scope(&connection, "scope", &request, 1).unwrap();
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].target_id, "topic");
    assert!(
        relationships_for_scope(&connection, "other", &request, 1)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        relationship_count_for_scope(&connection, "other").unwrap(),
        0
    );
    let rust_request = relationship_request(Vec::new(), vec!["rust".into()]);
    let edges = relationships_for_scope(&connection, "scope", &rust_request, 1000).unwrap();
    assert_eq!(edges.len(), 4);
    assert_eq!(
        edges[0].evidence_path, "Cargo.toml",
        "component language admits its manifest"
    );
    let excluded = relationship_request(vec!["src2".into()], Vec::new());
    assert!(
        relationships_for_scope(&connection, "scope", &excluded, 1000)
            .unwrap()
            .is_empty()
    );
    assert!(
        relationships_for_scope(&connection, "scope", &rust_request, 0)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn software_relationship_storage_scales_without_copying_map_edges() {
    let mut connection = relationship_fixture();
    let transaction = connection.transaction().unwrap();
    for index in 0..4096 {
        transaction
            .execute(
                "INSERT INTO software_topics VALUES (?1, 'repo', 'scope', ?1,
             'knowledge_map_topic', 'knowledge/knowledge-map.yaml', ?2, ?2, 42)",
                params![format!("dimension-{index:04}"), index + 10],
            )
            .unwrap();
    }
    transaction.commit().unwrap();
    let changes = connection.total_changes();
    let pages: u64 = connection
        .query_row("PRAGMA page_count", [], |row| row.get(0))
        .unwrap();
    let started = std::time::Instant::now();
    assert_eq!(
        relationship_count_for_scope(&connection, "scope").unwrap(),
        4101
    );
    let count_elapsed = started.elapsed();
    let request = relationship_request(vec!["knowledge".into()], Vec::new());
    let started = std::time::Instant::now();
    let edges = relationships_for_scope(&connection, "scope", &request, 1000).unwrap();
    let query_elapsed = started.elapsed();
    assert_eq!(edges.len(), 1000);
    assert_eq!(edges[999].target_hint.as_deref(), Some("dimension-0998"));
    assert_eq!(connection.total_changes(), changes);
    let after: u64 = connection
        .query_row("PRAGMA page_count", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        pages, after,
        "projection must not allocate persistent edge pages"
    );
    let stored: usize = connection
        .query_row("SELECT COUNT(*) FROM software_relationships", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(stored, 0);
    assert!(
        count_elapsed.as_secs_f64() < 2.0,
        "count must avoid repeated offset scans"
    );
    assert!(
        query_elapsed.as_secs_f64() < 2.0,
        "bounded read must remain interactive"
    );
    println!(
        "software_relationship_storage topics=4096 stored_edges=0 count_ms={:.3} query_1000_ms={:.3}",
        count_elapsed.as_secs_f64() * 1000.0,
        query_elapsed.as_secs_f64() * 1000.0
    );
}

#[test]
fn software_relationship_storage_reports_invalid_facts_and_sql_errors() {
    let connection = relationship_fixture();
    connection
        .execute(
            "UPDATE software_components SET confidence_basis_points = 10001",
            [],
        )
        .unwrap();
    let request = relationship_request(Vec::new(), Vec::new());
    assert!(matches!(
        relationship_count_for_scope(&connection, "scope"),
        Err(StorageError::InvalidInput(_))
    ));
    assert!(matches!(
        relationships_for_scope(&connection, "scope", &request, 1),
        Err(StorageError::InvalidInput(_))
    ));
    connection
        .execute("DROP TABLE software_topics", [])
        .unwrap();
    assert!(relationship_count_for_scope(&connection, "scope").is_err());
    assert!(relationships_for_scope(&connection, "scope", &request, 1).is_err());
}

#[test]
fn software_relationship_storage_validates_configuration_facts_before_deduplication() {
    for mutation in [
        "UPDATE code_repository_feature_flags SET confidence_basis_points = 10001",
        "UPDATE code_repository_feature_flags SET feature_flag_id = ''",
        "UPDATE code_repository_feature_flags SET source_key = ''",
        // Rust Unicode whitespace validation must also cover a losing duplicate.
        "UPDATE code_repository_feature_flags SET confidence_tier = '\u{2003}' WHERE usage_id = 'read'",
    ] {
        let connection = relationship_fixture();
        connection.execute(mutation, []).unwrap();
        let changes = connection.total_changes();
        assert!(
            matches!(
                relationship_count_for_scope(&connection, "scope"),
                Err(StorageError::InvalidInput(_))
            ),
            "invalid fact reached the publication count: {mutation}"
        );
        assert_eq!(connection.total_changes(), changes);
    }
}

#[test]
fn software_relationship_storage_normalizes_configuration_identity_before_ranking() {
    let whitespace: String = (0..=u32::from(char::MAX))
        .filter_map(char::from_u32)
        .filter(|character| character.is_whitespace())
        .collect();
    assert_eq!(RELATIONSHIP_TRIM_CHARACTERS, whitespace);
    for target in [
        " flag ",
        "\tflag\n",
        "\u{2003}flag\u{3000}",
        "\u{0085}flag\u{00a0}",
    ] {
        let connection = relationship_fixture();
        connection
            .execute(
                "INSERT INTO code_repository_feature_flags (source_scope,feature_flag_id,usage_id,source_key,edge_kind,confidence_basis_points,confidence_tier,path,line_start,line_end) VALUES
                 ('scope', ?1, 'normalized-duplicate', 'FEATURE', 'reads_config',
                  9500, 'extracted', 'src/lib.rs', 8, 12)",
                [target],
            )
            .unwrap();
        let request = relationship_request(Vec::new(), Vec::new());
        let edges = relationships_for_scope(&connection, "scope", &request, 1000).unwrap();
        assert_eq!(edges.len(), 5, "duplicate normalized target: {target:?}");
        assert_eq!(
            relationship_count_for_scope(&connection, "scope").unwrap(),
            5
        );
        let identities = edges
            .iter()
            .map(|edge| &edge.relationship_id)
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(identities.len(), edges.len());
        let configuration = edges
            .iter()
            .find(|edge| edge.relationship_kind == "configures")
            .unwrap();
        assert_eq!(configuration.target_id, "flag");
        assert_eq!(configuration.confidence_basis_points, 9500);
        assert_eq!(configuration.evidence_line_range.end, 12);
    }
}

#[test]
fn software_relationship_storage_filters_configuration_before_window_work() {
    use std::sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    };

    let connection = relationship_fixture();
    connection.execute_batch(
        "INSERT INTO software_files VALUES
            ('selected', 'repo', 'scope', 'config/feature.ini', 'ini', 'configuration', 'parsed', 1),
            ('unrelated', 'repo', 'scope', 'other/source.rs', 'rust', 'source', 'parsed', 1);
         INSERT INTO code_repository_feature_flags (source_scope,feature_flag_id,usage_id,source_key,edge_kind,confidence_basis_points,confidence_tier,path,line_start,line_end) VALUES
            ('scope', 'selected-flag', 'selected', 'FEATURE', 'reads_config', 9000, 'extracted', 'config/feature.ini', 1, 1);
         WITH RECURSIVE numbers(value) AS (
             SELECT 1 UNION ALL SELECT value + 1 FROM numbers WHERE value < 16384
         )
         INSERT INTO code_repository_feature_flags (source_scope,feature_flag_id,usage_id,source_key,edge_kind,confidence_basis_points,confidence_tier,path,line_start,line_end)
         SELECT 'scope', 'flag-' || value, 'usage-' || value, 'FEATURE',
                'reads_config', 8000, 'inferred', 'other/source.rs', value, value
         FROM numbers;"
    ).unwrap();
    let changes = connection.total_changes();
    let measured_query = |request: &SoftwareGlobalRequest| {
        let steps = Arc::new(AtomicU64::new(0));
        let observed = Arc::clone(&steps);
        connection.progress_handler(
            100,
            Some(move || {
                observed.fetch_add(100, Ordering::Relaxed);
                false
            }),
        );
        let result = relationships_for_scope(&connection, "scope", request, 1);
        connection.progress_handler(0, None::<fn() -> bool>);
        (steps.load(Ordering::Relaxed), result.unwrap())
    };
    let (full_steps, _) = measured_query(&relationship_request(Vec::new(), Vec::new()));
    for request in [
        relationship_request(vec!["config".into()], Vec::new()),
        relationship_request(Vec::new(), vec!["ini".into()]),
        relationship_request(vec!["config".into()], vec!["ini".into()]),
    ] {
        let (filtered_steps, edges) = measured_query(&request);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].target_id, "selected-flag");
        assert!(
            filtered_steps * 2 < full_steps,
            "request predicates must reduce VM work before ranking: filtered={filtered_steps}, full={full_steps}"
        );
        eprintln!("configuration_window_vm_steps filtered={filtered_steps} full={full_steps}");
    }
    assert_eq!(connection.total_changes(), changes);
}

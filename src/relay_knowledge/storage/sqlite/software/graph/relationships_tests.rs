use super::*;

#[test]
fn relationship_row_mapping_preserves_resolution_and_evidence() {
    let connection = Connection::open_in_memory().expect("database should open");

    let relationship = connection
        .query_row(
            "
            SELECT 'relationship', 'repository', 'scope', 'depends_on',
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
    assert_eq!(sql.matches("components.language_id = ?").count(), 2);
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

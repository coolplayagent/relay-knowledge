use super::*;

#[test]
fn file_and_topic_identities_include_source_evidence() {
    let file = SoftwareFile::new(SoftwareFileInput {
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        path: "src/lib.rs".to_owned(),
        language_id: "rust".to_owned(),
        file_role: "source".to_owned(),
        parse_status: "parsed".to_owned(),
        created_graph_version: GraphVersion::new(3),
    })
    .expect("file should validate");
    let topic = SoftwareTopic::new(SoftwareTopicInput {
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        name: "storage".to_owned(),
        topic_kind: "architecture".to_owned(),
        source_path: "README.md".to_owned(),
        line_range: RepositoryCodeRange { start: 4, end: 8 },
        created_graph_version: GraphVersion::new(3),
    })
    .expect("topic should validate");

    assert!(file.software_file_id.starts_with("file:"));
    assert!(topic.topic_id.starts_with("topic:"));
}

#[test]
fn relationship_preserves_unresolved_target_metadata() {
    let relationship = SoftwareRelationship::new(SoftwareRelationshipInput {
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        relationship_kind: "imports".to_owned(),
        source_id: "file:1".to_owned(),
        source_kind: "file".to_owned(),
        target_id: "external:securec".to_owned(),
        target_kind: "external_module".to_owned(),
        target_hint: Some("securec.h".to_owned()),
        resolution_state: "unresolved".to_owned(),
        confidence_basis_points: 7000,
        confidence_tier: "high".to_owned(),
        evidence_path: "src/main.cc".to_owned(),
        evidence_line_range: RepositoryCodeRange { start: 2, end: 2 },
        created_graph_version: GraphVersion::new(3),
    })
    .expect("relationship should validate");

    assert_eq!(relationship.resolution_state, "unresolved");
    assert_eq!(relationship.target_hint.as_deref(), Some("securec.h"));
}

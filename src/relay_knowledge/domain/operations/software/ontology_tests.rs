use std::collections::BTreeMap;

use super::*;

fn entity(scope: &str, kind: SoftwareEntityKind) -> SoftwareEntity {
    SoftwareEntity::new(SoftwareEntityInput {
        repository_id: "repo-1".to_owned(),
        source_scope: scope.to_owned(),
        entity_kind: kind,
        name: "relay-api".to_owned(),
        namespace: Some("rust".to_owned()),
        source_kind: SoftwareSourceKind::Manifest,
        evidence_refs: vec![
            SoftwareEvidenceRef::new(
                scope,
                "Cargo.toml",
                RepositoryCodeRange { start: 2, end: 2 },
            )
            .expect("evidence"),
        ],
        attributes: BTreeMap::new(),
        created_graph_version: GraphVersion::new(3),
    })
    .expect("entity")
}

#[test]
fn stable_entity_key_survives_snapshot_changes_while_occurrence_changes() {
    let first = entity("git_snapshot:first", SoftwareEntityKind::Component);
    let second = entity("git_snapshot:second", SoftwareEntityKind::Component);

    assert_eq!(first.entity_key, second.entity_key);
    assert_ne!(first.occurrence_id, second.occurrence_id);
}

#[test]
fn snapshot_entity_identity_remains_occurrence_bound() {
    let first = entity("git_snapshot:first", SoftwareEntityKind::RepositorySnapshot);
    let second = entity(
        "git_snapshot:second",
        SoftwareEntityKind::RepositorySnapshot,
    );

    assert_ne!(first.entity_key, second.entity_key);
    assert_ne!(first.occurrence_id, second.occurrence_id);
}

#[test]
fn evidence_rejects_invalid_line_ranges() {
    let error = SoftwareEvidenceRef::new(
        "scope",
        "Cargo.toml",
        RepositoryCodeRange { start: 4, end: 3 },
    )
    .expect_err("unordered lines must fail");

    assert!(error.to_string().contains("positive ordered"));
}

use super::*;
use crate::{
    domain::{EvidenceRecord, GraphMutationBatch},
    storage::sqlite::schema::initialization,
};

#[test]
fn exact_lookup_preserves_content_and_cannot_cross_scopes() {
    let mut connection = Connection::open_in_memory().unwrap();
    initialization::initialize_schema(&connection).unwrap();
    let evidence = EvidenceRecord::new(
        "bundle:one",
        SourceScope::parse("research").unwrap(),
        "{\"claims\":[]}",
        vec!["one".into()],
    )
    .unwrap()
    .with_metadata(
        None,
        None,
        crate::domain::ConfidenceScore::CERTAIN,
        FactStatus::Proposed,
    )
    .unwrap();
    super::super::commit_batch(
        &mut connection,
        GraphMutationBatch::new(vec![evidence]).unwrap(),
    )
    .unwrap();
    let document = evidence_document(&connection, "bundle:one", "research")
        .unwrap()
        .unwrap();
    assert_eq!(document.content, "{\"claims\":[]}");
    assert_eq!(document.status, FactStatus::Proposed);
    assert_eq!(document.graph_version.get(), 1);
    assert!(
        evidence_document(&connection, "bundle:one", "foreign")
            .unwrap()
            .is_none()
    );
    assert!(
        evidence_document(&connection, "missing", "research")
            .unwrap()
            .is_none()
    );
    assert!(evidence_document(&connection, "", "research").is_err());
    assert!(evidence_document(&connection, "bundle:one", "").is_err());
    connection
        .execute(
            "UPDATE evidence SET content = ?1 WHERE id = 'bundle:one'",
            ["x".repeat(2 * 1024 * 1024 + 1)],
        )
        .unwrap();
    assert!(evidence_document(&connection, "bundle:one", "research").is_err());
    connection
        .execute(
            "UPDATE evidence SET content = 'small', status = 'invalid' WHERE id = 'bundle:one'",
            [],
        )
        .unwrap();
    assert!(evidence_document(&connection, "bundle:one", "research").is_err());
}

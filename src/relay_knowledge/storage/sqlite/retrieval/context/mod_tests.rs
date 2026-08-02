//! Direct context projection and fact-admission invariants.

use super::*;

#[test]
fn code_artifact_projection_accepts_only_supported_document_kinds() {
    let symbol = code_artifact_for_document("code_symbol", "symbol-1", Some("src/lib.rs"))
        .expect("symbol document should project");
    let chunk = code_artifact_for_document("code_chunk", "chunk-1", None)
        .expect("chunk document should project");

    assert_eq!(symbol.kind, CodeGraphArtifactKind::Symbol);
    assert_eq!(symbol.artifact_id, "symbol-1");
    assert_eq!(symbol.path, "src/lib.rs");
    assert_eq!(chunk.kind, CodeGraphArtifactKind::Chunk);
    assert!(chunk.path.is_empty());
    assert!(code_artifact_for_document("evidence", "ev-1", None).is_none());
}

#[test]
fn retrievable_status_allows_only_live_fact_states() {
    assert!(retrievable_status(FactStatus::Accepted));
    assert!(retrievable_status(FactStatus::Proposed));
    assert!(!retrievable_status(FactStatus::Rejected));
    assert!(!retrievable_status(FactStatus::Superseded));
}

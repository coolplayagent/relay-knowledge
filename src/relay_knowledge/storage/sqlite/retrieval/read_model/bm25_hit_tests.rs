use std::collections::BTreeMap;

use rusqlite::Connection;

use super::scored_bm25_hit;
use crate::{
    domain::{CodeGraphArtifactKind, GraphVersion, RetrieverSource},
    storage::sqlite::retrieval::bm25::RawBm25Row,
};

#[test]
fn code_document_hit_maps_artifact_without_graph_queries() {
    let connection = Connection::open_in_memory().expect("database should open");
    let hit = scored_bm25_hit(
        &connection,
        RawBm25Row {
            document_id: "code:symbol:scope:src/lib.rs:symbol".to_owned(),
            document_kind: "code_symbol".to_owned(),
            evidence_id: "symbol".to_owned(),
            parent_evidence_id: None,
            modality: "text_span".to_owned(),
            source_scope: "scope".to_owned(),
            source_path: Some("src/lib.rs".to_owned()),
            entity_labels: vec!["Symbol".to_owned()],
            content: "Symbol function".to_owned(),
            rank: -2.0,
            explanation: Some("hierarchical_bm25 fallback=population_guard".to_owned()),
        },
        GraphVersion::new(1),
        &BTreeMap::new(),
    )
    .expect("code document hit should map");

    assert_eq!(hit.source, RetrieverSource::CodeGraph);
    assert_eq!(hit.source_score, 2.0);
    assert_eq!(
        hit.explanation.as_deref(),
        Some("hierarchical_bm25 fallback=population_guard")
    );
    assert_eq!(
        hit.hit.code_artifact.expect("code artifact").kind,
        CodeGraphArtifactKind::Symbol
    );
}

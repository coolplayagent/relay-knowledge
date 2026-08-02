use crate::{
    domain::{EvidenceRecord, GraphMutationBatch, GraphRelationRecord, SourceScope},
    storage::{GraphStore, SqliteGraphStore},
};

pub(super) async fn commit_evidence(
    store: &SqliteGraphStore,
    id: &str,
    source_scope: &str,
    content: &str,
) {
    let evidence = EvidenceRecord::new(
        id,
        SourceScope::parse(source_scope).expect("scope should parse"),
        content,
        Vec::new(),
    )
    .expect("evidence should validate");
    let batch = GraphMutationBatch::new(vec![evidence]).expect("batch should validate");
    store
        .commit_mutation_batch(batch)
        .await
        .expect("commit should succeed");
}

pub(super) async fn commit_relation(
    store: &SqliteGraphStore,
    relation_id: &str,
    source_scope: &str,
    evidence_id: &str,
) {
    let relation = GraphRelationRecord::new(
        relation_id,
        SourceScope::parse(source_scope).expect("scope should parse"),
        "relay-knowledge",
        "references",
        "cursor metadata preservation",
        vec![evidence_id.to_owned()],
    )
    .expect("relation should validate");
    let batch = GraphMutationBatch::with_facts(Vec::new(), vec![relation], Vec::new(), Vec::new())
        .expect("batch should validate");
    store
        .commit_mutation_batch(batch)
        .await
        .expect("relation commit should succeed");
}

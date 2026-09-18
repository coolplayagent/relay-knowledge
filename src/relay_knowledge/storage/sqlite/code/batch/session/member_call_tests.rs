//! Checkpointed member-call identity and display regression.

use super::*;

#[tokio::test]
async fn checkpointed_finalize_indexes_unresolved_member_name_and_preserves_receiver_hint() {
    let store = registered_store().await;
    let source_scope = "git_snapshot:member-call-finalize";
    let session = session_for_scope(source_scope, 1);
    let mut member_call = reference(
        source_scope,
        "member-call",
        "caller-file",
        "src/dispatch.c",
        "read",
    );
    member_call.target_hint = Some("table[stage].read".to_owned());
    member_call.confidence_tier = "extracted".to_owned();

    store
        .begin_code_index_session(session.clone())
        .await
        .expect("session should begin");
    store
        .apply_code_index_batch(CodeIndexBatch {
            files: vec![file(
                source_scope,
                "caller-file",
                "src/dispatch.c",
                "c",
                CodeParseStatus::Parsed,
            )],
            references: vec![member_call],
            ..batch(source_scope, 1)
        })
        .await
        .expect("batch should persist");
    store
        .finalize_code_index_session(session)
        .await
        .expect("session should finalize");

    let hits = search(&store, "read", CodeQueryKind::Callers).await;
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "src/dispatch.c");
    assert_eq!(
        hits[0].edge_target_hint.as_deref(),
        Some("table[stage].read")
    );
    assert!(
        hits[0].excerpt.contains("calls table[stage].read"),
        "{hits:?}"
    );
    assert_eq!(search_document_count(&store, source_scope, "call").await, 1);
}

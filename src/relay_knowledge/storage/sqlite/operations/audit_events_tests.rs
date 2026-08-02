use crate::{
    domain::AuditStatus,
    storage::{AuditQueryRequest, IndexStore, NewAuditEvent, SqliteGraphStore},
};

#[tokio::test]
async fn sqlite_audit_events_round_trip() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .insert_audit_event(NewAuditEvent {
            operation: "proposal.reject".to_owned(),
            interface: "cli".to_owned(),
            request_id: "req-audit".to_owned(),
            trace_id: "trace-audit".to_owned(),
            status: AuditStatus::Completed,
            actor: Some("reviewer".to_owned()),
            source_scope: Some("docs".to_owned()),
            graph_version: 2,
            detail_json: "{\"proposal\":\"proposal:fixture\"}".to_owned(),
            message: None,
            now_ms: 30,
        })
        .await
        .expect("audit event should insert");

    let audit = store
        .query_audit_events(AuditQueryRequest {
            operation: Some("proposal.reject".to_owned()),
            limit: 5,
        })
        .await
        .expect("audit should query");
    let count = store.audit_event_count().await.expect("audit count");

    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0].actor.as_deref(), Some("reviewer"));
    assert_eq!(count, 1);
}

use super::*;

#[test]
fn jsonl_sink_is_absent_without_entered_runtime() {
    let sink = AgentAuditSink::jsonl(PathBuf::from("/tmp/relay-audit.jsonl"), 1);

    assert!(sink.is_none());
}

#[tokio::test]
async fn jsonl_sink_clamps_configured_queue_depth() {
    let sink = AgentAuditSink::jsonl(PathBuf::from("/tmp/relay-audit.jsonl"), usize::MAX)
        .expect("runtime should create audit sink");

    assert_eq!(sink.sender.max_capacity(), MAX_AUDIT_SINK_QUEUE_DEPTH);
}

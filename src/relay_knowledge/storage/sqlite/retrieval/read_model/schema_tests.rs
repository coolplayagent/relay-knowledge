use super::{
    graph_retrieval_schema_error_is_retryable, graph_retrieval_schema_error_message_is_retryable,
};

#[test]
fn schema_retry_is_limited_to_transient_open_errors() {
    assert!(!graph_retrieval_schema_error_is_retryable(
        &rusqlite::Error::InvalidQuery
    ));
    assert!(graph_retrieval_schema_error_message_is_retryable(
        "vtable constructor failed: graph_bm25"
    ));
    assert!(graph_retrieval_schema_error_message_is_retryable(
        "database schema is locked"
    ));
    assert!(!graph_retrieval_schema_error_message_is_retryable(
        "no such table: graph_bm25"
    ));
}

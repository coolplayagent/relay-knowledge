//! Direct BM25 retry-classification invariants.

use super::{graph_bm25_query_error_is_retryable, graph_bm25_query_error_message_is_retryable};
use crate::storage::StorageError;

#[test]
fn query_retry_is_limited_to_transient_query_errors() {
    assert!(graph_bm25_query_error_message_is_retryable(
        "vtable constructor failed: graph_bm25"
    ));
    assert!(graph_bm25_query_error_message_is_retryable(
        "database table is locked: graph_bm25"
    ));
    assert!(!graph_bm25_query_error_message_is_retryable(
        "no such table: graph_bm25"
    ));
    assert!(!graph_bm25_query_error_is_retryable(
        &StorageError::InvalidInput("database is locked".to_owned())
    ));
}

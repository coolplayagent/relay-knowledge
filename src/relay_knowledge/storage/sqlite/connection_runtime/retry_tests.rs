use super::sqlite_transient_message_is_retryable;

#[test]
fn sqlite_retry_messages_are_limited_to_transient_lock_failures() {
    assert!(sqlite_transient_message_is_retryable(
        "sqlite operation failed: database is locked"
    ));
    assert!(sqlite_transient_message_is_retryable(
        "database schema is locked"
    ));
    assert!(sqlite_transient_message_is_retryable(
        "vtable constructor failed: code_repository_search"
    ));
    assert!(sqlite_transient_message_is_retryable(
        "vtable constructor failed: file_index_search"
    ));
    assert!(!sqlite_transient_message_is_retryable(
        "no such table: code_repository_search"
    ));
}

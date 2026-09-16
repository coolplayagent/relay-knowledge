use super::*;
#[test]
fn source_io_counts_distinct_files_and_directories_and_keeps_legacy_defaults() {
    let connection = Connection::open_in_memory().unwrap();
    connection.execute_batch(r#"CREATE TABLE code_repository_file_diagnostics (source_scope TEXT, path TEXT, io_json TEXT);
        INSERT INTO code_repository_file_diagnostics VALUES ('scope','broken', '{"path_kind":"directory"}');"#).unwrap();
    let directory = measure(&connection, "scope").unwrap();
    assert_eq!(directory.state, CodeContentIntegrityState::Partial);
    assert_eq!(directory.degraded_file_count, Some(0));
    assert_eq!(directory.io_skipped_directory_count, Some(1));
    connection.execute_batch(r#"INSERT INTO code_repository_file_diagnostics VALUES ('scope','a.rs',NULL), ('scope','a.rs',NULL),
        ('scope','b.rs','{"path_kind":"file"}'), ('other','c.rs',NULL);"#).unwrap();
    let mixed = measure(&connection, "scope").unwrap();
    assert_eq!(mixed.degraded_file_count, Some(2));
    assert_eq!(mixed.io_skipped_file_count, Some(1));
    assert_eq!(
        measure(&connection, "empty").unwrap().state,
        CodeContentIntegrityState::Complete
    );
}

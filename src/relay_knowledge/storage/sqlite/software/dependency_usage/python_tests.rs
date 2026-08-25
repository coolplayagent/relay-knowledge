use rusqlite::Connection;

use super::*;

#[test]
fn local_module_inputs_reject_file_cap_plus_one() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_files (
                source_scope TEXT NOT NULL,
                path TEXT NOT NULL,
                language_id TEXT NOT NULL
            );
            INSERT INTO code_repository_files VALUES ('scope', 'a.py', 'python');
            INSERT INTO code_repository_files VALUES ('scope', 'b.py', 'python');
            ",
        )
        .expect("Python file fixture should initialize");

    let error = local_modules(&connection, "scope", 1, 10)
        .expect_err("two files should exceed a one-file cap");

    assert!(matches!(error, StorageError::CapacityExceeded(message)
        if message.contains("Python files")));
}

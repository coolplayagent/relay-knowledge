//! Direct tests for scope-local file metadata loading.

use rusqlite::{Connection, params};

use super::load_file_languages;

#[test]
fn file_language_loading_is_strictly_scoped_and_path_keyed() {
    let mut connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "CREATE TABLE code_repository_files (
                source_scope TEXT NOT NULL,
                path TEXT NOT NULL,
                language_id TEXT NOT NULL
            );",
        )
        .expect("file schema should be created");
    connection
        .execute(
            "INSERT INTO code_repository_files (source_scope, path, language_id)
             VALUES ('scope', 'src/lib.rs', 'rust'), ('other', 'src/app.py', 'python')",
            [],
        )
        .expect("files should be inserted");
    let transaction = connection.transaction().expect("transaction should open");

    let languages = load_file_languages(&transaction, "scope").expect("languages should load");

    assert_eq!(languages.len(), 1);
    assert_eq!(languages.get("src/lib.rs"), Some(&"rust".to_owned()));
    assert_eq!(
        transaction
            .query_row(
                "SELECT COUNT(*) FROM code_repository_files WHERE source_scope = ?1",
                params!["other"],
                |row| row.get::<_, usize>(0),
            )
            .expect("other scope should remain"),
        1
    );
}

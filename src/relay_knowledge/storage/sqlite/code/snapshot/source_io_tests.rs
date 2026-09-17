//! Local failure diagnostics are immutable history and must be reobserved for new scopes.
use super::*;
#[test]
fn source_io_clone_preserves_history_and_drops_repaired_directory_diagnostics() {
    let mut connection = Connection::open_in_memory().unwrap();
    connection.execute_batch("CREATE TABLE code_repository_file_diagnostics(repository_id TEXT, source_scope TEXT, path TEXT, parse_status TEXT, message TEXT, io_json TEXT);
        INSERT INTO code_repository_file_diagnostics VALUES ('r','base','src','failed','blocked','{}'), ('r','base','src/a.rs','partial','syntax',NULL);").unwrap();
    let transaction = connection.transaction().unwrap();
    let table = CODE_SCOPE_TABLES
        .iter()
        .find(|t| t.table == "code_repository_file_diagnostics")
        .unwrap();
    clone_code_table(&transaction, table, "base", "repaired").unwrap();
    let count = |scope| {
        transaction
            .query_row(
                "SELECT count(*) FROM code_repository_file_diagnostics WHERE source_scope=?1",
                [scope],
                |r| r.get::<_, usize>(0),
            )
            .unwrap()
    };
    assert_eq!(count("base"), 2);
    assert_eq!(count("repaired"), 1);
    assert_eq!(
        transaction
            .query_row(
                "SELECT path FROM code_repository_file_diagnostics WHERE source_scope='repaired'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "src/a.rs"
    );
}

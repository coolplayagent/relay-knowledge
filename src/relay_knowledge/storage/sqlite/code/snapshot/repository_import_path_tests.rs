use super::*;

#[test]
fn import_attachment_checks_the_managed_source_before_attaching_it() {
    let mut connection = Connection::open_in_memory().unwrap();
    let source = Path::new("D:/relay-knowledge/users/S-1-invalid/data/relay-knowledge.sqlite");
    assert!(
        import_repository_from_database(&mut connection, source, "repo", None)
            .unwrap_err()
            .to_string()
            .contains("invalid account SID")
    );
    let databases: i64 = connection
        .query_row("SELECT COUNT(*) FROM pragma_database_list", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(databases, 1);
    assert!(!source.exists());
}

use super::super::tests::{database, fixture};
use super::*;

#[test]
fn replay_is_idempotent_and_replacement_removes_deleted_edges() {
    let connection = database();
    let (modules, edges) = fixture();
    persist(&connection, "scope", &modules, &edges).unwrap();
    let count: usize = connection
        .query_row("SELECT COUNT(*) FROM maven_reactor_edges", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(count, 5);
    persist(&connection, "scope", &modules, &[]).unwrap();
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM maven_reactor_edges", [], |row| row
                .get::<_, usize>(
                0
            ))
            .unwrap(),
        0
    );
    persist(&connection, "scope", &[], &[]).unwrap();
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM maven_reactor_modules", [], |row| {
                row.get::<_, usize>(0)
            })
            .unwrap(),
        0
    );
}

#[test]
fn incomplete_projection_and_corrupt_or_oversized_facts_are_rejected() {
    let connection = database();
    connection
        .execute("UPDATE maven_reactor_status SET complete = 0", [])
        .unwrap();
    assert!(require_complete(&connection, "scope").is_err());
    assert!(require_complete(&connection, "unknown").is_ok());
    connection
        .execute(
            "INSERT INTO code_repository_files VALUES ('old', 'pom.xml')",
            [],
        )
        .unwrap();
    assert!(require_complete(&connection, "old").is_err());
    assert!(decode::<String>("not-json".into()).is_err());
    assert!(encode(&"x".repeat(32_769)).is_err());
}

#[test]
fn invalid_or_missing_pom_documents_preserve_graph_until_valid_repair_or_deletion() {
    let connection = database();
    connection.execute_batch("CREATE TABLE code_repository_scopes(source_scope TEXT, language_filters_json TEXT);
        CREATE TABLE code_repository_chunks(repository_id TEXT, source_scope TEXT, chunk_id TEXT, path TEXT, content TEXT, line_start INTEGER);
        INSERT INTO code_repository_files VALUES ('scope','c/pom.xml');").unwrap();
    for content in ["", "   ", "<settings/>", "<project>"] {
        connection
            .execute("DELETE FROM code_repository_chunks", [])
            .unwrap();
        connection.execute("INSERT INTO code_repository_chunks VALUES ('repo','scope','chunk','c/pom.xml',?1,1)", [content]).unwrap();
        refresh(&connection, "scope", GraphVersion::ZERO).unwrap();
        assert!(
            require_complete(&connection, "scope").is_err(),
            "invalid POM: {content:?}"
        );
        let count: usize = connection
            .query_row("SELECT COUNT(*) FROM maven_reactor_modules", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 4);
    }
    connection.execute("UPDATE code_repository_chunks SET content = '<project><groupId>x</groupId><artifactId>c</artifactId><version>1</version></project>'", []).unwrap();
    refresh(&connection, "scope", GraphVersion::ZERO).unwrap();
    require_complete(&connection, "scope").unwrap();
    connection
        .execute("DELETE FROM code_repository_chunks", [])
        .unwrap();
    refresh(&connection, "scope", GraphVersion::ZERO).unwrap();
    assert!(
        require_complete(&connection, "scope").is_err(),
        "an indexed POM without a source chunk is incomplete"
    );
    let count: usize = connection
        .query_row("SELECT COUNT(*) FROM maven_reactor_modules", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(count, 1);
    connection
        .execute("DELETE FROM code_repository_files", [])
        .unwrap();
    refresh(&connection, "scope", GraphVersion::ZERO).unwrap();
    require_complete(&connection, "scope").unwrap();
    let count: usize = connection
        .query_row("SELECT COUNT(*) FROM maven_reactor_modules", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(count, 0, "actual deletion still clears the graph");
    connection.execute_batch("WITH RECURSIVE modules(n) AS (SELECT 0 UNION ALL SELECT n+1 FROM modules WHERE n<8192) INSERT INTO code_repository_files SELECT 'scope', n||'/pom.xml' FROM modules").unwrap();
    assert!(matches!(
        refresh(&connection, "scope", GraphVersion::ZERO),
        Err(StorageError::CapacityExceeded(_))
    ));
}

fn refresh(
    connection: &Connection,
    scope: &str,
    version: GraphVersion,
) -> Result<bool, StorageError> {
    let loaded = super::super::super::effective_models(connection, scope)?;
    super::refresh(connection, scope, version, &loaded)
}

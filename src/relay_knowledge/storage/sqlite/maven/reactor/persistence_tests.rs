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

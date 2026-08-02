//! Direct tests for ordinary reference normalization and resolution.

use rusqlite::{Connection, params};

use super::{normalize_unresolved, resolve};

#[test]
fn reference_resolution_prefers_a_unique_symbol_name() {
    let mut connection = reference_database();
    let transaction = connection.transaction().expect("transaction should open");
    insert_reference(&transaction, "reference:1", "src/use.rs", "Widget");
    insert_symbol(&transaction, "symbol:1", "src/widget.rs", "Widget");

    resolve(&transaction, "scope").expect("reference should resolve");

    let resolution = transaction
        .query_row(
            "
            SELECT target_symbol_snapshot_id, resolution_state, confidence_basis_points
            FROM code_repository_references
            WHERE reference_id = 'reference:1'
            ",
            [],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, u16>(2)?,
                ))
            },
        )
        .expect("resolved reference should remain");
    assert_eq!(
        resolution,
        (Some("symbol:1".to_owned()), "resolved".to_owned(), 8_000)
    );
}

#[test]
fn normalization_clears_stale_targets_before_resolution() {
    let mut connection = reference_database();
    let transaction = connection.transaction().expect("transaction should open");
    insert_reference(&transaction, "reference:1", "src/use.rs", "Missing");
    transaction
        .execute(
            "
            UPDATE code_repository_references
            SET target_symbol_snapshot_id = 'stale',
                target_hint = 'old',
                resolution_state = 'resolved',
                confidence_basis_points = 9000,
                confidence_tier = 'exact'
            ",
            [],
        )
        .expect("stale resolution should be installed");

    normalize_unresolved(&transaction, "scope").expect("reference should be normalized");

    let resolution = transaction
        .query_row(
            "
            SELECT target_symbol_snapshot_id, target_hint, resolution_state,
                   confidence_basis_points, confidence_tier
            FROM code_repository_references
            WHERE reference_id = 'reference:1'
            ",
            [],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, u16>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .expect("normalized reference should remain");
    assert_eq!(
        resolution,
        (
            None,
            Some("Missing".to_owned()),
            "unresolved".to_owned(),
            2_500,
            "ambiguous".to_owned(),
        )
    );
}

fn reference_database() -> Connection {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_references (
                source_scope TEXT NOT NULL,
                reference_id TEXT NOT NULL,
                path TEXT NOT NULL,
                name TEXT NOT NULL,
                kind TEXT NOT NULL,
                target_symbol_snapshot_id TEXT,
                target_hint TEXT,
                resolution_state TEXT NOT NULL,
                confidence_basis_points INTEGER NOT NULL,
                confidence_tier TEXT NOT NULL
            );
            CREATE TABLE code_repository_symbols (
                source_scope TEXT NOT NULL,
                symbol_snapshot_id TEXT NOT NULL,
                path TEXT NOT NULL,
                name TEXT NOT NULL
            );
            ",
        )
        .expect("reference schema should be created");
    connection
}

fn insert_reference(
    transaction: &rusqlite::Transaction<'_>,
    reference_id: &str,
    path: &str,
    name: &str,
) {
    transaction
        .execute(
            "
            INSERT INTO code_repository_references (
                source_scope, reference_id, path, name, kind, target_hint,
                resolution_state, confidence_basis_points, confidence_tier
            )
            VALUES ('scope', ?1, ?2, ?3, 'reference', ?3, 'unresolved', 2500, 'ambiguous')
            ",
            params![reference_id, path, name],
        )
        .expect("reference should be inserted");
}

fn insert_symbol(
    transaction: &rusqlite::Transaction<'_>,
    symbol_snapshot_id: &str,
    path: &str,
    name: &str,
) {
    transaction
        .execute(
            "
            INSERT INTO code_repository_symbols (
                source_scope, symbol_snapshot_id, path, name
            )
            VALUES ('scope', ?1, ?2, ?3)
            ",
            params![symbol_snapshot_id, path, name],
        )
        .expect("symbol should be inserted");
}

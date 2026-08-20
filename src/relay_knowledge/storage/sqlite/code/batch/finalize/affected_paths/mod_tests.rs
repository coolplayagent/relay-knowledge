use super::*;
use rusqlite::Connection;

#[test]
fn full_scope_when_changed_paths_empty() {
    let result = AffectedPaths::full_scope();
    assert!(result.is_full_scope());
    assert!(result.path_refs().is_empty());
}

#[test]
fn path_refs_returns_str_slices() {
    let result = AffectedPaths {
        paths: vec!["a/b.py".to_owned(), "c/d.py".to_owned()],
        fallback_to_full_scope: false,
    };
    assert_eq!(result.path_refs(), vec!["a/b.py", "c/d.py"]);
}

#[test]
fn symbol_cardinality_changes_include_unchanged_reference_paths() {
    let mut connection = Connection::open_in_memory().expect("connection should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_files (source_scope TEXT, path TEXT);
            CREATE TABLE code_repository_symbols (
                source_scope TEXT,
                symbol_snapshot_id TEXT,
                name TEXT
            );
            CREATE TABLE code_repository_references (
                source_scope TEXT,
                path TEXT,
                name TEXT,
                target_symbol_snapshot_id TEXT
            );

            INSERT INTO code_repository_symbols VALUES
                ('base', 'base-shared', 'Shared'),
                ('base', 'base-solo-a', 'Solo'),
                ('base', 'base-solo-b', 'Solo'),
                ('target', 'target-shared-a', 'Shared'),
                ('target', 'target-shared-b', 'Shared'),
                ('target', 'target-solo', 'Solo');

            INSERT INTO code_repository_references VALUES
                ('target', 'src/unique_to_ambiguous.rs', 'Shared', 'target-shared-a'),
                ('target', 'src/ambiguous_to_unique.rs', 'Solo', NULL);

            INSERT INTO code_repository_files VALUES
                ('target', 'src/new.rs'),
                ('target', 'src/unique_to_ambiguous.rs'),
                ('target', 'src/ambiguous_to_unique.rs'),
                ('target', 'src/extra-1.rs'),
                ('target', 'src/extra-2.rs'),
                ('target', 'src/extra-3.rs'),
                ('target', 'src/extra-4.rs'),
                ('target', 'src/extra-5.rs');
            ",
        )
        .expect("fixture schema should persist");
    let transaction = connection.transaction().expect("transaction should start");

    let affected = compute(
        &transaction,
        "target",
        "base",
        &["src/new.rs".to_owned()],
        &[],
    )
    .expect("affected paths should load");

    assert!(!affected.is_full_scope());
    assert_eq!(
        affected.path_refs(),
        vec![
            "src/ambiguous_to_unique.rs",
            "src/new.rs",
            "src/unique_to_ambiguous.rs",
        ]
    );
}

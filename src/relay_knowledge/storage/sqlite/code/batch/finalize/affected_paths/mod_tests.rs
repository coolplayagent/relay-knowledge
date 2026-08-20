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
        finalize_imports_for_full_scope: false,
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
                kind TEXT,
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
                ('target', 'src/unique_to_ambiguous.rs', 'Shared', 'type', 'target-shared-a'),
                ('target', 'src/ambiguous_to_unique.rs', 'Solo', 'type', NULL),
                ('target', 'src/ffi_call.rs', 'ffi::Shared', 'call', NULL);

            INSERT INTO code_repository_files VALUES
                ('base', 'src/new.rs'),
                ('base', 'src/unique_to_ambiguous.rs'),
                ('base', 'src/ambiguous_to_unique.rs'),
                ('base', 'src/ffi_call.rs'),
                ('base', 'src/extra-1.rs'),
                ('base', 'src/extra-2.rs'),
                ('base', 'src/extra-3.rs'),
                ('base', 'src/extra-4.rs'),
                ('base', 'src/extra-5.rs'),
                ('base', 'src/extra-6.rs'),
                ('target', 'src/new.rs'),
                ('target', 'src/unique_to_ambiguous.rs'),
                ('target', 'src/ambiguous_to_unique.rs'),
                ('target', 'src/ffi_call.rs'),
                ('target', 'src/extra-1.rs'),
                ('target', 'src/extra-2.rs'),
                ('target', 'src/extra-3.rs'),
                ('target', 'src/extra-4.rs'),
                ('target', 'src/extra-5.rs'),
                ('target', 'src/extra-6.rs');
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
            "src/ffi_call.rs",
            "src/new.rs",
            "src/unique_to_ambiguous.rs",
        ]
    );
    assert!(!affected.imports_need_full_scope());
}

#[test]
fn module_file_set_changes_require_full_import_finalization() {
    let mut connection = Connection::open_in_memory().expect("connection should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_files (source_scope TEXT, path TEXT);
            CREATE TABLE code_repository_symbols (
                source_scope TEXT, symbol_snapshot_id TEXT, name TEXT
            );
            CREATE TABLE code_repository_references (
                source_scope TEXT, path TEXT, name TEXT, kind TEXT,
                target_symbol_snapshot_id TEXT
            );
            INSERT INTO code_repository_files VALUES
                ('base', 'src/importer.rs'),
                ('target', 'src/importer.rs'),
                ('target', 'src/new_module.rs'),
                ('target', 'src/extra-1.rs'),
                ('target', 'src/extra-2.rs'),
                ('target', 'src/extra-3.rs');
            ",
        )
        .expect("fixture schema should persist");
    let transaction = connection.transaction().expect("transaction should start");

    let affected = compute(
        &transaction,
        "target",
        "base",
        &["src/new_module.rs".to_owned()],
        &[],
    )
    .expect("affected paths should load");

    assert!(!affected.is_full_scope());
    assert!(affected.imports_need_full_scope());
}

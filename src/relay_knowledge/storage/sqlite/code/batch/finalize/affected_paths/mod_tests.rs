use super::*;
use rusqlite::Connection;

#[test]
fn empty_delta_skips_edge_finalization() {
    let result = AffectedPaths::empty();
    assert!(!result.is_full_scope());
    assert!(result.is_empty());
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
            CREATE TABLE code_repository_files (
                source_scope TEXT, path TEXT, language_id TEXT DEFAULT 'rust'
            );
            CREATE TABLE code_repository_symbols (
                source_scope TEXT,
                symbol_snapshot_id TEXT,
                name TEXT,
                kind TEXT,
                signature TEXT,
                path TEXT DEFAULT 'src/default.rs'
            );
            CREATE TABLE code_repository_references (
                source_scope TEXT,
                path TEXT,
                name TEXT,
                kind TEXT,
                target_symbol_snapshot_id TEXT,
                reference_id TEXT
            );
            CREATE TABLE code_repository_imports (
                source_scope TEXT,
                path TEXT,
                module TEXT,
                import_id TEXT
            );
            CREATE VIRTUAL TABLE code_repository_search USING fts5(
                source_scope UNINDEXED, document_kind UNINDEXED,
                record_id UNINDEXED, path UNINDEXED,
                language_id UNINDEXED, content
            );

            INSERT INTO code_repository_symbols
                (source_scope, symbol_snapshot_id, name, kind, signature) VALUES
                ('base', 'base-shared', 'Shared', 'function', 'fn Shared() {}'),
                ('base', 'base-solo-a', 'Solo', 'function', 'fn Solo() {}'),
                ('base', 'base-solo-b', 'Solo', 'function', 'fn Solo(i: i32) {}'),
                ('base', 'base-stable', 'Stable', 'function_declaration', 'fn Stable();'),
                ('base', 'base-moved', 'Moved', 'function', 'fn Moved() {}'),
                ('base', 'base-body-only', 'BodyOnly', 'function', 'fn BodyOnly() {}'),
                ('target', 'target-shared-a', 'Shared', 'function', 'fn Shared() {}'),
                ('target', 'target-shared-b', 'Shared', 'function', 'fn Shared(i: i32) {}'),
                ('target', 'target-solo', 'Solo', 'function', 'fn Solo() {}'),
                ('target', 'target-stable', 'Stable', 'function', 'fn Stable() {}'),
                ('target', 'target-moved', 'Moved', 'function', 'fn Moved() {}');
            INSERT INTO code_repository_symbols
                (source_scope, symbol_snapshot_id, name, kind, signature) VALUES
                ('target', 'target-body-only', 'BodyOnly', 'function', 'fn BodyOnly() {}');

            UPDATE code_repository_symbols SET path = 'src/new.rs'
            WHERE symbol_snapshot_id IN (
                'target-shared-b', 'base-solo-b', 'base-stable', 'target-stable',
                'base-body-only', 'target-body-only'
            );
            UPDATE code_repository_symbols SET path = 'src/move-old.rs'
            WHERE symbol_snapshot_id = 'base-moved';
            UPDATE code_repository_symbols SET path = 'src/move-new.rs'
            WHERE symbol_snapshot_id = 'target-moved';

            INSERT INTO code_repository_references VALUES
                ('target', 'src/unique_to_ambiguous.rs', 'Shared', 'type', 'target-shared-a', 'ref-shared'),
                ('target', 'src/ambiguous_to_unique.rs', 'Solo', 'type', NULL, 'ref-solo'),
                ('target', 'src/ffi_call.rs', 'ffi::Shared', 'call', NULL, 'ref-ffi'),
                ('target', 'src/metadata_call.rs', 'Stable', 'call', 'target-stable', 'ref-stable');
            INSERT INTO code_repository_references VALUES
                ('target', 'src/move-consumer.rs', 'Moved', 'type', 'target-moved', 'ref-moved');
            INSERT INTO code_repository_references VALUES
                ('target', 'src/body-consumer.rs', 'BodyOnly', 'type', 'base-body-only', 'ref-body');

            INSERT INTO code_repository_imports VALUES
                ('target', 'src/aliased_import.ts',
                 'import { Shared as LocalShared } from ''./shared'';', 'import-ts'),
                ('target', 'src/aliased_import.py',
                 'from shared import Shared as LocalShared', 'import-python'),
                ('target', 'src/AliasedImport.java',
                 'import static com.example.Shared.Shared;', 'import-java'),
                ('target', 'src/moved_import.ts',
                 'import { Moved as LocalMoved } from ''./move-new'';', 'import-moved');

            INSERT INTO code_repository_search VALUES
                ('target', 'reference', 'ref-ffi', 'src/ffi_call.rs', 'rust', 'ffi::Shared'),
                ('target', 'import', 'import-ts', 'src/aliased_import.ts', 'typescript',
                 'import Shared LocalShared shared'),
                ('target', 'import', 'import-python', 'src/aliased_import.py', 'python',
                 'from shared import Shared LocalShared'),
                ('target', 'import', 'import-java', 'src/AliasedImport.java', 'java',
                 'import static com example Shared'),
                ('target', 'import', 'import-moved', 'src/moved_import.ts', 'typescript',
                 'import Moved LocalMoved move new');

            INSERT INTO code_repository_files (source_scope, path) VALUES
                ('base', 'src/new.rs'),
                ('base', 'src/unique_to_ambiguous.rs'),
                ('base', 'src/ambiguous_to_unique.rs'),
                ('base', 'src/ffi_call.rs'),
                ('base', 'src/metadata_call.rs'),
                ('base', 'src/aliased_import.ts'),
                ('base', 'src/aliased_import.py'),
                ('base', 'src/AliasedImport.java'),
                ('base', 'src/extra-1.rs'),
                ('base', 'src/extra-2.rs'),
                ('base', 'src/extra-3.rs'),
                ('base', 'src/extra-4.rs'),
                ('base', 'src/extra-5.rs'),
                ('base', 'src/extra-6.rs'),
                ('base', 'src/extra-7.rs'),
                ('base', 'src/extra-8.rs'),
                ('base', 'src/extra-9.rs'),
                ('base', 'src/extra-10.rs'),
                ('base', 'src/extra-11.rs'),
                ('base', 'src/extra-12.rs'),
                ('base', 'src/extra-13.rs'),
                ('base', 'src/extra-14.rs'),
                ('base', 'src/extra-15.rs'),
                ('base', 'src/extra-16.rs'),
                ('base', 'src/move-old.rs'),
                ('base', 'src/move-new.rs'),
                ('base', 'src/move-consumer.rs'),
                ('base', 'src/moved_import.ts'),
                ('base', 'src/body-consumer.rs'),
                ('target', 'src/new.rs'),
                ('target', 'src/unique_to_ambiguous.rs'),
                ('target', 'src/ambiguous_to_unique.rs'),
                ('target', 'src/ffi_call.rs'),
                ('target', 'src/metadata_call.rs'),
                ('target', 'src/aliased_import.ts'),
                ('target', 'src/aliased_import.py'),
                ('target', 'src/AliasedImport.java'),
                ('target', 'src/extra-1.rs'),
                ('target', 'src/extra-2.rs'),
                ('target', 'src/extra-3.rs'),
                ('target', 'src/extra-4.rs'),
                ('target', 'src/extra-5.rs'),
                ('target', 'src/extra-6.rs'),
                ('target', 'src/extra-7.rs'),
                ('target', 'src/extra-8.rs'),
                ('target', 'src/extra-9.rs'),
                ('target', 'src/extra-10.rs'),
                ('target', 'src/extra-11.rs'),
                ('target', 'src/extra-12.rs');
            INSERT INTO code_repository_files (source_scope, path) VALUES
                ('target', 'src/extra-13.rs'),
                ('target', 'src/extra-14.rs'),
                ('target', 'src/extra-15.rs'),
                ('target', 'src/extra-16.rs');
            INSERT INTO code_repository_files (source_scope, path) VALUES
                ('target', 'src/move-old.rs'),
                ('target', 'src/move-new.rs'),
                ('target', 'src/move-consumer.rs');
            INSERT INTO code_repository_files (source_scope, path) VALUES
                ('target', 'src/moved_import.ts');
            INSERT INTO code_repository_files (source_scope, path) VALUES
                ('target', 'src/body-consumer.rs');

            UPDATE code_repository_files SET language_id = 'typescript'
            WHERE path = 'src/aliased_import.ts';
            UPDATE code_repository_files SET language_id = 'python'
            WHERE path = 'src/aliased_import.py';
            UPDATE code_repository_files SET language_id = 'java'
            WHERE path = 'src/AliasedImport.java';
            UPDATE code_repository_files SET language_id = 'typescript'
            WHERE path = 'src/moved_import.ts';
            ",
        )
        .expect("fixture schema should persist");
    let transaction = connection.transaction().expect("transaction should start");

    let affected = compute(
        &transaction,
        "target",
        "base",
        &[
            "src/new.rs".to_owned(),
            "src/move-old.rs".to_owned(),
            "src/move-new.rs".to_owned(),
        ],
        &[],
    )
    .expect("affected paths should load");

    assert!(!affected.is_full_scope());
    assert_eq!(
        affected.path_refs(),
        vec![
            "src/AliasedImport.java",
            "src/aliased_import.py",
            "src/aliased_import.ts",
            "src/ambiguous_to_unique.rs",
            "src/body-consumer.rs",
            "src/ffi_call.rs",
            "src/metadata_call.rs",
            "src/move-consumer.rs",
            "src/move-new.rs",
            "src/move-old.rs",
            "src/moved_import.ts",
            "src/new.rs",
            "src/unique_to_ambiguous.rs",
        ]
    );
}

#[test]
fn module_file_set_changes_require_full_import_finalization() {
    let mut connection = Connection::open_in_memory().expect("connection should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_files (
                source_scope TEXT, path TEXT, language_id TEXT DEFAULT 'rust'
            );
            CREATE TABLE code_repository_symbols (
                source_scope TEXT, symbol_snapshot_id TEXT, name TEXT,
                kind TEXT, signature TEXT, path TEXT
            );
            CREATE TABLE code_repository_references (
                source_scope TEXT, path TEXT, name TEXT, kind TEXT,
                target_symbol_snapshot_id TEXT, reference_id TEXT
            );
            CREATE TABLE code_repository_imports (
                source_scope TEXT, path TEXT, module TEXT, import_id TEXT
            );
            INSERT INTO code_repository_files (source_scope, path) VALUES
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

    assert!(affected.is_full_scope());
}

#[test]
fn saturated_affected_path_discovery_falls_back_to_full_scope() {
    let mut connection = Connection::open_in_memory().expect("connection should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_files (
                source_scope TEXT, path TEXT, language_id TEXT
            );
            CREATE TABLE code_repository_symbols (
                source_scope TEXT, symbol_snapshot_id TEXT, name TEXT,
                kind TEXT, signature TEXT, path TEXT
            );
            CREATE TABLE code_repository_references (
                source_scope TEXT, path TEXT, name TEXT, kind TEXT,
                target_symbol_snapshot_id TEXT, reference_id TEXT
            );
            CREATE TABLE code_repository_imports (
                source_scope TEXT, path TEXT, module TEXT, import_id TEXT
            );
            CREATE VIRTUAL TABLE code_repository_search USING fts5(
                source_scope UNINDEXED, document_kind UNINDEXED,
                record_id UNINDEXED, path UNINDEXED,
                language_id UNINDEXED, content
            );
            WITH RECURSIVE paths(value) AS (
                SELECT 0 UNION ALL SELECT value + 1 FROM paths WHERE value < 1099
            )
            INSERT INTO code_repository_files
            SELECT 'base', printf('src/file-%04d.rs', value), 'rust' FROM paths;
            INSERT INTO code_repository_files
            SELECT 'target', path, language_id
            FROM code_repository_files WHERE source_scope = 'base';
            INSERT INTO code_repository_symbols VALUES
                ('base', 'base-hot', 'Hot', 'function', 'fn Hot() {}', 'src/file-0000.rs'),
                ('target', 'target-hot', 'Hot', 'type', 'struct Hot;', 'src/file-0000.rs');
            WITH RECURSIVE reference_rows(value) AS (
                SELECT 1 UNION ALL SELECT value + 1 FROM reference_rows WHERE value < 513
            )
            INSERT INTO code_repository_references
            SELECT 'target', printf('src/file-%04d.rs', value), 'Hot', 'type',
                   'base-hot', printf('reference-%04d', value)
            FROM reference_rows;
            ",
        )
        .expect("large fixture schema should persist");
    let transaction = connection.transaction().expect("transaction should start");

    let affected = compute(
        &transaction,
        "target",
        "base",
        &["src/file-0000.rs".to_owned()],
        &[],
    )
    .expect("saturated discovery should complete");

    assert!(affected.is_full_scope());
}

#[test]
fn repeated_alias_rows_at_query_cap_fall_back_to_full_scope() {
    let mut connection = Connection::open_in_memory().expect("connection should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_files (
                source_scope TEXT, path TEXT, language_id TEXT
            );
            CREATE TABLE code_repository_symbols (
                source_scope TEXT, symbol_snapshot_id TEXT, name TEXT,
                kind TEXT, signature TEXT, path TEXT
            );
            CREATE TABLE code_repository_references (
                source_scope TEXT, path TEXT, name TEXT, kind TEXT,
                target_symbol_snapshot_id TEXT, reference_id TEXT
            );
            CREATE TABLE code_repository_imports (
                source_scope TEXT, path TEXT, module TEXT, import_id TEXT
            );
            CREATE VIRTUAL TABLE code_repository_search USING fts5(
                source_scope UNINDEXED, document_kind UNINDEXED,
                record_id UNINDEXED, path UNINDEXED,
                language_id UNINDEXED, content
            );
            WITH RECURSIVE paths(value) AS (
                SELECT 0 UNION ALL SELECT value + 1 FROM paths WHERE value < 1099
            )
            INSERT INTO code_repository_files
            SELECT 'base', printf('src/file-%04d.rs', value), 'rust' FROM paths;
            INSERT INTO code_repository_files
            SELECT 'target', path, language_id
            FROM code_repository_files WHERE source_scope = 'base';
            INSERT INTO code_repository_symbols VALUES
                ('base', 'base-hot', 'Hot', 'function', 'fn Hot() {}', 'src/file-0000.rs'),
                ('target', 'target-hot', 'Hot', 'type', 'struct Hot;', 'src/file-0000.rs');
            WITH RECURSIVE rows(value) AS (
                SELECT 1 UNION ALL SELECT value + 1 FROM rows WHERE value < 513
            )
            INSERT INTO code_repository_references
            SELECT 'target', 'src/repeated.rs', 'ns::Hot', 'call', NULL,
                   printf('reference-%04d', value)
            FROM rows;
            WITH RECURSIVE rows(value) AS (
                SELECT 1 UNION ALL SELECT value + 1 FROM rows WHERE value < 513
            )
            INSERT INTO code_repository_search
            SELECT 'target', 'reference', printf('reference-%04d', value),
                   'src/repeated.rs', 'rust', 'ns::Hot'
            FROM rows;
            ",
        )
        .expect("repeated alias fixture should persist");
    let transaction = connection.transaction().expect("transaction should start");

    let affected = compute(
        &transaction,
        "target",
        "base",
        &["src/file-0000.rs".to_owned()],
        &[],
    )
    .expect("raw-row saturation should complete");

    assert!(affected.is_full_scope());
}

#[test]
fn repeated_named_import_rows_report_query_saturation() {
    let mut connection = Connection::open_in_memory().expect("connection should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_files (
                source_scope TEXT, path TEXT, language_id TEXT
            );
            CREATE TABLE code_repository_imports (
                source_scope TEXT, path TEXT, module TEXT, import_id TEXT
            );
            CREATE VIRTUAL TABLE code_repository_search USING fts5(
                source_scope UNINDEXED, document_kind UNINDEXED,
                record_id UNINDEXED, path UNINDEXED,
                language_id UNINDEXED, content
            );
            INSERT INTO code_repository_files VALUES
                ('target', 'src/repeated.ts', 'typescript');
            WITH RECURSIVE rows(value) AS (
                SELECT 1 UNION ALL SELECT value + 1 FROM rows WHERE value < 513
            )
            INSERT INTO code_repository_imports
            SELECT 'target', 'src/repeated.ts',
                   'import { Hot as LocalHot } from ''./hot'';',
                   printf('import-%04d', value)
            FROM rows;
            WITH RECURSIVE rows(value) AS (
                SELECT 1 UNION ALL SELECT value + 1 FROM rows WHERE value < 513
            )
            INSERT INTO code_repository_search
            SELECT 'target', 'import', printf('import-%04d', value),
                   'src/repeated.ts', 'typescript', 'import Hot LocalHot hot'
            FROM rows;
            ",
        )
        .expect("repeated import fixture should persist");
    let transaction = connection.transaction().expect("transaction should start");

    let discovery = load_named_import_affected_paths(
        &transaction,
        "target",
        &BTreeSet::from(["Hot".to_owned()]),
    )
    .expect("named import saturation should complete");

    assert!(discovery.saturated);
    assert_eq!(discovery.paths, vec!["src/repeated.ts"]);
}

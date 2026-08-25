use super::super::repository_schema::initialize_repository_schema;
use super::*;

const LEGACY_V1_QUERY_INDEX_IDENTITIES: [(&str, &str, &[&str]); 16] = [
    (
        "code_repository_search_metadata_scope_path",
        "code_repository_search_metadata",
        &["source_scope", "path"],
    ),
    (
        "code_repository_symbols_lookup",
        "code_repository_symbols",
        &["source_scope", "name", "qualified_name", "path"],
    ),
    (
        "code_repository_symbols_name_path_lookup",
        "code_repository_symbols",
        &["source_scope", "name", "path"],
    ),
    (
        "code_repository_symbols_path_line_lookup",
        "code_repository_symbols",
        &["source_scope", "path", "line_end", "line_start"],
    ),
    (
        "code_repository_references_lookup",
        "code_repository_references",
        &["source_scope", "name", "kind", "path"],
    ),
    (
        "code_repository_calls_lookup",
        "code_repository_calls",
        &["source_scope", "callee_name", "caller_name", "path"],
    ),
    (
        "code_repository_feature_flags_lookup",
        "code_repository_feature_flags",
        &["source_scope", "name", "source_key", "edge_kind", "path"],
    ),
    (
        "code_repository_routes_lookup",
        "code_repository_routes",
        &["source_scope", "url", "http_method", "path"],
    ),
    (
        "code_repository_routes_handler_lookup",
        "code_repository_routes",
        &["source_scope", "handler_symbol_snapshot_id", "path"],
    ),
    (
        "code_repository_imports_lookup",
        "code_repository_imports",
        &["source_scope", "module", "path"],
    ),
    (
        "code_repository_imports_target_lookup",
        "code_repository_imports",
        &["source_scope", "target_hint", "path"],
    ),
    (
        "code_repository_dependencies_lookup",
        "code_repository_dependencies",
        &["source_scope", "ecosystem", "package_name", "path"],
    ),
    (
        "code_repository_dependencies_group_lookup",
        "code_repository_dependencies",
        &["source_scope", "dependency_group", "path"],
    ),
    (
        "code_repository_chunks_lookup",
        "code_repository_chunks",
        &["source_scope", "path"],
    ),
    (
        "code_repository_chunks_symbol_lookup",
        "code_repository_chunks",
        &["source_scope", "symbol_snapshot_id"],
    ),
    (
        "code_repository_calls_caller_lookup",
        "code_repository_calls",
        &["source_scope", "caller_name", "path", "line_start"],
    ),
];

fn install_legacy_v1_query_indexes(connection: &Connection) {
    for (position, (name, table, columns)) in LEGACY_V1_QUERY_INDEX_IDENTITIES.iter().enumerate() {
        let descriptor = &SEARCH_QUERY_INDEXES[position];
        assert_eq!(descriptor.name, *name, "v1 descriptor {position} name");
        assert_eq!(descriptor.table, *table, "v1 descriptor {position} owner");
        assert_eq!(
            descriptor.columns, *columns,
            "v1 descriptor {position} columns"
        );
        connection
            .execute(descriptor.sql, [])
            .expect("version-one descriptor should persist");
    }
}

#[test]
fn creates_search_read_model_and_defers_fact_indexes_until_requested() {
    let connection = Connection::open_in_memory().expect("database should open");
    initialize_repository_schema(&connection).expect("repository schema should initialize");

    initialize_search_schema(&connection).expect("search schema should initialize");
    initialize_search_schema(&connection).expect("search schema should be idempotent");

    let deferred_index_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_schema
             WHERE type = 'index' AND name = 'code_repository_symbols_lookup'",
            [],
            |row| row.get(0),
        )
        .expect("deferred index state should be inspectable");
    assert_eq!(deferred_index_count, 0);
    ensure_search_query_indexes(&connection).expect("query indexes should build");
    ensure_search_query_indexes(&connection).expect("query index build should be idempotent");
    assert!(
        persisted_query_index_columns(&connection, &SEARCH_QUERY_INDEXES[1])
            .expect("retired query-index state should load")
            .is_none()
    );
    require_persisted_query_index(&connection, &SEARCH_QUERY_INDEXES[2])
        .expect("the replacement name/path lookup should persist");

    connection
        .execute(
            "
            INSERT INTO code_repository_search (
                source_scope, document_kind, record_id, path, language_id, content
            )
            VALUES ('scope', 'symbol', 'symbol-1', 'src/lib.rs', 'rust', 'SearchableThing')
            ",
            [],
        )
        .expect("search row should insert");
    let match_count: i64 = connection
        .query_row(
            "
            SELECT COUNT(*)
            FROM code_repository_search
            WHERE code_repository_search MATCH 'SearchableThing'
            ",
            [],
            |row| row.get(0),
        )
        .expect("FTS row should be searchable");
    assert_eq!(match_count, 1);

    let metadata_index_count: i64 = connection
        .query_row(
            "
            SELECT COUNT(*)
            FROM sqlite_schema
            WHERE type = 'index'
              AND name = 'code_repository_search_metadata_scope_path'
            ",
            [],
            |row| row.get(0),
        )
        .expect("search metadata indexes should be inspectable");
    assert_eq!(metadata_index_count, 1);
    let redundant_scope_kind_index: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'index' AND name = 'code_repository_search_metadata_scope_kind'",
            [],
            |row| row.get(0),
        )
        .expect("redundant metadata index should be inspectable");
    assert_eq!(redundant_scope_kind_index, 0);
    let search_rowid_primary_key: i64 = connection
        .query_row(
            "SELECT pk FROM pragma_table_info('code_repository_search_metadata') WHERE name = 'search_rowid'",
            [],
            |row| row.get(0),
        )
        .expect("metadata rowid primary key should be inspectable");
    assert_eq!(search_rowid_primary_key, 1);
}

#[test]
fn startup_validation_never_creates_missing_query_indexes() {
    let connection = Connection::open_in_memory().expect("database should open");
    initialize_repository_schema(&connection).expect("repository schema should initialize");
    initialize_search_schema(&connection).expect("search schema should initialize");
    connection
        .execute(
            "INSERT INTO code_repository_search_metadata (
                source_scope, document_kind, record_id, path, search_rowid
             ) VALUES ('scope', 'symbol', 'symbol-1', 'src/lib.rs', 1)",
            [],
        )
        .expect("search metadata fact should insert");

    validate_existing_query_indexes(&connection)
        .expect("startup should validate existing query indexes");

    let populated_table_index_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_schema
             WHERE type = 'index' AND name = 'code_repository_search_metadata_scope_path'",
            [],
            |row| row.get(0),
        )
        .expect("populated-table index state should be inspectable");
    assert_eq!(populated_table_index_count, 0);
    let empty_table_index_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_schema
             WHERE type = 'index' AND name = 'code_repository_symbols_lookup'",
            [],
            |row| row.get(0),
        )
        .expect("empty-table index state should be inspectable");
    assert_eq!(empty_table_index_count, 0);
}

#[test]
fn empty_owner_prepare_creates_the_complete_stable_query_index_plan() {
    let connection = Connection::open_in_memory().expect("database should open");
    initialize_repository_schema(&connection).expect("repository schema should initialize");
    initialize_search_schema(&connection).expect("search schema should initialize");

    prepare_query_indexes_for_empty_owners(&connection)
        .expect("empty target tables should receive all query indexes");

    for descriptor in SEARCH_QUERY_INDEXES {
        if descriptor.mode == SearchQueryIndexMode::Retired {
            assert!(
                persisted_query_index_columns(&connection, descriptor)
                    .expect("retired descriptor state should load")
                    .is_none(),
                "retired descriptor {} must remain a stable skip",
                descriptor.name
            );
            continue;
        }
        if query_index_descriptor_is_applicable(&connection, descriptor)
            .expect("descriptor applicability should load")
        {
            require_persisted_query_index(&connection, descriptor)
                .expect("empty-owner prepare should persist every applicable descriptor");
        }
    }
}

#[test]
fn code_index_task_retired_symbol_lookup_is_never_created_or_dropped_but_keeps_exact_shape_validation()
 {
    let connection = Connection::open_in_memory().expect("database should open");
    initialize_repository_schema(&connection).expect("repository schema should initialize");
    initialize_search_schema(&connection).expect("search schema should initialize");
    let retired = &SEARCH_QUERY_INDEXES[1];
    assert_eq!(retired.mode, SearchQueryIndexMode::Retired);

    prepare_query_indexes_for_empty_owners(&connection)
        .expect("active empty-owner indexes should prepare");
    assert!(
        persisted_query_index_columns(&connection, retired)
            .expect("retired index state should load")
            .is_none()
    );

    connection
        .execute(retired.sql, [])
        .expect("an exact legacy retired index should install");
    validate_existing_query_indexes(&connection)
        .expect("an exact legacy retired index should validate");
    prepare_query_indexes_for_empty_owners(&connection)
        .expect("preparation should retain an exact legacy retired index");
    assert_eq!(
        persisted_query_index_columns(&connection, retired)
            .expect("retained legacy index shape should load")
            .expect("the legacy index must not be dropped"),
        ["source_scope", "name", "qualified_name", "path"]
    );
}

#[test]
fn code_index_task_retired_symbol_lookup_shape_collision_fails_startup_validation_closed() {
    let connection = Connection::open_in_memory().expect("database should open");
    initialize_repository_schema(&connection).expect("repository schema should initialize");
    initialize_search_schema(&connection).expect("search schema should initialize");
    connection
        .execute(
            "CREATE INDEX code_repository_symbols_lookup
             ON code_repository_symbols(source_scope, qualified_name, name, path)",
            [],
        )
        .expect("an incompatible retired-name collision should install");

    let error = validate_existing_query_indexes(&connection)
        .expect_err("a retired-name collision must remain fail-closed");

    assert!(matches!(error, StorageError::Invariant(_)));
}

#[test]
fn code_index_task_restart_prepares_only_chunk_indexes_for_an_empty_owner() {
    let connection = Connection::open_in_memory().expect("database should open");
    initialize_repository_schema(&connection).expect("repository schema should initialize");
    initialize_search_schema(&connection).expect("search schema should initialize");

    prepare_restart_query_indexes(&connection)
        .expect("an empty chunk owner should receive its two lookups");

    for (position, descriptor) in SEARCH_QUERY_INDEXES.iter().enumerate() {
        let persisted = persisted_query_index_columns(&connection, descriptor)
            .expect("query-index state should load");
        if matches!(position, 13 | 14) {
            require_query_index_columns(descriptor, persisted)
                .expect("both chunk descriptors should persist exactly");
        } else {
            assert!(
                persisted.is_none(),
                "restart must defer unit {position} ({})",
                descriptor.name
            );
        }
    }

    let populated = Connection::open_in_memory().expect("database should open");
    initialize_repository_schema(&populated).expect("repository schema should initialize");
    initialize_search_schema(&populated).expect("search schema should initialize");
    populated
        .execute_batch(
            "INSERT INTO code_repositories (
                repository_id, alias, root_path, path_filters_json, language_filters_json,
                state, indexed_file_count, symbol_count, reference_count, chunk_count, stale
             ) VALUES ('repo', 'repo', '/tmp/repo', '[]', '[]', 'fresh', 0, 0, 0, 1, 0);
             INSERT INTO code_repository_chunks (
                repository_id, source_scope, chunk_id, file_id, path, language_id, content,
                byte_start, byte_end, line_start, line_end, symbol_snapshot_id
             ) VALUES (
                'repo', 'scope', 'chunk', 'file', 'src/lib.rs', 'rust', 'content',
                0, 7, 1, 1, NULL
             );",
        )
        .expect("a populated chunk owner should be constructible");

    prepare_restart_query_indexes(&populated)
        .expect("a populated chunk owner should remain deferred");
    for position in [13, 14] {
        assert!(
            persisted_query_index_columns(&populated, &SEARCH_QUERY_INDEXES[position])
                .expect("chunk index state should load")
                .is_none()
        );
    }
}

#[test]
fn code_index_task_identity_scoped_identity_and_resolution_group_use_the_name_path_lookup() {
    let connection = Connection::open_in_memory().expect("database should open");
    initialize_repository_schema(&connection).expect("repository schema should initialize");
    initialize_search_schema(&connection).expect("search schema should initialize");
    prepare_query_indexes_for_empty_owners(&connection)
        .expect("active query indexes should prepare");

    for sql in [
        "EXPLAIN QUERY PLAN
         SELECT 1 FROM code_repository_symbols
         WHERE source_scope = 'scope' AND name = 'Thing'
         LIMIT 1",
        "EXPLAIN QUERY PLAN
         SELECT symbol_snapshot_id FROM code_repository_symbols
         WHERE source_scope = 'scope' AND name = 'Thing'
           AND lower(qualified_name) LIKE '%thing%' ESCAPE '\\'
         ORDER BY path, line_start
         LIMIT 32",
        "EXPLAIN QUERY PLAN
         WITH reference_names(name) AS (VALUES ('Thing'))
         SELECT name, MIN(symbol_snapshot_id)
         FROM code_repository_symbols
         WHERE source_scope = 'scope'
           AND name IN (SELECT name FROM reference_names)
         GROUP BY name
         HAVING COUNT(*) = 1",
    ] {
        let plan = connection
            .prepare(sql)
            .expect("query plan should prepare")
            .query_map([], |row| row.get::<_, String>(3))
            .expect("query plan should execute")
            .collect::<Result<Vec<_>, _>>()
            .expect("query plan should collect");
        assert!(
            plan.iter()
                .any(|detail| detail.contains("code_repository_symbols_name_path_lookup")),
            "{plan:?}"
        );
    }

    assert!(
        persisted_query_index_columns(&connection, &SEARCH_QUERY_INDEXES[1])
            .expect("retired query-index state should load")
            .is_none()
    );
}

#[test]
fn import_scope_path_line_index_has_exact_startup_shape_and_supports_path_seek() {
    let connection = Connection::open_in_memory().expect("database should open");
    initialize_repository_schema(&connection).expect("repository schema should initialize");
    initialize_search_schema(&connection).expect("search schema should initialize");

    prepare_query_indexes_for_empty_owners(&connection)
        .expect("fresh target tables should receive all query indexes");

    let descriptor = SEARCH_QUERY_INDEXES
        .last()
        .expect("the query-index plan should not be empty");
    assert_eq!(
        descriptor.name,
        "code_repository_imports_scope_path_line_lookup"
    );
    assert_eq!(
        persisted_query_index_columns(&connection, descriptor)
            .expect("persisted import index shape should load")
            .expect("import index should exist"),
        ["source_scope", "path", "line_start", "line_end"]
    );

    let plan = connection
        .prepare(
            "EXPLAIN QUERY PLAN
             SELECT module
             FROM code_repository_imports
             WHERE source_scope = ?1 AND path = ?2
             ORDER BY path ASC, line_start ASC",
        )
        .expect("exact-path query plan should prepare")
        .query_map(["scope", "src/provider.ts"], |row| row.get::<_, String>(3))
        .expect("exact-path query plan should execute")
        .collect::<Result<Vec<_>, _>>()
        .expect("exact-path query plan should collect");

    assert!(
        plan.iter().any(|detail| {
            detail.contains("code_repository_imports_scope_path_line_lookup")
                && detail.contains("source_scope=? AND path=?")
        }),
        "{plan:?}"
    );
}

#[test]
fn version_one_finalization_cursor_builds_the_appended_import_index() {
    let connection = Connection::open_in_memory().expect("database should open");
    initialize_repository_schema(&connection).expect("repository schema should initialize");
    initialize_search_schema(&connection).expect("search schema should initialize");
    install_legacy_v1_query_indexes(&connection);

    let advance = advance_search_query_indexes(&connection, Some(15), true)
        .expect("a version-one finalization cursor should build the appended unit");

    assert_eq!(
        advance,
        SearchQueryIndexAdvance::Created {
            completed_unit: 16,
            plan_complete: true,
        }
    );
    assert_eq!(
        persisted_query_index_columns(
            &connection,
            SEARCH_QUERY_INDEXES
                .last()
                .expect("the appended descriptor should exist"),
        )
        .expect("appended import index shape should load")
        .expect("appended import index should persist"),
        ["source_scope", "path", "line_start", "line_end"]
    );
    require_persisted_query_index(&connection, &SEARCH_QUERY_INDEXES[1])
        .expect("a version-one completed prefix must retain its unit-one proof");
}

#[test]
fn code_index_task_version_one_and_two_cursors_never_skip_a_missing_retired_unit() {
    let connection = Connection::open_in_memory().expect("database should open");
    initialize_repository_schema(&connection).expect("repository schema should initialize");
    initialize_search_schema(&connection).expect("search schema should initialize");
    assert!(matches!(
        advance_search_query_indexes(&connection, None, false)
            .expect("current finalization should create unit zero"),
        SearchQueryIndexAdvance::Created {
            completed_unit: 0,
            ..
        }
    ));
    for state in [
        "finalizing:build_query_indexes:v1:0",
        "finalizing:build_query_indexes:v1:1",
        "finalizing:build_query_indexes:v2:0",
        "finalizing:build_query_indexes:v2:1",
    ] {
        let cursor = crate::domain::code_query_index_subphase(state)
            .expect("canonical legacy cursor should parse");
        let error = advance_search_query_indexes(
            &connection,
            Some(cursor.completed_unit),
            cursor.requires_legacy_retired_prefix(),
        )
        .expect_err("a legacy cursor must retain an exact physical unit one");
        assert!(matches!(error, StorageError::Invariant(_)), "state={state}");
    }
    assert!(
        persisted_query_index_columns(&connection, &SEARCH_QUERY_INDEXES[2])
            .expect("next active unit state should load")
            .is_none()
    );
}

#[test]
fn code_index_task_version_three_cursor_stably_skips_a_missing_retired_unit() {
    let connection = Connection::open_in_memory().expect("database should open");
    initialize_repository_schema(&connection).expect("repository schema should initialize");
    initialize_search_schema(&connection).expect("search schema should initialize");
    assert!(matches!(
        advance_search_query_indexes(&connection, None, false)
            .expect("current finalization should create unit zero"),
        SearchQueryIndexAdvance::Created {
            completed_unit: 0,
            ..
        }
    ));
    let cursor = crate::domain::code_query_index_subphase("finalizing:build_query_indexes:v3:1")
        .expect("canonical version-three cursor should parse");

    let advance = advance_search_query_indexes(
        &connection,
        Some(cursor.completed_unit),
        cursor.requires_legacy_retired_prefix(),
    )
    .expect("current cursor should accept the retired stable skip");

    assert!(matches!(
        advance,
        SearchQueryIndexAdvance::Created {
            completed_unit: 2,
            ..
        }
    ));
    assert!(
        persisted_query_index_columns(&connection, &SEARCH_QUERY_INDEXES[1])
            .expect("retired unit state should load")
            .is_none()
    );
}

#[test]
fn code_index_task_versioned_repair_tokens_keep_their_retired_prefix_policy() {
    let legacy = Connection::open_in_memory().expect("database should open");
    initialize_repository_schema(&legacy).expect("repository schema should initialize");
    initialize_search_schema(&legacy).expect("search schema should initialize");
    advance_search_query_indexes(&legacy, None, false)
        .expect("current finalization should create unit zero");
    let legacy_repair =
        crate::domain::code_query_index_repair("finalizing:query_index_repair:v2:1:resume:0")
            .expect("canonical version-two repair should parse");
    let error = advance_search_query_index_repair(
        &legacy,
        Some(legacy_repair.completed_unit),
        legacy_repair.requires_legacy_retired_prefix(),
    )
    .expect_err("version-two repair must require its completed retired unit");
    assert!(matches!(error, StorageError::Invariant(_)));

    let current = Connection::open_in_memory().expect("database should open");
    initialize_repository_schema(&current).expect("repository schema should initialize");
    initialize_search_schema(&current).expect("search schema should initialize");
    advance_search_query_indexes(&current, None, false)
        .expect("current finalization should create unit zero");
    let current_repair =
        crate::domain::code_query_index_repair("finalizing:query_index_repair:v3:1:resume:0")
            .expect("canonical version-three repair should parse");
    let advance = advance_search_query_index_repair(
        &current,
        Some(current_repair.completed_unit),
        current_repair.requires_legacy_retired_prefix(),
    )
    .expect("version-three repair should accept the retired stable skip");
    assert!(matches!(
        advance,
        SearchQueryIndexAdvance::Created {
            completed_unit: 2,
            ..
        }
    ));
}

#[test]
fn version_one_cursor_fails_if_its_completed_prefix_becomes_inapplicable() {
    let connection = Connection::open_in_memory().expect("database should open");
    initialize_repository_schema(&connection).expect("repository schema should initialize");
    initialize_search_schema(&connection).expect("search schema should initialize");
    install_legacy_v1_query_indexes(&connection);
    connection
        .execute("DROP INDEX code_repository_calls_caller_lookup", [])
        .expect("legacy caller index should drop");
    connection
        .execute(
            "ALTER TABLE code_repository_calls DROP COLUMN line_start",
            [],
        )
        .expect("legacy owner shape should become inapplicable");

    let error = advance_search_query_indexes(&connection, Some(15), true)
        .expect_err("an ordinary v1 cursor must keep its completed-prefix proof strict");

    assert!(matches!(error, StorageError::Invariant(_)));
    assert!(
        persisted_query_index_columns(
            &connection,
            SEARCH_QUERY_INDEXES
                .last()
                .expect("appended descriptor should exist"),
        )
        .expect("appended index state should load")
        .is_none()
    );
}

#[test]
fn appended_import_index_shape_collision_fails_finalization_closed() {
    let connection = Connection::open_in_memory().expect("database should open");
    initialize_repository_schema(&connection).expect("repository schema should initialize");
    initialize_search_schema(&connection).expect("search schema should initialize");
    install_legacy_v1_query_indexes(&connection);
    connection
        .execute(
            "CREATE INDEX code_repository_imports_scope_path_line_lookup
             ON code_repository_imports(source_scope, path, line_end, line_start)",
            [],
        )
        .expect("an incompatible appended index should be constructible");

    let error = advance_search_query_indexes(&connection, Some(15), true)
        .expect_err("an incompatible appended index must not complete finalization");

    assert!(matches!(error, StorageError::Invariant(_)));
}

#[test]
fn startup_validation_rejects_existing_query_index_shape_collisions() {
    for invalid_sql in [
        "CREATE INDEX code_repository_search_metadata_scope_path ON code_repository_symbols(source_scope, path)",
        "CREATE INDEX code_repository_search_metadata_scope_path ON code_repository_search_metadata(path, source_scope)",
        "CREATE INDEX code_repository_search_metadata_scope_path ON code_repository_search_metadata(source_scope, path) WHERE path <> ''",
        "CREATE UNIQUE INDEX code_repository_search_metadata_scope_path ON code_repository_search_metadata(source_scope, path)",
        "CREATE INDEX code_repository_search_metadata_scope_path ON code_repository_search_metadata(source_scope COLLATE NOCASE, path)",
        "CREATE INDEX code_repository_search_metadata_scope_path ON code_repository_search_metadata(source_scope DESC, path)",
    ] {
        let connection = Connection::open_in_memory().expect("database should open");
        initialize_repository_schema(&connection).expect("repository schema should initialize");
        initialize_search_schema(&connection).expect("search schema should initialize");
        connection
            .execute(invalid_sql, [])
            .expect("invalid collision should be constructible");

        let error = validate_existing_query_indexes(&connection)
            .expect_err("startup must reject an incompatible existing index");

        assert!(matches!(error, StorageError::Invariant(_)));
    }
}

#[test]
fn one_step_index_creation_is_atomic_and_resumes_from_its_unit() {
    let mut connection = Connection::open_in_memory().expect("database should open");
    initialize_repository_schema(&connection).expect("repository schema should initialize");
    initialize_search_schema(&connection).expect("search schema should initialize");

    let transaction = connection.transaction().expect("transaction should open");
    let rolled_back = advance_search_query_indexes(&transaction, None, false)
        .expect("one descriptor should build inside the transaction");
    assert_eq!(
        rolled_back,
        SearchQueryIndexAdvance::Created {
            completed_unit: 0,
            plan_complete: false,
        }
    );
    transaction
        .rollback()
        .expect("simulated crash should roll back");
    assert!(
        persisted_query_index_columns(&connection, &SEARCH_QUERY_INDEXES[0])
            .expect("index state should load")
            .is_none()
    );

    let transaction = connection.transaction().expect("transaction should reopen");
    let committed = advance_search_query_indexes(&transaction, None, false)
        .expect("first descriptor should rebuild after rollback");
    transaction.commit().expect("descriptor should commit");
    let SearchQueryIndexAdvance::Created { completed_unit, .. } = committed else {
        panic!("first durable descriptor should report progress");
    };
    let resumed = advance_search_query_indexes(&connection, Some(completed_unit), false)
        .expect("the next call should resume after the durable unit");
    assert!(matches!(
        resumed,
        SearchQueryIndexAdvance::Created {
            completed_unit: 2,
            ..
        }
    ));
    assert!(
        persisted_query_index_columns(&connection, &SEARCH_QUERY_INDEXES[1])
            .expect("retired query-index state should load")
            .is_none()
    );
}

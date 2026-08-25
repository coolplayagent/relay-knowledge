//! Direct contracts for code search-document content.

use std::sync::Mutex;

use rusqlite::{Connection, limits::Limit, params};

use super::{
    EXACT_SEARCH_OWNER_PREDICATE_SQL, SEARCH_DOCUMENT_INSERT_BATCH_SIZE,
    SEARCH_DOCUMENT_MAX_ROWID_EXISTS_SQL, SearchDocumentInserter, search_document_content,
    search_document_content_into,
};

static SEARCH_SQL_TRACE: Mutex<Vec<String>> = Mutex::new(Vec::new());

#[test]
fn symbol_search_content_preserves_identifier_expansion() {
    let content = search_document_content(
        "symbol",
        [
            "NewLRUCache",
            "",
            "leveldb::NewLRUCache",
            "function",
            "db/cache.cc",
        ],
    );

    assert_eq!(
        content,
        "NewLRUCache leveldb::NewLRUCache function db/cache.cc cache leveldb lru new newlrucache"
    );
}

#[test]
fn route_search_content_expands_handler_identifier_terms() {
    let content = search_document_content(
        "route",
        [
            "route endpoint http",
            "/api/users",
            "get",
            "listUsers",
            "express",
            "src/routes.ts",
        ],
    );

    assert_eq!(
        content,
        "route endpoint http /api/users get listUsers express src/routes.ts list listusers users"
    );
}

#[test]
fn non_symbol_search_content_keeps_only_nonempty_fields() {
    let content = search_document_content("chunk", ["", "body text", "  ", "src/lib.rs"]);

    assert_eq!(content, "body text src/lib.rs");
}

#[test]
fn reusable_search_content_buffers_do_not_leak_previous_terms() {
    let mut content = String::from("stale content");
    let mut symbol_terms = vec!["stale".to_owned()];
    search_document_content_into(
        &mut content,
        &mut symbol_terms,
        "symbol",
        ["GraphIndex", "relay_knowledge::GraphIndex"],
    );
    assert_eq!(
        content,
        "GraphIndex relay_knowledge::GraphIndex graph graphindex index knowledge relay relay_knowledge"
    );

    search_document_content_into(&mut content, &mut symbol_terms, "chunk", ["new chunk"]);
    assert_eq!(content, "new chunk");
    assert!(symbol_terms.is_empty());
}

#[test]
fn buffered_search_inserts_keep_fts_and_metadata_in_lockstep() {
    let mut connection = Connection::open_in_memory().expect("database should open");
    super::super::schema::initialize_code_schema(&connection)
        .expect("code schema should initialize");
    let transaction = connection.transaction().expect("transaction should start");
    let mut inserter = SearchDocumentInserter::new(&transaction).expect("inserter should build");
    for index in 0..=SEARCH_DOCUMENT_INSERT_BATCH_SIZE {
        let record_id = format!("symbol-{index}");
        let symbol = format!("GraphIndex{index}");
        inserter
            .insert(
                "scope",
                "symbol",
                &record_id,
                "src/lib.rs",
                "rust",
                [symbol.as_str(), "relay_knowledge::GraphIndex"],
            )
            .expect("search document should buffer");
    }
    inserter.finish().expect("remaining documents should flush");
    transaction.commit().expect("transaction should commit");

    let expected = (SEARCH_DOCUMENT_INSERT_BATCH_SIZE + 1) as i64;
    let search_count: i64 = connection
        .query_row(
            "SELECT count(*) FROM code_repository_search WHERE source_scope = 'scope'",
            [],
            |row| row.get(0),
        )
        .expect("search rows should count");
    let metadata_count: i64 = connection
        .query_row(
            "SELECT count(*) FROM code_repository_search_metadata WHERE source_scope = 'scope'",
            [],
            |row| row.get(0),
        )
        .expect("metadata rows should count");
    let matched: i64 = connection
        .query_row(
            "SELECT count(*) FROM code_repository_search WHERE code_repository_search MATCH ?1",
            params!["graph"],
            |row| row.get(0),
        )
        .expect("expanded identifier terms should remain searchable");

    assert_eq!(search_count, expected);
    assert_eq!(metadata_count, expected);
    assert_eq!(matched, expected);
}

#[test]
fn search_document_groups_follow_the_runtime_variable_limit_and_fail_below_one_row() {
    let mut grouped = Connection::open_in_memory().expect("grouped database should open");
    super::super::schema::initialize_code_schema(&grouped)
        .expect("grouped schema should initialize");
    grouped.set_limit(Limit::SQLITE_LIMIT_VARIABLE_NUMBER, 12);
    let transaction = grouped
        .transaction()
        .expect("grouped transaction should start");
    let mut inserter = SearchDocumentInserter::new(&transaction)
        .expect("twelve variables should admit two search documents");
    assert_eq!(inserter.document_batch_size, 2);
    for index in 0..3 {
        inserter
            .insert(
                "scope",
                "chunk",
                &format!("chunk-{index}"),
                "src/lib.rs",
                "rust",
                ["bounded content"],
            )
            .expect("grouped document should insert");
    }
    inserter.finish().expect("grouped tail should flush");
    transaction
        .commit()
        .expect("grouped transaction should commit");
    assert_eq!(
        grouped
            .query_row("SELECT COUNT(*) FROM code_repository_search", [], |row| {
                row.get::<_, usize>(0)
            })
            .expect("grouped rows should count"),
        3
    );

    let mut exact = Connection::open_in_memory().expect("exact database should open");
    super::super::schema::initialize_code_schema(&exact).expect("exact schema should initialize");
    exact.set_limit(Limit::SQLITE_LIMIT_VARIABLE_NUMBER, 6);
    let transaction = exact.transaction().expect("exact transaction should start");
    let inserter = SearchDocumentInserter::new(&transaction)
        .expect("six variables should admit one search document");
    assert_eq!(inserter.document_batch_size, 1);
    transaction
        .rollback()
        .expect("exact transaction should roll back");

    let mut short = Connection::open_in_memory().expect("short database should open");
    super::super::schema::initialize_code_schema(&short).expect("short schema should initialize");
    short.set_limit(Limit::SQLITE_LIMIT_VARIABLE_NUMBER, 5);
    let transaction = short.transaction().expect("short transaction should start");
    let error = match SearchDocumentInserter::new(&transaction) {
        Ok(_) => panic!("five variables must not admit one search document"),
        Err(error) => error,
    };
    assert!(
        error
            .to_string()
            .contains("cannot admit one 6-column search-document row")
    );
    assert_eq!(
        transaction
            .query_row("SELECT COUNT(*) FROM code_repository_search", [], |row| {
                row.get::<_, usize>(0)
            })
            .expect("short rows should count"),
        0
    );
}

#[test]
fn tail_owner_failure_rolls_back_the_preceding_cached_full_batch() {
    let mut connection = Connection::open_in_memory().expect("database should open");
    super::super::schema::initialize_code_schema(&connection)
        .expect("code schema should initialize");
    let transaction = connection.transaction().expect("transaction should start");
    let mut inserter = SearchDocumentInserter::new(&transaction).expect("inserter should build");
    for index in 0..SEARCH_DOCUMENT_INSERT_BATCH_SIZE {
        let record_id = format!("reference-{index}");
        inserter
            .insert(
                "scope",
                "reference",
                &record_id,
                "src/lib.rs",
                "rust",
                ["full batch content"],
            )
            .expect("the cached full batch should flush");
    }
    inserter
        .insert(
            "scope",
            "reference",
            "reference-0",
            "src/tail.rs",
            "rust",
            ["conflicting tail content"],
        )
        .expect("the conflicting tail should buffer");

    inserter
        .finish()
        .expect_err("the tail metadata owner must not replace the full-batch owner");
    assert_eq!(
        transaction
            .query_row("SELECT COUNT(*) FROM code_repository_search", [], |row| {
                row.get::<_, usize>(0)
            })
            .expect("pending FTS rows should count"),
        SEARCH_DOCUMENT_INSERT_BATCH_SIZE + 1
    );
    assert_eq!(
        transaction
            .query_row(
                "SELECT COUNT(*) FROM code_repository_search_metadata",
                [],
                |row| row.get::<_, usize>(0),
            )
            .expect("pending metadata rows should count"),
        SEARCH_DOCUMENT_INSERT_BATCH_SIZE
    );
    transaction
        .rollback()
        .expect("the caller should roll back both batches");

    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM code_repository_search", [], |row| {
                row.get::<_, usize>(0)
            })
            .expect("rolled-back FTS rows should count"),
        0
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM code_repository_search_metadata",
                [],
                |row| row.get::<_, usize>(0),
            )
            .expect("rolled-back metadata rows should count"),
        0
    );
}

#[test]
fn duplicate_search_owner_fails_without_orphaning_a_new_fts_row() {
    let mut connection = Connection::open_in_memory().expect("database should open");
    super::super::schema::initialize_code_schema(&connection)
        .expect("code schema should initialize");
    let transaction = connection.transaction().expect("transaction should start");
    let mut inserter = SearchDocumentInserter::new(&transaction).expect("inserter should build");
    inserter
        .insert(
            "scope",
            "reference",
            "reference-1",
            "src/lib.rs",
            "rust",
            ["first content"],
        )
        .expect("first search document should buffer");
    inserter.finish().expect("first document should flush");
    transaction.commit().expect("first document should commit");

    let transaction = connection.transaction().expect("transaction should start");
    let mut duplicate =
        SearchDocumentInserter::new(&transaction).expect("duplicate inserter should build");
    duplicate
        .insert(
            "scope",
            "reference",
            "reference-1",
            "src/lib.rs",
            "rust",
            ["replacement content"],
        )
        .expect("duplicate document should buffer");
    duplicate
        .finish()
        .expect_err("metadata ownership must never replace an existing FTS owner");
    transaction
        .rollback()
        .expect("failed duplicate should roll back its FTS row");

    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM code_repository_search WHERE source_scope = 'scope'",
                [],
                |row| row.get::<_, usize>(0),
            )
            .expect("FTS rows should count"),
        1
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM code_repository_search_metadata
                 WHERE source_scope = 'scope'",
                [],
                |row| row.get::<_, usize>(0),
            )
            .expect("metadata rows should count"),
        1
    );
}

#[test]
fn highest_unowned_fts_row_does_not_block_new_owned_documents_or_surface() {
    let mut connection = Connection::open_in_memory().expect("database should open");
    super::super::schema::initialize_code_schema(&connection)
        .expect("code schema should initialize");
    let transaction = connection.transaction().expect("transaction should start");
    let mut initial = SearchDocumentInserter::new(&transaction).expect("inserter should build");
    initial
        .insert(
            "scope",
            "reference",
            "reference-1",
            "src/lib.rs",
            "rust",
            ["owned content"],
        )
        .expect("initial document should buffer");
    initial.finish().expect("initial document should flush");
    transaction
        .commit()
        .expect("initial document should commit");

    connection
        .execute(
            "INSERT INTO code_repository_search (
                 rowid, source_scope, document_kind, record_id, path, language_id, content
             ) VALUES (9000000, 'scope', 'reference', 'reference-1', 'src/lib.rs', 'rust',
                       'legacy_orphan_term')",
            [],
        )
        .expect("legacy orphan should insert above the owned rowid");

    let transaction = connection.transaction().expect("transaction should start");
    let mut next = SearchDocumentInserter::new(&transaction).expect("inserter should build");
    next.insert(
        "scope",
        "reference",
        "reference-2",
        "src/next.rs",
        "rust",
        ["current_owned_term"],
    )
    .expect("new document should buffer");
    next.finish()
        .expect("the legacy orphan must stay outside the new metadata interval");
    transaction.commit().expect("new document should commit");

    let owned_count: usize = connection
        .query_row(
            &format!(
                "SELECT COUNT(*) FROM code_repository_search
                 WHERE code_repository_search MATCH 'current_owned_term'
                 {EXACT_SEARCH_OWNER_PREDICATE_SQL}"
            ),
            [],
            |row| row.get(0),
        )
        .expect("owned match should count");
    let orphan_count: usize = connection
        .query_row(
            &format!(
                "SELECT COUNT(*) FROM code_repository_search
                 WHERE code_repository_search MATCH 'legacy_orphan_term'
                 {EXACT_SEARCH_OWNER_PREDICATE_SQL}"
            ),
            [],
            |row| row.get(0),
        )
        .expect("orphan match should count");

    assert_eq!(owned_count, 1);
    assert_eq!(orphan_count, 0);
}

#[test]
fn interleaved_raw_fts_write_stays_outside_the_derived_metadata_interval() {
    let mut connection = Connection::open_in_memory().expect("database should open");
    super::super::schema::initialize_code_schema(&connection)
        .expect("code schema should initialize");
    let transaction = connection.transaction().expect("transaction should start");
    let mut inserter = SearchDocumentInserter::new(&transaction).expect("inserter should build");
    inserter
        .insert(
            "owned-scope",
            "reference",
            "owned-reference",
            "src/owned.rs",
            "rust",
            ["owned_pending_term"],
        )
        .expect("owned document should buffer");
    transaction
        .execute(
            "INSERT INTO code_repository_search (
                 source_scope, document_kind, record_id, path, language_id, content
             ) VALUES (
                 'raw-scope', 'reference', 'raw-reference', 'src/raw.rs', 'rust',
                 'interleaved_raw_term'
             )",
            [],
        )
        .expect("raw FTS row should interleave in the caller transaction");

    inserter
        .finish()
        .expect("the later owned interval should not claim the raw row");
    assert_eq!(
        transaction
            .query_row(
                "SELECT COUNT(*) FROM code_repository_search_metadata
                 WHERE source_scope = 'owned-scope'",
                [],
                |row| row.get::<_, usize>(0),
            )
            .expect("owned metadata rows should count"),
        1
    );
    assert_eq!(
        transaction
            .query_row(
                "SELECT COUNT(*) FROM code_repository_search_metadata
                 WHERE source_scope = 'raw-scope'",
                [],
                |row| row.get::<_, usize>(0),
            )
            .expect("raw metadata rows should count"),
        0
    );
    transaction.commit().expect("transaction should commit");
}

#[test]
fn maximum_fts_rowid_fails_before_a_pending_document_is_inserted() {
    let mut connection = Connection::open_in_memory().expect("database should open");
    super::super::schema::initialize_code_schema(&connection)
        .expect("code schema should initialize");
    connection
        .execute(
            "INSERT INTO code_repository_search (
                 rowid, source_scope, document_kind, record_id, path, language_id, content
             ) VALUES (?1, 'legacy', 'reference', 'maximum', 'legacy.rs', 'rust', 'maximum')",
            params![i64::MAX],
        )
        .expect("maximum legacy rowid should insert");

    let transaction = connection.transaction().expect("transaction should start");
    transaction
        .execute(
            "INSERT INTO code_repository_search (
                 rowid, source_scope, document_kind, record_id, path, language_id, content
             ) VALUES (42, 'raw', 'reference', 'raw', 'raw.rs', 'rust', 'raw')",
            [],
        )
        .expect("an unrelated caller write should be pending");
    let mut inserter = SearchDocumentInserter::new(&transaction).expect("inserter should build");
    inserter
        .insert(
            "owned",
            "reference",
            "pending",
            "pending.rs",
            "rust",
            ["must_not_be_inserted"],
        )
        .expect("document should buffer without writing");
    let error = inserter
        .finish()
        .expect_err("maximum rowid must fail before random allocation");
    assert!(error.to_string().contains("maximum SQLite rowid"));
    assert_eq!(
        transaction
            .query_row(
                "SELECT COUNT(*) FROM code_repository_search
                 WHERE content = 'must_not_be_inserted'",
                [],
                |row| row.get::<_, usize>(0),
            )
            .expect("pending FTS rows should count"),
        0
    );
    assert_eq!(
        transaction
            .query_row(
                "SELECT COUNT(*) FROM code_repository_search_metadata",
                [],
                |row| row.get::<_, usize>(0),
            )
            .expect("metadata rows should count"),
        0
    );
    transaction
        .rollback()
        .expect("the caller should roll back unrelated writes too");
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM code_repository_search", [], |row| {
                row.get::<_, usize>(0)
            })
            .expect("only the committed maximum row should remain"),
        1
    );
}

#[test]
fn code_index_persistence_performance_suite_search_insert_uses_post_insert_intervals_without_max_scan()
 {
    let mut connection = Connection::open_in_memory().expect("database should open");
    super::super::schema::initialize_code_schema(&connection)
        .expect("code schema should initialize");
    connection
        .execute(
            "INSERT INTO code_repository_search (
                 rowid, source_scope, document_kind, record_id, path, language_id, content
             ) VALUES (9000000, 'legacy', 'reference', 'orphan', 'legacy.rs', 'rust', 'orphan')",
            [],
        )
        .expect("high orphan should insert");

    SEARCH_SQL_TRACE.lock().expect("trace should lock").clear();
    connection.trace(Some(capture_search_sql));
    let transaction = connection.transaction().expect("transaction should start");
    let mut inserter = SearchDocumentInserter::new(&transaction).expect("inserter should build");
    for index in 0..=SEARCH_DOCUMENT_INSERT_BATCH_SIZE {
        let record_id = format!("reference-{index:04}");
        inserter
            .insert(
                "scope",
                "reference",
                &record_id,
                "src/lib.rs",
                "rust",
                ["owned"],
            )
            .expect("document should buffer or flush");
    }
    inserter.finish().expect("tail should flush");
    transaction.commit().expect("transaction should commit");
    connection.trace(None);

    let trace = SEARCH_SQL_TRACE.lock().expect("trace should lock").clone();
    assert!(
        trace.iter().all(|statement| {
            !statement
                .split_whitespace()
                .collect::<String>()
                .to_ascii_lowercase()
                .contains("max(rowid)")
        }),
        "search insertion must not scan the FTS virtual table for max(rowid): {trace:?}"
    );
    assert_eq!(
        trace
            .iter()
            .filter(|statement| statement.contains("9223372036854775807"))
            .count(),
        2,
        "each of the full and tail flushes needs one exact maximum-rowid probe: {trace:?}"
    );
    assert_eq!(
        trace
            .iter()
            .filter(|statement| {
                statement
                    .split_whitespace()
                    .collect::<String>()
                    .starts_with("INSERTINTOcode_repository_search(")
            })
            .count(),
        2,
        "one 1024-document group plus its tail must use two bounded main FTS inserts: {trace:?}"
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM code_repository_search_metadata
                 WHERE source_scope = 'legacy'",
                [],
                |row| row.get::<_, usize>(0),
            )
            .expect("orphan ownership should count"),
        0
    );
    let owned_interval = connection
        .query_row(
            "SELECT MIN(search_rowid), MAX(search_rowid), COUNT(*)
             FROM code_repository_search_metadata
             WHERE source_scope = 'scope'",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, usize>(2)?,
                ))
            },
        )
        .expect("owned interval should load");
    assert_eq!(
        owned_interval,
        (
            9_000_001,
            9_000_000 + SEARCH_DOCUMENT_INSERT_BATCH_SIZE as i64 + 1,
            SEARCH_DOCUMENT_INSERT_BATCH_SIZE + 1,
        )
    );

    let plan = connection
        .prepare(&format!(
            "EXPLAIN QUERY PLAN {SEARCH_DOCUMENT_MAX_ROWID_EXISTS_SQL}"
        ))
        .expect("probe plan should prepare")
        .query_map(params![i64::MAX], |row| row.get::<_, String>(3))
        .expect("probe plan should run")
        .collect::<Result<Vec<_>, _>>()
        .expect("probe plan should collect");
    assert!(
        plan.iter().any(|detail| {
            detail.contains("code_repository_search VIRTUAL TABLE INDEX") && detail.contains('=')
        }),
        "maximum-rowid admission must use the FTS rowid equality plan: {plan:?}"
    );
}

fn capture_search_sql(statement: &str) {
    SEARCH_SQL_TRACE
        .lock()
        .expect("trace should lock")
        .push(statement.to_owned());
}

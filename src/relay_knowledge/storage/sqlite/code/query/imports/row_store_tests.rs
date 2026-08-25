//! Direct row-store contract for identifier/path filtering and SQL bind order.

use rusqlite::params;

use super::*;
use crate::{
    domain::{
        CodeQueryKind, CodeRepositoryRegistration, CodeRepositorySelector, CodeRepositoryStatus,
        CodeRetrievalRequest, FreshnessPolicy,
    },
    storage::{CodeRepositoryStore, SqliteGraphStore},
};

const TEST_SCOPE: &str = "code:test:import-direct-path:commit:tree";

#[test]
fn import_identifier_patterns_keep_alias_and_target_terms() {
    let patterns = import_identifier_patterns("disposeInstance as runDisposers instance registry");

    assert!(patterns.contains(&"%disposeinstance%".to_owned()));
    assert!(patterns.contains(&"%rundisposers%".to_owned()));
    assert!(patterns.contains(&"%registry%".to_owned()));
    assert!(!patterns.contains(&"%as%".to_owned()));
}

#[test]
fn import_identifier_patterns_drop_import_syntax_keywords() {
    let patterns = import_identifier_patterns("import serde using System include vector");

    assert!(patterns.contains(&"%serde%".to_owned()));
    assert!(patterns.contains(&"%system%".to_owned()));
    assert!(patterns.contains(&"%vector%".to_owned()));
    assert!(!patterns.contains(&"%import%".to_owned()));
    assert!(!patterns.contains(&"%using%".to_owned()));
    assert!(!patterns.contains(&"%include%".to_owned()));
}

#[test]
fn import_identifier_probe_completes_inside_its_sqlite_work_budget() {
    let connection = identifier_probe_connection();

    let probe = search_import_identifier_rows_with_progress_budget(
        &connection,
        &identifier_status(),
        &identifier_request(),
        ImportIdentifierProbeBudget {
            progress_interval: IMPORT_IDENTIFIER_SQL_PROGRESS_INTERVAL,
            max_progress_callbacks: MAX_IMPORT_IDENTIFIER_SQL_PROGRESS_CALLBACKS,
        },
    )
    .expect("a small optional identifier probe should complete");

    assert!(!probe.saturated);
    assert_eq!(probe.rows.len(), 1);
    assert_eq!(probe.rows[0].path, "src/UtilityConsumer.java");
}

#[test]
fn import_identifier_sqlite_work_budget_interrupts_and_clears_its_handler() {
    let connection = identifier_probe_connection();

    let probe = search_import_identifier_rows_with_progress_budget(
        &connection,
        &identifier_status(),
        &identifier_request(),
        ImportIdentifierProbeBudget {
            progress_interval: 1,
            max_progress_callbacks: 0,
        },
    )
    .expect("a work-budget interruption should disable optional identifier recall");

    assert!(probe.saturated);
    assert!(probe.rows.is_empty());
    let value = connection
        .query_row("SELECT 1", [], |row| row.get::<_, i64>(0))
        .expect("the interrupted probe must remove its progress handler");
    assert_eq!(value, 1);
}

#[tokio::test]
async fn import_path_direct_rows_apply_inline_path_before_gate() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
                .expect("registration should validate"),
        )
        .await
        .expect("repository should persist");
    store
            .run(|connection| {
                connection.execute(
                    "
                    INSERT INTO code_repository_files (
                        repository_id, source_scope, file_id, path, language_id, blob_hash,
                        byte_len, line_count, parse_status, is_generated, degraded_reason
                    )
                    VALUES
                        ('repo', ?1, 'api-file', 'src/api.rs', 'rust', 'api-hash', 1, 1, 'parsed', 0, NULL),
                        ('repo', ?1, 'storage-file', 'src/storage/use.rs', 'rust', 'storage-hash', 1, 1, 'parsed', 0, NULL)
                    ",
                    params![TEST_SCOPE],
                )?;
                connection.execute(
                    "
                    INSERT INTO code_repository_imports (
                        repository_id, source_scope, import_id, file_id, path, module,
                        target_hint, resolution_state, confidence_basis_points,
                        confidence_tier, line_start, line_end
                    )
                    VALUES
                        ('repo', ?1, 'api-import', 'api-file', 'src/api.rs', 'shared/module', NULL, 'unresolved', 5000, 'inferred', 1, 1),
                        ('repo', ?1, 'storage-import', 'storage-file', 'src/storage/use.rs', 'shared/module', NULL, 'unresolved', 5000, 'inferred', 1, 1)
                    ",
                    params![TEST_SCOPE],
                )?;
                Ok(())
            })
            .await
            .expect("fixture rows should insert");
    let request = CodeRetrievalRequest::new(
        "path:storage shared/module",
        CodeRepositorySelector::new("repo", "commit", Vec::new(), vec!["rust".to_owned()])
            .expect("selector should validate"),
        CodeQueryKind::Imports,
        5,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");
    let status = CodeRepositoryStatus {
        repository_id: "repo".to_owned(),
        alias: "repo".to_owned(),
        root_path: "/tmp/repo".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        last_indexed_scope_id: Some(TEST_SCOPE.to_owned()),
        last_indexed_commit: Some("commit".to_owned()),
        tree_hash: Some("tree".to_owned()),
        state: "fresh".to_owned(),
        indexed_file_count: 2,
        symbol_count: 0,
        reference_count: 0,
        chunk_count: 0,
        stale: false,
        degraded_reason: None,
    };

    let paths = store
        .run(move |connection| {
            let rows = search_import_path_rows(connection, &status, &request)?;
            Ok(rows
                .rows
                .into_iter()
                .map(|row| row.path)
                .collect::<Vec<_>>())
        })
        .await
        .expect("direct import lookup should run");

    assert_eq!(paths, ["src/storage/use.rs"]);
}

#[test]
fn exact_import_paths_reserve_the_bounded_full_candidate_window() {
    let mut connection = identifier_probe_connection();
    let transaction = connection
        .transaction()
        .expect("fixture transaction should begin");
    for index in 0..=300 {
        let file_id = format!("runtime-file-{index:03}");
        let path = format!("src/runtime/consumer-{index:03}.go");
        transaction
            .execute(
                "INSERT INTO code_repository_files VALUES (?1, ?2, ?3, 'go', 0, 20)",
                params!["scope", file_id, path],
            )
            .expect("fixture file should insert");
        transaction
            .execute(
                "INSERT INTO code_repository_imports VALUES (
                     ?1, ?2, ?3, ?4, 'clientset example.org/runtime/client',
                     'example.org/runtime/client', 'resolved', 8000, 'inferred', 1, 1
                 )",
                params!["scope", format!("runtime-import-{index:03}"), file_id, path],
            )
            .expect("fixture import should insert");
    }
    transaction
        .commit()
        .expect("fixture transaction should commit");
    let request = CodeRetrievalRequest::new(
        "example.org/runtime/client",
        CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
            .expect("selector should validate"),
        CodeQueryKind::Imports,
        5,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");

    let rows = search_import_path_rows(&connection, &identifier_status(), &request)
        .expect("exact path recall should remain bounded");

    assert!(!rows.saturated);
    assert_eq!(rows.rows.len(), 301);
    assert!(
        rows.rows
            .iter()
            .any(|row| row.path == "src/runtime/consumer-300.go")
    );
}

fn identifier_probe_connection() -> Connection {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_files (
                source_scope TEXT NOT NULL,
                file_id TEXT NOT NULL,
                path TEXT NOT NULL,
                language_id TEXT NOT NULL,
                is_generated INTEGER NOT NULL,
                line_count INTEGER NOT NULL
            );
            CREATE TABLE code_repository_imports (
                source_scope TEXT NOT NULL,
                import_id TEXT NOT NULL,
                file_id TEXT NOT NULL,
                path TEXT NOT NULL,
                module TEXT NOT NULL,
                target_hint TEXT,
                resolution_state TEXT NOT NULL,
                confidence_basis_points INTEGER NOT NULL,
                confidence_tier TEXT NOT NULL,
                line_start INTEGER NOT NULL,
                line_end INTEGER NOT NULL
            );
            INSERT INTO code_repository_files VALUES (
                'scope', 'file-1', 'src/UtilityConsumer.java', 'java', 0, 12
            );
            INSERT INTO code_repository_imports VALUES (
                'scope', 'import-1', 'file-1', 'src/UtilityConsumer.java',
                'import org.springframework.util.ObjectUtils;', NULL,
                'unresolved', 10000, 'extracted', 1, 1
            );
            ",
        )
        .expect("identifier probe fixture should persist");
    connection
}

fn identifier_request() -> CodeRetrievalRequest {
    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");
    CodeRetrievalRequest::new(
        "ObjectUtils",
        selector,
        CodeQueryKind::Imports,
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate")
}

fn identifier_status() -> CodeRepositoryStatus {
    CodeRepositoryStatus {
        repository_id: "repo".to_owned(),
        alias: "repo".to_owned(),
        root_path: "/repo".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        last_indexed_scope_id: Some("scope".to_owned()),
        last_indexed_commit: Some("commit".to_owned()),
        tree_hash: Some("tree".to_owned()),
        state: "fresh".to_owned(),
        indexed_file_count: 1,
        symbol_count: 0,
        reference_count: 0,
        chunk_count: 0,
        stale: false,
        degraded_reason: None,
    }
}

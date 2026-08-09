//! Direct contracts for empty impact evidence sets at the SQLite boundary.

use std::collections::BTreeSet;

use rusqlite::Connection;

use super::evidence::{callers_for_symbols, chunks_for_paths, importers_for_modules};
use crate::domain::{CodeImpactRequest, CodeRepositorySelector, CodeRepositoryStatus};

#[test]
fn empty_seeds_return_without_preparing_evidence_queries() {
    let connection = Connection::open_in_memory().expect("database should open");
    let status = status();
    let request = request();

    assert!(
        chunks_for_paths(&connection, &status, &BTreeSet::new(), &request)
            .expect("empty paths should not query")
            .is_empty()
    );
    assert!(
        callers_for_symbols(&connection, &status, &[], &[], &request)
            .expect("empty symbols should not query")
            .is_empty()
    );
    assert!(
        importers_for_modules(&connection, &status, &[], &request)
            .expect("empty modules should not query")
            .is_empty()
    );
}

#[test]
fn importer_evidence_batches_large_module_seed_sets() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_files (
                source_scope TEXT, path TEXT, language_id TEXT, is_generated INTEGER
            );
            CREATE TABLE code_repository_imports (
                source_scope TEXT, file_id TEXT, path TEXT, module TEXT,
                line_start INTEGER, line_end INTEGER, target_hint TEXT,
                resolution_state TEXT, confidence_basis_points INTEGER,
                confidence_tier TEXT
            );
            INSERT INTO code_repository_files VALUES ('scope', 'src/main.rs', 'rust', 0);
            INSERT INTO code_repository_imports VALUES (
                'scope', 'file-main', 'src/main.rs', 'crate::target', 1, 1,
                'crate::target', 'resolved', 10000, 'extracted'
            );
            ",
        )
        .expect("impact tables should be created");
    let mut modules = (0..1_001)
        .map(|index| format!("unrelated::{index}"))
        .collect::<Vec<_>>();
    modules.push("crate::target".to_owned());

    let hits = importers_for_modules(&connection, &status(), &modules, &request())
        .expect("large module sets should stay below SQLite expression limits");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "src/main.rs");
}

fn status() -> CodeRepositoryStatus {
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
        indexed_file_count: 0,
        symbol_count: 0,
        reference_count: 0,
        chunk_count: 0,
        stale: false,
        degraded_reason: None,
    }
}

fn request() -> CodeImpactRequest {
    CodeImpactRequest::new(
        CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
            .expect("selector should validate"),
        "base",
        "head",
        10,
    )
    .expect("impact request should validate")
}

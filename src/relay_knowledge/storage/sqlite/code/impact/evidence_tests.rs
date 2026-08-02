//! Direct contracts for empty impact evidence sets at the SQLite boundary.

use std::collections::BTreeSet;

use rusqlite::Connection;

use super::evidence::{callers_for_symbols, chunks_for_paths, importers_for_modules};
use crate::domain::{CodeImpactRequest, CodeRepositorySelector, CodeRepositoryStatus};

#[test]
fn empty_seeds_return_without_preparing_evidence_queries() {
    let connection = Connection::open_in_memory().expect("database should open");
    let status = CodeRepositoryStatus {
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
    };
    let request = CodeImpactRequest::new(
        CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
            .expect("selector should validate"),
        "base",
        "head",
        10,
    )
    .expect("impact request should validate");

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

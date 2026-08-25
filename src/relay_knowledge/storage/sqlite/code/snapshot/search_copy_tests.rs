use rusqlite::{Connection, params};

use crate::domain::CodeRepositoryRegistration;

use super::super::{
    super::{
        code_tests::{retarget_snapshot_to_fact_scope, snapshot_with_chunk},
        initialize_code_schema,
        lifecycle::status::upsert_repository,
    },
    apply_snapshot,
};

#[test]
fn bounded_incremental_snapshot_copies_only_exactly_owned_search_documents() {
    let mut connection = Connection::open_in_memory().expect("database should open");
    initialize_code_schema(&connection).expect("schema should initialize");
    upsert_repository(
        &mut connection,
        CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate"),
    )
    .expect("repository should persist");

    let mut base = snapshot_with_chunk("repo", "src/lib.rs", "fn stable_policy() {}");
    base.resolved_commit_sha = "commit-base".to_owned();
    base.tree_hash = "tree-base".to_owned();
    retarget_snapshot_to_fact_scope(&mut base);
    let base_scope = base.source_scope.clone();
    let mut incremental = base.clone();
    apply_snapshot(&mut connection, base).expect("base snapshot should persist");

    connection
        .execute(
            "
            INSERT INTO code_repository_search (
                source_scope, document_kind, record_id, path, language_id, content
            )
            SELECT source_scope, document_kind, record_id, path, language_id,
                   content || ' orphan-highest-rowid'
            FROM code_repository_search
            WHERE source_scope = ?1
              AND document_kind = 'chunk'
              AND record_id = 'chunk'
            LIMIT 1
            ",
            params![base_scope],
        )
        .expect("unowned duplicate should insert");

    incremental.base_resolved_commit_sha = Some("commit-base".to_owned());
    incremental.resolved_commit_sha = "commit-next".to_owned();
    incremental.tree_hash = "tree-next".to_owned();
    incremental.full_replace = false;
    incremental.changed_path_count = 0;
    incremental.skipped_unchanged_count = 1;
    retarget_snapshot_to_fact_scope(&mut incremental);
    let target_scope = incremental.source_scope.clone();
    incremental.files.clear();
    incremental.symbols.clear();
    incremental.references.clear();
    incremental.imports.clear();
    incremental.calls.clear();
    incremental.dependencies.clear();
    incremental.feature_flags.clear();
    incremental.routes.clear();
    incremental.chunks.clear();
    incremental.workspaces.clear();
    incremental.diagnostics.clear();
    incremental.tombstones.clear();

    let summary = apply_snapshot(&mut connection, incremental)
        .expect("a bounded incremental snapshot should publish directly");
    let (
        target_scope_count,
        target_file_count,
        target_search_count,
        target_metadata_count,
        base_search_count,
        base_metadata_count,
        target_orphan_count,
    ): (usize, usize, usize, usize, usize, usize, usize) = connection
        .query_row(
            "
            SELECT
                (SELECT COUNT(*)
                 FROM code_repository_scopes
                 WHERE source_scope = ?1),
                (SELECT COUNT(*)
                 FROM code_repository_files
                 WHERE source_scope = ?1),
                (SELECT COUNT(*)
                 FROM code_repository_search
                 WHERE source_scope = ?1),
                (SELECT COUNT(*)
                 FROM code_repository_search_metadata
                 WHERE source_scope = ?1),
                (SELECT COUNT(*)
                 FROM code_repository_search
                 WHERE source_scope = ?2),
                (SELECT COUNT(*)
                 FROM code_repository_search_metadata
                 WHERE source_scope = ?2),
                (SELECT COUNT(*)
                 FROM code_repository_search
                 WHERE source_scope = ?1
                   AND instr(content, 'orphan-highest-rowid') > 0)
            ",
            params![target_scope, base_scope],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )
        .expect("isolated orphan clone state should load");

    assert_eq!(summary.source_scope, target_scope);
    assert_eq!(target_scope_count, 1);
    assert_eq!(target_file_count, 1);
    assert_eq!(target_search_count, 1);
    assert_eq!(target_metadata_count, 1);
    assert_eq!(base_search_count, 2);
    assert_eq!(base_metadata_count, 1);
    assert_eq!(target_orphan_count, 0);
}

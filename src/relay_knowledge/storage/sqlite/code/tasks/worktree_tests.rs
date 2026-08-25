use rusqlite::Connection;

use super::{
    active_worktree_base_scopes, compatible_non_retiring_scopes_for_commit,
    worktree_task_base_commit,
};

#[test]
fn active_worktree_retains_a_clean_base_reached_through_commit_alias() {
    let connection = Connection::open_in_memory().expect("in-memory database should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_scopes (
                source_scope TEXT PRIMARY KEY,
                repository_id TEXT NOT NULL,
                resolved_commit_sha TEXT NOT NULL,
                path_filters_json TEXT NOT NULL,
                language_filters_json TEXT NOT NULL,
                stale INTEGER NOT NULL DEFAULT 0,
                retiring INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE code_repository_commit_scopes (
                repository_id TEXT NOT NULL,
                resolved_commit_sha TEXT NOT NULL,
                source_scope TEXT NOT NULL
            );
            CREATE TABLE code_repository_scope_gc_jobs (
                repository_id TEXT NOT NULL,
                source_scope TEXT NOT NULL
            );
            INSERT INTO code_repository_scopes VALUES
                ('scope-base', 'repo', 'same-tree-newer', '[]', '[]', 0, 0),
                ('scope-worktree', 'repo', 'worktree:base-commit:overlay', '[]', '[]', 0, 0);
            INSERT INTO code_repository_commit_scopes VALUES
                ('repo', 'base-commit', 'scope-base');
            ",
        )
        .expect("worktree retention fixtures should persist");

    let retained = active_worktree_base_scopes(&connection, "repo", "scope-worktree")
        .expect("worktree base scopes should resolve");

    assert_eq!(retained, ["scope-base"]);
}

#[test]
fn compatible_commit_scope_requires_matching_filters_and_no_retirement_job() {
    let connection = Connection::open_in_memory().expect("in-memory database should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_scopes (
                source_scope TEXT PRIMARY KEY,
                repository_id TEXT NOT NULL,
                resolved_commit_sha TEXT NOT NULL,
                path_filters_json TEXT NOT NULL,
                language_filters_json TEXT NOT NULL,
                stale INTEGER NOT NULL DEFAULT 0,
                retiring INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE code_repository_commit_scopes (
                repository_id TEXT NOT NULL,
                resolved_commit_sha TEXT NOT NULL,
                source_scope TEXT NOT NULL
            );
            CREATE TABLE code_repository_scope_gc_jobs (
                repository_id TEXT NOT NULL,
                source_scope TEXT NOT NULL
            );
            INSERT INTO code_repository_scopes VALUES
                ('direct', 'repo', 'commit-a', '[\"src\"]', '[\"rust\"]', 0, 0),
                ('retiring', 'repo', 'commit-b', '[\"src\"]', '[\"rust\"]', 0, 1),
                ('alias', 'repo', 'newer-same-tree', '[\"src\"]', '[\"rust\"]', 0, 0),
                ('stale', 'repo', 'commit-c', '[\"src\"]', '[\"rust\"]', 1, 0),
                ('wrong-filter', 'repo', 'commit-a', '[]', '[\"rust\"]', 0, 0);
            INSERT INTO code_repository_commit_scopes VALUES
                ('repo', 'commit-b', 'alias');
            INSERT INTO code_repository_scope_gc_jobs VALUES
                ('repo', 'direct');
            ",
        )
        .expect("compatible scope fixtures should persist");

    let direct = compatible_non_retiring_scopes_for_commit(
        &connection,
        "repo",
        "commit-a",
        "[\"src\"]",
        "[\"rust\"]",
    )
    .expect("direct scope should resolve");
    let alias = compatible_non_retiring_scopes_for_commit(
        &connection,
        "repo",
        "commit-b",
        "[\"src\"]",
        "[\"rust\"]",
    )
    .expect("alias scope should resolve");
    let stale = compatible_non_retiring_scopes_for_commit(
        &connection,
        "repo",
        "commit-c",
        "[\"src\"]",
        "[\"rust\"]",
    )
    .expect("stale scope eligibility should resolve");

    assert!(direct.is_empty());
    assert_eq!(alias, ["alias"]);
    assert!(stale.is_empty());
}

#[test]
fn worktree_task_base_prefers_pinned_identity_over_selector() {
    assert_eq!(
        worktree_task_base_commit("worktree:pending:pinned", "moving-head"),
        Some("pinned")
    );
    assert_eq!(
        worktree_task_base_commit("worktree:clean:overlay", "moving-head"),
        Some("clean")
    );
    assert_eq!(
        worktree_task_base_commit("unexpected", "fallback-clean"),
        Some("fallback-clean")
    );
}

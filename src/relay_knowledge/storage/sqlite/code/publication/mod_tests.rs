//! Transaction-level publication and rollback invariants.

use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

use crate::domain::{CodeIncrementalSummaryReceipt, CodeIndexPublicationFence};
use crate::storage::CodeIndexPublicationTarget;

use super::{ScopePublication, adopt_active_target, complete_after_software_projection, stage};

#[test]
fn fenced_publication_withholds_scope_repository_and_checkpoint_until_projection() {
    let connection = publication_database();
    let publication = fixture_publication();
    let guard = local_guard(&connection);

    stage(&connection, &publication, true).expect("scope should stage");

    assert_eq!(repository_state(&connection), ("indexing".to_owned(), true));
    assert_eq!(scope_stale(&connection), Some(true));
    assert_eq!(
        checkpoint_state(&connection),
        "finalizing:software_projection"
    );
    let error = complete_after_software_projection(&connection, "scope-new", &guard)
        .expect_err("missing software projection must block publication");
    assert!(error.to_string().contains("cannot publish before"));

    connection
        .execute(
            "INSERT INTO software_global_status (source_scope, stale) VALUES ('scope-new', 1)",
            [],
        )
        .expect("staged projection should insert");
    let transaction = connection
        .unchecked_transaction()
        .expect("publication transaction should begin");
    complete_after_software_projection(&transaction, "scope-new", &guard)
        .expect("projection should release publication");
    transaction.commit().expect("publication should commit");

    assert_eq!(repository_state(&connection), ("fresh".to_owned(), false));
    assert_eq!(scope_stale(&connection), Some(false));
    assert_eq!(checkpoint_state(&connection), "completed");
    assert_eq!(software_stale(&connection), Some(false));
    assert_eq!(publication_receipt_count(&connection), 1);
}

#[test]
fn projection_failure_rolls_back_to_the_previous_published_scope() {
    let connection = publication_database();
    let publication = fixture_publication();
    let guard = local_guard(&connection);
    stage(&connection, &publication, true).expect("scope should stage");
    connection
        .execute(
            "INSERT INTO software_global_status (source_scope, stale) VALUES ('scope-new', 1)",
            [],
        )
        .expect("staged projection should insert");
    let transaction = connection
        .unchecked_transaction()
        .expect("publication transaction should begin");
    complete_after_software_projection(&transaction, "scope-new", &guard)
        .expect("publication should stage in transaction");
    transaction
        .rollback()
        .expect("simulated projection failure should rollback");

    assert_eq!(repository_state(&connection), ("indexing".to_owned(), true));
    assert_eq!(active_scope(&connection), "scope-old");
    assert_eq!(scope_stale(&connection), Some(true));
    assert_eq!(
        checkpoint_state(&connection),
        "finalizing:software_projection"
    );
    assert_eq!(software_stale(&connection), Some(true));
}

#[test]
fn publication_rejects_an_early_checkpoint_phase_without_mutation() {
    let connection = publication_database();
    let guard = local_guard(&connection);
    stage(&connection, &fixture_publication(), true).expect("scope should stage");
    connection
        .execute(
            "INSERT INTO software_global_status (source_scope, stale) VALUES ('scope-new', 1)",
            [],
        )
        .expect("staged projection should insert");
    connection
        .execute(
            "UPDATE code_repository_index_checkpoints SET state = 'indexing' WHERE source_scope = 'scope-new'",
            [],
        )
        .expect("checkpoint should rewind for the invariant fixture");

    let error = complete_after_software_projection(&connection, "scope-new", &guard)
        .expect_err("an indexing checkpoint must not publish");

    assert!(error.to_string().contains("checkpoint state 'indexing'"));
    assert_eq!(repository_state(&connection), ("indexing".to_owned(), true));
    assert_eq!(scope_stale(&connection), Some(true));
    assert_eq!(software_stale(&connection), Some(true));
    assert_eq!(checkpoint_state(&connection), "indexing");
}

#[test]
fn local_fence_adopts_a_same_tree_commit_without_rewriting_facts() {
    let mut connection = active_adoption_database();
    let guard = local_guard(&connection);
    let before = derived_fact_state(&connection);

    assert!(
        adopt_active_target(&mut connection, &same_tree_target(), &guard)
            .expect("same-tree commit should adopt the active content scope")
    );

    assert_eq!(
        published_commit_state(&connection),
        (
            "commit-next".to_owned(),
            "commit-next".to_owned(),
            "commit-next".to_owned()
        )
    );
    assert_eq!(
        commit_aliases(&connection),
        vec!["commit-next", "commit-old"]
    );
    assert_eq!(
        registration_filters(&connection),
        ("[]".to_owned(), "[]".to_owned())
    );
    assert_eq!(publication_receipt_count(&connection), 1);
    assert_eq!(derived_fact_state(&connection), before);
}

#[test]
fn local_fence_reactivates_a_retained_content_scope_without_rewriting_facts() {
    let mut connection = retained_adoption_database();
    let guard = local_guard(&connection);
    let retained_facts = (1, "blob-stable".to_owned(), 1, 1, 1, 17);

    assert!(
        adopt_active_target(&mut connection, &same_tree_target(), &guard)
            .expect("retained same-tree content should be adopted")
    );

    assert_eq!(active_scope(&connection), "scope-old");
    assert_eq!(derived_fact_state(&connection), retained_facts);
    assert_eq!(
        published_commit_state(&connection),
        (
            "commit-next".to_owned(),
            "commit-next".to_owned(),
            "commit-next".to_owned()
        )
    );
    assert_eq!(
        commit_aliases(&connection),
        vec!["commit-next", "commit-old"]
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM code_repository_commit_scopes
                 WHERE repository_id = 'repo'
                   AND resolved_commit_sha = 'commit-current'
                   AND source_scope = 'scope-current'",
                [],
                |row| row.get::<_, usize>(0),
            )
            .expect("previous active alias should load"),
        1
    );
}

#[test]
fn same_content_adoption_clears_a_previous_tasks_incremental_receipt() {
    let mut connection = active_adoption_database();
    let encoded = super::super::checkpoint_receipt::encode(&incremental_receipt("task-old"))
        .expect("old task receipt should encode");
    connection
        .execute(
            "UPDATE code_repository_index_checkpoints
             SET incremental_summary_json = ?1 WHERE source_scope = 'scope-old'",
            [encoded],
        )
        .expect("old task receipt should install");
    let guard = local_guard(&connection);

    assert!(
        adopt_active_target(&mut connection, &same_tree_target(), &guard)
            .expect("new task should adopt the active content scope")
    );

    assert_eq!(incremental_receipt_json(&connection), None);
}

#[test]
fn same_task_adoption_retains_its_incremental_receipt_for_response_recovery() {
    let mut connection = active_adoption_database();
    let encoded = super::super::checkpoint_receipt::encode(&incremental_receipt("task"))
        .expect("same task receipt should encode");
    connection
        .execute(
            "UPDATE code_repository_index_checkpoints
             SET incremental_summary_json = ?1 WHERE source_scope = 'scope-old'",
            [encoded.clone()],
        )
        .expect("same task receipt should install");
    let guard = local_guard(&connection);

    assert!(
        adopt_active_target(&mut connection, &same_tree_target(), &guard)
            .expect("same task should reconcile the active content scope")
    );

    assert_eq!(incremental_receipt_json(&connection), Some(encoded));
}

#[test]
fn external_authority_adopts_metadata_without_writing_a_local_receipt() {
    let authority_path = std::env::temp_dir().join(format!(
        "relay-knowledge-publication-authority-{}-{}.sqlite",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let authority = Connection::open(&authority_path).expect("authority database should open");
    initialize_authority(&authority);
    drop(authority);

    let mut connection = active_adoption_database();
    connection
        .execute("DELETE FROM code_repository_publication_receipts", [])
        .expect("local receipts should start empty");
    let guard = super::super::lifecycle::publication_fence::prepare_guard(
        &connection,
        publication_fence(),
        Some(&authority_path),
    )
    .expect("external publication guard should prepare");
    let before = derived_fact_state(&connection);

    assert!(
        adopt_active_target(&mut connection, &same_tree_target(), &guard)
            .expect("external authority should fence same-tree metadata adoption")
    );

    assert_eq!(
        published_commit_state(&connection),
        (
            "commit-next".to_owned(),
            "commit-next".to_owned(),
            "commit-next".to_owned()
        )
    );
    assert_eq!(
        commit_aliases(&connection),
        vec!["commit-next", "commit-old"]
    );
    assert_eq!(publication_receipt_count(&connection), 0);
    assert_eq!(derived_fact_state(&connection), before);
    drop(connection);
    std::fs::remove_file(authority_path).expect("authority database should be removed");
}

fn fixture_publication() -> ScopePublication<'static> {
    ScopePublication {
        repository_id: "repo",
        source_scope: "scope-new",
        resolved_commit_sha: "commit-new",
        tree_hash: "tree-new",
        path_filters_json: "[]",
        language_filters_json: "[]",
        indexed_file_count: 3,
        symbol_count: 4,
        reference_count: 5,
        chunk_count: 6,
        degraded_reason: None,
    }
}

fn local_guard(
    connection: &Connection,
) -> super::super::lifecycle::publication_fence::PublicationFenceGuard {
    super::super::lifecycle::publication_fence::prepare_guard(connection, publication_fence(), None)
        .expect("local publication guard should prepare")
}

fn publication_fence() -> CodeIndexPublicationFence {
    CodeIndexPublicationFence {
        repository_id: "repo".to_owned(),
        task_id: "task".to_owned(),
        lease_owner: "worker".to_owned(),
        attempt_count: 1,
        generation: 1,
    }
}

fn same_tree_target() -> CodeIndexPublicationTarget {
    CodeIndexPublicationTarget {
        task_id: "task".to_owned(),
        repository_id: "repo".to_owned(),
        source_scope: "scope-old".to_owned(),
        resolved_commit_sha: "commit-next".to_owned(),
        tree_hash: "tree-old".to_owned(),
        path_filters: vec!["src/**".to_owned()],
        language_filters: vec!["rust".to_owned()],
    }
}

fn active_adoption_database() -> Connection {
    let connection = publication_database();
    connection
        .execute_batch(
            "
            UPDATE code_repository_index_tasks SET source_scope = 'scope-old';
            UPDATE code_repository_scopes
            SET path_filters_json = '[\"src/**\"]',
                language_filters_json = '[\"rust\"]'
            WHERE source_scope = 'scope-old';
            INSERT INTO software_global_status (source_scope, repository_id, stale, component_count)
            VALUES ('scope-old', 'repo', 0, 17);
            INSERT INTO code_repository_index_checkpoints (
                source_scope, repository_id, state, resolved_commit_sha, tree_hash,
                path_filters_json, language_filters_json, updated_at_ms, error_message
            ) VALUES (
                'scope-old', 'repo', 'completed', 'commit-old', 'tree-old',
                '[\"src/**\"]', '[\"rust\"]', 7, NULL
            );
            INSERT INTO code_repository_files (source_scope, path, blob_hash)
            VALUES ('scope-old', 'src/lib.rs', 'blob-stable');
            ",
        )
        .expect("active adoption fixture should initialize");
    connection
}

fn retained_adoption_database() -> Connection {
    let connection = active_adoption_database();
    connection
        .execute_batch(
            "
            INSERT INTO code_repository_scopes VALUES (
                'scope-current', 'repo', 'commit-current', 'tree-current',
                '[\"src/**\"]', '[\"rust\"]', 2, 3, 4, 5, 0, NULL, 0
            );
            INSERT INTO software_global_status (
                source_scope, repository_id, stale, component_count
            ) VALUES ('scope-current', 'repo', 0, 23);
            INSERT INTO code_repository_files (source_scope, path, blob_hash)
            VALUES ('scope-current', 'src/lib.rs', 'blob-current');
            UPDATE code_repositories
            SET last_indexed_scope_id = 'scope-current',
                last_indexed_commit = 'commit-current',
                tree_hash = 'tree-current',
                indexed_file_count = 2,
                symbol_count = 3,
                reference_count = 4,
                chunk_count = 5
            WHERE repository_id = 'repo';
            ",
        )
        .expect("retained adoption fixture should initialize");
    connection
}

fn incremental_receipt(task_id: &str) -> CodeIncrementalSummaryReceipt {
    CodeIncrementalSummaryReceipt {
        task_id: task_id.to_owned(),
        base_resolved_commit_sha: "base".to_owned(),
        changed_path_count: 1,
        skipped_unchanged_count: 0,
        deleted_path_count: 0,
        affected_path_count: 1,
        blob_read_count: 1,
        parsed_file_count: 1,
        sqlite_write_count: 1,
        degraded_file_count: 0,
        batch_count: 1,
    }
}

fn incremental_receipt_json(connection: &Connection) -> Option<String> {
    connection
        .query_row(
            "SELECT incremental_summary_json FROM code_repository_index_checkpoints
             WHERE source_scope = 'scope-old'",
            [],
            |row| row.get(0),
        )
        .expect("checkpoint receipt should load")
}

fn initialize_authority(connection: &Connection) {
    connection
        .execute_batch(
            "
            CREATE TABLE code_repository_index_tasks (
                task_id TEXT PRIMARY KEY,
                repository_id TEXT NOT NULL,
                source_scope TEXT NOT NULL,
                publication_generation INTEGER NOT NULL,
                attempt_count INTEGER NOT NULL,
                lease_owner TEXT,
                lease_expires_at_ms INTEGER,
                state TEXT NOT NULL
            );
            CREATE TABLE code_repository_publication_fences (
                repository_id TEXT PRIMARY KEY,
                generation INTEGER NOT NULL,
                task_id TEXT NOT NULL,
                attempt_count INTEGER NOT NULL,
                lease_owner TEXT NOT NULL,
                updated_at_ms INTEGER NOT NULL
            );
            INSERT INTO code_repository_index_tasks VALUES (
                'task', 'repo', 'scope-old', 1, 1, 'worker',
                9000000000000000, 'running'
            );
            INSERT INTO code_repository_publication_fences VALUES (
                'repo', 1, 'task', 1, 'worker', 1
            );
            ",
        )
        .expect("external authority fixture should initialize");
}

fn publication_database() -> Connection {
    let connection = Connection::open_in_memory().expect("sqlite should open");
    connection
        .execute_batch(
            "
            CREATE TABLE code_repositories (
                repository_id TEXT PRIMARY KEY,
                last_indexed_scope_id TEXT,
                last_indexed_commit TEXT,
                tree_hash TEXT,
                state TEXT NOT NULL,
                indexed_file_count INTEGER NOT NULL,
                symbol_count INTEGER NOT NULL,
                reference_count INTEGER NOT NULL,
                chunk_count INTEGER NOT NULL,
                stale INTEGER NOT NULL,
                degraded_reason TEXT,
                path_filters_json TEXT NOT NULL DEFAULT '[]',
                language_filters_json TEXT NOT NULL DEFAULT '[]'
            );
            CREATE TABLE code_repository_scopes (
                source_scope TEXT PRIMARY KEY,
                repository_id TEXT NOT NULL,
                resolved_commit_sha TEXT NOT NULL,
                tree_hash TEXT NOT NULL,
                path_filters_json TEXT NOT NULL,
                language_filters_json TEXT NOT NULL,
                indexed_file_count INTEGER NOT NULL,
                symbol_count INTEGER NOT NULL,
                reference_count INTEGER NOT NULL,
                chunk_count INTEGER NOT NULL,
                stale INTEGER NOT NULL,
                degraded_reason TEXT,
                retiring INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE code_repository_commit_scopes (
                repository_id TEXT NOT NULL,
                resolved_commit_sha TEXT NOT NULL,
                source_scope TEXT NOT NULL,
                published_sequence INTEGER NOT NULL,
                PRIMARY KEY (repository_id, resolved_commit_sha, source_scope)
            );
            CREATE TABLE code_repository_index_checkpoints (
                source_scope TEXT PRIMARY KEY,
                repository_id TEXT NOT NULL DEFAULT 'repo',
                state TEXT NOT NULL,
                resolved_commit_sha TEXT NOT NULL DEFAULT '',
                tree_hash TEXT NOT NULL DEFAULT '',
                path_filters_json TEXT NOT NULL DEFAULT '[]',
                language_filters_json TEXT NOT NULL DEFAULT '[]',
                resource_budget_json TEXT NOT NULL DEFAULT
                    '{\"max_files_per_batch\":512,\"max_bytes_per_batch\":16777216,\"max_rows_per_batch\":150000}',
                incremental_summary_json TEXT,
                updated_at_ms INTEGER NOT NULL,
                error_message TEXT
            );
            CREATE TABLE software_global_status (
                source_scope TEXT PRIMARY KEY,
                repository_id TEXT NOT NULL DEFAULT 'repo',
                stale INTEGER NOT NULL,
                component_count INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE code_repository_files (
                source_scope TEXT NOT NULL,
                path TEXT NOT NULL,
                blob_hash TEXT NOT NULL,
                PRIMARY KEY (source_scope, path)
            );
            CREATE TABLE code_repository_index_tasks (
                task_id TEXT PRIMARY KEY,
                repository_id TEXT NOT NULL,
                source_scope TEXT NOT NULL,
                publication_generation INTEGER NOT NULL,
                attempt_count INTEGER NOT NULL,
                lease_owner TEXT,
                lease_expires_at_ms INTEGER,
                state TEXT NOT NULL
            );
            CREATE TABLE code_repository_publication_fences (
                repository_id TEXT PRIMARY KEY,
                generation INTEGER NOT NULL,
                task_id TEXT NOT NULL,
                attempt_count INTEGER NOT NULL,
                lease_owner TEXT NOT NULL,
                updated_at_ms INTEGER NOT NULL
            );
            CREATE TABLE code_repository_publication_receipts (
                task_id TEXT NOT NULL,
                repository_id TEXT NOT NULL,
                source_scope TEXT NOT NULL,
                publication_generation INTEGER NOT NULL,
                published_at_ms INTEGER NOT NULL,
                PRIMARY KEY (task_id, publication_generation)
            );
            CREATE TABLE code_repository_reference_search_groups (
                source_scope TEXT NOT NULL,
                group_id TEXT NOT NULL,
                PRIMARY KEY (source_scope, group_id)
            );
            CREATE TABLE code_repository_reference_search_manifests (
                source_scope TEXT PRIMARY KEY,
                projection_version INTEGER NOT NULL,
                reference_count INTEGER NOT NULL,
                group_count INTEGER NOT NULL
            );
            CREATE TABLE code_repository_reference_search_progress (
                source_scope TEXT PRIMARY KEY
            );
            INSERT INTO code_repositories VALUES (
                'repo', 'scope-old', 'commit-old', 'tree-old', 'fresh',
                1, 1, 1, 1, 0, NULL, '[]', '[]'
            );
            INSERT INTO code_repository_scopes VALUES (
                'scope-old', 'repo', 'commit-old', 'tree-old', '[]', '[]',
                1, 1, 1, 1, 0, NULL, 0
            );
            INSERT INTO code_repository_index_checkpoints (
                source_scope, state, updated_at_ms, error_message
            ) VALUES (
                'scope-new', 'finalizing:software_projection', 1, NULL
            );
            INSERT INTO code_repository_index_tasks VALUES (
                'task', 'repo', 'scope-new', 1, 1, 'worker',
                9000000000000000, 'running'
            );
            INSERT INTO code_repository_publication_fences VALUES (
                'repo', 1, 'task', 1, 'worker', 1
            );
            INSERT INTO code_repository_reference_search_manifests VALUES (
                'scope-new', 2, 5, 1
            );
            ",
        )
        .expect("publication schema should initialize");
    connection
}

fn publication_receipt_count(connection: &Connection) -> usize {
    connection
        .query_row(
            "SELECT COUNT(*) FROM code_repository_publication_receipts",
            [],
            |row| row.get(0),
        )
        .expect("receipt count should load")
}

fn repository_state(connection: &Connection) -> (String, bool) {
    connection
        .query_row(
            "SELECT state, stale FROM code_repositories WHERE repository_id = 'repo'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("repository state should load")
}

fn active_scope(connection: &Connection) -> String {
    connection
        .query_row(
            "SELECT last_indexed_scope_id FROM code_repositories WHERE repository_id = 'repo'",
            [],
            |row| row.get(0),
        )
        .expect("active scope should load")
}

fn scope_stale(connection: &Connection) -> Option<bool> {
    connection
        .query_row(
            "SELECT stale FROM code_repository_scopes WHERE source_scope = 'scope-new'",
            [],
            |row| row.get(0),
        )
        .ok()
}

fn checkpoint_state(connection: &Connection) -> String {
    connection
        .query_row(
            "SELECT state FROM code_repository_index_checkpoints WHERE source_scope = 'scope-new'",
            [],
            |row| row.get(0),
        )
        .expect("checkpoint state should load")
}

fn software_stale(connection: &Connection) -> Option<bool> {
    connection
        .query_row(
            "SELECT stale FROM software_global_status WHERE source_scope = 'scope-new'",
            [],
            |row| row.get(0),
        )
        .ok()
}

fn published_commit_state(connection: &Connection) -> (String, String, String) {
    connection
        .query_row(
            "SELECT repository.last_indexed_commit, scope.resolved_commit_sha,
                    checkpoint.resolved_commit_sha
             FROM code_repositories repository
             JOIN code_repository_scopes scope
               ON scope.source_scope = repository.last_indexed_scope_id
             JOIN code_repository_index_checkpoints checkpoint
               ON checkpoint.source_scope = scope.source_scope
             WHERE repository.repository_id = 'repo'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("published commit metadata should load")
}

fn registration_filters(connection: &Connection) -> (String, String) {
    connection
        .query_row(
            "SELECT path_filters_json, language_filters_json
             FROM code_repositories WHERE repository_id = 'repo'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("registration filters should load")
}

fn commit_aliases(connection: &Connection) -> Vec<String> {
    let mut statement = connection
        .prepare(
            "SELECT resolved_commit_sha FROM code_repository_commit_scopes
             WHERE repository_id = 'repo' AND source_scope = 'scope-old'
             ORDER BY resolved_commit_sha ASC",
        )
        .expect("commit alias query should prepare");
    statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("commit aliases should query")
        .collect::<Result<Vec<_>, _>>()
        .expect("commit aliases should decode")
}

fn derived_fact_state(connection: &Connection) -> (usize, String, usize, usize, usize, usize) {
    connection
        .query_row(
            "SELECT repository.indexed_file_count, file.blob_hash,
                    repository.symbol_count, repository.reference_count,
                    repository.chunk_count, software.component_count
             FROM code_repositories repository
             JOIN code_repository_files file
               ON file.source_scope = repository.last_indexed_scope_id
             JOIN software_global_status software
               ON software.source_scope = repository.last_indexed_scope_id
             WHERE repository.repository_id = 'repo'",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .expect("derived fact state should load")
}

use std::path::PathBuf;

use rusqlite::params;

use crate::{
    domain::{CodeIndexResourceBudget, CodeRepositoryRegistration},
    storage::{CodeRepositoryStore, SqliteGraphStore},
};

#[tokio::test]
async fn scheduler_excludes_user_sets_counts_auto_sets_and_recovers_after_reopen() {
    let database = TemporaryDatabase::new();
    {
        let store = SqliteGraphStore::open(&database.path).expect("store should open");
        for (repository_id, alias) in [
            ("repo-user", "user"),
            ("repo-alias-user", "alias-user"),
            ("repo-auto", "auto"),
            ("repo-newer", "newer"),
        ] {
            store
                .upsert_code_repository(
                    CodeRepositoryRegistration::new(
                        repository_id,
                        alias,
                        format!("/tmp/{repository_id}"),
                        Vec::new(),
                        Vec::new(),
                    )
                    .expect("registration should validate"),
                )
                .await
                .expect("repository should register");
        }
        store
            .run(|connection| {
                insert_published_scope(connection, "repo-user", "scope-user", 10)?;
                insert_published_scope(connection, "repo-alias-user", "scope-alias-user", 5)?;
                insert_published_scope(connection, "repo-auto", "scope-auto", 20)?;
                insert_published_scope(connection, "repo-newer", "scope-newer", 30)?;
                insert_set_member(connection, "user-set", "team", "repo-user", "scope-user")?;
                insert_set_member(
                    connection,
                    "user-set-with-auto-alias",
                    "repo-alias-user-auto-workspace",
                    "repo-alias-user",
                    "scope-alias-user",
                )?;
                insert_set_member(
                    connection,
                    &super::super::super::workspace::workspace_set_id("repo-auto"),
                    "repo-auto-auto-workspace",
                    "repo-auto",
                    "scope-auto",
                )?;
                connection.execute(
                    "UPDATE code_repository_index_tasks
                     SET publication_generation = 7
                     WHERE repository_id = 'repo-auto'",
                    [],
                )?;
                Ok(())
            })
            .await
            .expect("retention fixtures should insert");

        let selected = store
            .schedule_code_repository_retention(1, 100)
            .await
            .expect("repository retention should schedule");
        assert_eq!(selected.as_deref(), Some("repo-auto"));

        let existing = store
            .schedule_code_repository_retention(99, 101)
            .await
            .expect("existing repository retention should be reused");
        assert_eq!(existing.as_deref(), Some("repo-auto"));
    }

    let reopened = SqliteGraphStore::open(&database.path).expect("store should reopen");
    let status = reopened
        .code_scope_retention("repo-auto".to_owned())
        .await
        .expect("retention status should recover");
    let job = status
        .repository_retention_job
        .expect("durable repository retention job should remain pending");
    assert_eq!(job.repository_id, "repo-auto");
    assert_eq!(job.initial_scope, "scope-auto");
    assert_eq!(job.cutoff_ms, 100);
    assert_eq!(job.cutoff_publication_generation, 7);
}

#[tokio::test]
async fn scheduler_scans_past_a_page_of_user_set_repositories() {
    let database = TemporaryDatabase::new();
    let store = SqliteGraphStore::open(&database.path).expect("store should open");
    for index in 0..65 {
        let repository_id = format!("repo-protected-{index:03}");
        store
            .upsert_code_repository(
                CodeRepositoryRegistration::new(
                    repository_id.clone(),
                    format!("protected-{index:03}"),
                    format!("/tmp/{repository_id}"),
                    Vec::new(),
                    Vec::new(),
                )
                .expect("registration should validate"),
            )
            .await
            .expect("protected repository should register");
    }
    for repository_id in ["repo-eligible-old", "repo-eligible-new"] {
        store
            .upsert_code_repository(
                CodeRepositoryRegistration::new(
                    repository_id,
                    repository_id,
                    format!("/tmp/{repository_id}"),
                    Vec::new(),
                    Vec::new(),
                )
                .expect("registration should validate"),
            )
            .await
            .expect("eligible repository should register");
    }
    store
        .run(|connection| {
            for index in 0..65 {
                let repository_id = format!("repo-protected-{index:03}");
                let scope = format!("scope-protected-{index:03}");
                insert_published_scope(connection, &repository_id, &scope, index as u64 + 1)?;
                insert_set_member(
                    connection,
                    &format!("set-protected-{index:03}"),
                    &format!("team-{index:03}"),
                    &repository_id,
                    &scope,
                )?;
            }
            insert_published_scope(connection, "repo-eligible-old", "scope-eligible-old", 1000)?;
            insert_published_scope(connection, "repo-eligible-new", "scope-eligible-new", 1001)?;
            Ok(())
        })
        .await
        .expect("retention fixtures should insert");

    let first_pass = store
        .schedule_code_repository_retention(1, 2000)
        .await
        .expect("first bounded candidate page should scan");
    assert!(first_pass.is_none());
    drop(store);

    let reopened = SqliteGraphStore::open(&database.path).expect("store should reopen");
    let selected = reopened
        .schedule_code_repository_retention(1, 2001)
        .await
        .expect("persisted candidate scan should resume");

    assert_eq!(selected.as_deref(), Some("repo-eligible-old"));
}

fn insert_published_scope(
    connection: &mut rusqlite::Connection,
    repository_id: &str,
    scope: &str,
    published_at_ms: u64,
) -> Result<(), crate::storage::StorageError> {
    connection.execute(
        "INSERT INTO code_repository_scopes (
             source_scope, repository_id, resolved_commit_sha, tree_hash,
             path_filters_json, language_filters_json, indexed_file_count,
             symbol_count, reference_count, chunk_count, stale, degraded_reason
         ) VALUES (?1, ?2, ?3, ?4, '[]', '[]', 1, 0, 0, 0, 0, NULL)",
        params![
            scope,
            repository_id,
            format!("commit-{scope}"),
            format!("tree-{scope}")
        ],
    )?;
    connection.execute(
        "UPDATE code_repositories
         SET last_indexed_scope_id = ?2, last_indexed_commit = ?3,
             tree_hash = ?4, state = 'fresh', stale = 0
         WHERE repository_id = ?1",
        params![
            repository_id,
            scope,
            format!("commit-{scope}"),
            format!("tree-{scope}")
        ],
    )?;
    connection.execute(
        "INSERT INTO code_repository_index_tasks (
             task_id, repository_id, alias, ref_selector, resolved_commit_sha, tree_hash,
             source_scope, path_filters_json, language_filters_json, mode_json, state,
             attempt_count, next_retry_at_ms, input_fingerprint, resource_budget_json,
             payload_json, created_at_ms, updated_at_ms
         ) VALUES (?1, ?2, ?2, ?3, ?3, ?4, ?5, '[]', '[]', '\"full\"',
                   'succeeded', 1, 0, ?1, ?6, '{}', ?7, ?7)",
        params![
            format!("task-{repository_id}"),
            repository_id,
            format!("commit-{scope}"),
            format!("tree-{scope}"),
            scope,
            serde_json::to_string(&CodeIndexResourceBudget::default())
                .map_err(|error| crate::storage::StorageError::InvalidInput(error.to_string()))?,
            published_at_ms,
        ],
    )?;
    Ok(())
}

fn insert_set_member(
    connection: &mut rusqlite::Connection,
    set_id: &str,
    alias: &str,
    repository_id: &str,
    scope: &str,
) -> Result<(), crate::storage::StorageError> {
    connection.execute(
        "INSERT INTO code_repository_sets (
             set_id, alias, description, default_ref_policy_json, created_at_ms, updated_at_ms
         ) VALUES (?1, ?2, NULL, '{}', 1, 1)",
        params![set_id, alias],
    )?;
    connection.execute(
        "INSERT INTO code_repository_set_members (
             set_id, repository_id, repository_alias, ref_selector,
             resolved_commit_sha, source_scope, path_filters_json,
             language_filters_json, priority
         ) VALUES (?1, ?2, ?2, ?3, ?3, ?3, '[]', '[]', 0)",
        params![set_id, repository_id, scope],
    )?;
    Ok(())
}

struct TemporaryDatabase {
    path: PathBuf,
}

impl TemporaryDatabase {
    fn new() -> Self {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time should follow Unix epoch")
            .as_nanos();
        Self {
            path: std::env::temp_dir().join(format!(
                "relay-knowledge-repository-retention-{}-{unique}.sqlite",
                std::process::id()
            )),
        }
    }
}

impl Drop for TemporaryDatabase {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm"] {
            let mut path = self.path.as_os_str().to_os_string();
            path.push(suffix);
            let _ = std::fs::remove_file(PathBuf::from(path));
        }
    }
}

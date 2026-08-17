//! Owns durable code-scope retention planning and pruning.

use std::collections::BTreeSet;

use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use crate::{
    domain::{CodeIndexMode, CodeRepositoryRetentionJobStatus, CodeScopeRetentionSummary},
    storage::{CodeScopeRetentionRequest, StorageError},
};

use super::super::{lifecycle::commit_scope, workspace};
use super::retention_gc;
use super::retention_publications::{
    CommitReference, latest_successful_incremental_base, latest_successful_incremental_base_since,
    scope_is_queryable, successful_scopes_since,
};
use super::worktree::{
    active_worktree_base_scopes, compatible_non_retiring_scopes_for_commit,
    worktree_overlay_base_commit, worktree_task_base_commit,
};

pub(super) const RETAIN_SUCCEEDED_TASK_AUDIT_ROWS: usize = 128;
pub(super) const RETAIN_FAILED_TASK_AUDIT_ROWS: usize = 64;
const MAX_SCOPE_STATUS_ROWS: usize = 64;
const MAX_EXPLICIT_RETENTION_PINS: usize = 512;
const MAX_PUBLICATION_CANDIDATE_ROWS: usize = RETAIN_SUCCEEDED_TASK_AUDIT_ROWS + 1;

pub(in crate::storage::sqlite::code) fn retention_status(
    connection: &mut Connection,
    repository_id: &str,
) -> Result<CodeScopeRetentionSummary, StorageError> {
    let active_scope = connection
        .query_row(
            "SELECT last_indexed_scope_id FROM code_repositories WHERE repository_id = ?1",
            params![repository_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten()
        .unwrap_or_default();
    let repository_retention = super::repository_retention_job(connection, repository_id)?;
    let plan = retention_plan(
        connection,
        repository_id,
        &active_scope,
        2,
        Vec::new(),
        repository_retention.as_ref(),
    )?;
    let retiring_jobs = retention_gc::jobs(connection, repository_id)?;
    let protected_aliases = protected_alias_commits(
        connection,
        repository_id,
        &plan.unfinished_tasks,
        plan.latest_incremental_base.as_ref(),
    )?;
    let audit_pending = finished_task_history_pending(connection, repository_id)?
        || commit_scope::repository_alias_pruning_pending(
            connection,
            repository_id,
            &protected_aliases,
        )?;
    Ok(summary_from_plan(
        repository_id,
        plan,
        Vec::new(),
        retiring_jobs,
        audit_pending,
        repository_retention,
    ))
}

pub(in crate::storage::sqlite::code) fn prune_scopes(
    connection: &mut Connection,
    request: CodeScopeRetentionRequest,
) -> Result<CodeScopeRetentionSummary, StorageError> {
    prune_scopes_with_retained(connection, request, Vec::new())
}

pub(in crate::storage::sqlite::code) fn prune_scopes_with_retained(
    connection: &mut Connection,
    request: CodeScopeRetentionRequest,
    extra_retained_scopes: Vec<String>,
) -> Result<CodeScopeRetentionSummary, StorageError> {
    let explicit_repository_retention = match (
        request.repository_retention_cutoff_ms,
        request.repository_retention_initial_scope.clone(),
    ) {
        (Some(cutoff_ms), Some(initial_scope)) => Some(CodeRepositoryRetentionJobStatus {
            repository_id: request.repository_id.clone(),
            initial_scope,
            cutoff_ms,
            cutoff_publication_generation: request
                .repository_retention_cutoff_generation
                .unwrap_or_default(),
            phase: "retiring_scopes".to_owned(),
            created_at_ms: cutoff_ms,
            updated_at_ms: cutoff_ms,
            last_error: None,
        }),
        (None, None) if request.repository_retention_cutoff_generation.is_none() => None,
        _ => {
            return Err(StorageError::InvalidInput(
                "repository retention cutoff and initial scope must be provided together"
                    .to_owned(),
            ));
        }
    };
    let persisted_repository_retention =
        super::repository_retention_job(connection, &request.repository_id)?;
    let complete_repository_retention =
        explicit_repository_retention.is_none() && persisted_repository_retention.is_some();
    let repository_retention = persisted_repository_retention.or(explicit_repository_retention);
    run_retention_pass(
        connection,
        &request.repository_id,
        &request.active_scope,
        request.retain_recent_successful_scopes,
        extra_retained_scopes,
        repository_retention,
        complete_repository_retention,
    )
}

fn retention_plan(
    connection: &Connection,
    repository_id: &str,
    active_scope: &str,
    retain_recent_successful_scopes: usize,
    extra_retained_scopes: Vec<String>,
    repository_retention: Option<&CodeRepositoryRetentionJobStatus>,
) -> Result<RetentionPlan, StorageError> {
    let mut retained = BTreeSet::new();
    let current_active_scope = connection
        .query_row(
            "SELECT last_indexed_scope_id FROM code_repositories WHERE repository_id = ?1",
            params![repository_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten();
    let user_set_scopes =
        user_repository_set_member_scopes(connection, repository_id, MAX_EXPLICIT_RETENTION_PINS)?;
    let repository_set_protected =
        repository_retention.is_some() && !user_set_scopes.scopes.is_empty();
    let user_pins_truncated = user_set_scopes.truncated;
    retained.extend(user_set_scopes.scopes);

    let active_repository_retention = repository_retention.filter(|_| !repository_set_protected);
    let (latest_incremental_base, publication_history_incomplete) =
        if let Some(repository_retention) = active_repository_retention {
            let mut protected_publications = successful_scopes_since(
                connection,
                repository_id,
                repository_retention.cutoff_ms,
                repository_retention.cutoff_publication_generation,
                &repository_retention.initial_scope,
                MAX_SCOPE_STATUS_ROWS,
            )?;
            if let Some(current_active_scope) = current_active_scope
                .as_ref()
                .filter(|scope| *scope != &repository_retention.initial_scope)
            {
                protected_publications
                    .scopes
                    .push(current_active_scope.clone());
            }
            protected_publications.scopes.sort();
            protected_publications.scopes.dedup();
            for scope in &protected_publications.scopes {
                retained.insert(scope.clone());
                retained.extend(active_worktree_base_scopes(
                    connection,
                    repository_id,
                    scope,
                )?);
            }
            (
                latest_successful_incremental_base_since(
                    connection,
                    repository_id,
                    repository_retention.cutoff_ms,
                    repository_retention.cutoff_publication_generation,
                    &repository_retention.initial_scope,
                    current_active_scope
                        .as_deref()
                        .filter(|scope| *scope != repository_retention.initial_scope),
                )?,
                protected_publications.truncated,
            )
        } else {
            if let Some(current_active_scope) = &current_active_scope {
                retained.insert(current_active_scope.clone());
            }
            if !active_scope.is_empty() {
                retained.insert(active_scope.to_owned());
            }
            let mut active_scopes = BTreeSet::new();
            if !active_scope.is_empty() {
                active_scopes.insert(active_scope.to_owned());
            }
            if let Some(current_active_scope) = current_active_scope {
                active_scopes.insert(current_active_scope);
            }
            for scope in active_scopes {
                retained.extend(active_worktree_base_scopes(
                    connection,
                    repository_id,
                    &scope,
                )?);
            }
            let recent = recent_successful_scopes(
                connection,
                repository_id,
                retain_recent_successful_scopes,
            )?;
            for scope in recent.scopes {
                retained.insert(scope);
            }
            (
                latest_successful_incremental_base(connection, repository_id)?,
                recent.incomplete,
            )
        };
    let unfinished_tasks = unfinished_tasks(connection, repository_id)?;
    for scope in unfinished_task_scopes(connection, repository_id, &unfinished_tasks)? {
        retained.insert(scope);
    }
    if let Some(base) = &latest_incremental_base {
        retained.extend(scopes_for_commit(
            connection,
            repository_id,
            &base.resolved_commit_sha,
            &base.path_filters_json,
            &base.language_filters_json,
        )?);
    }
    for scope in extra_retained_scopes {
        retained.insert(scope);
    }
    let (mut prunable, scope_listing_truncated) =
        prunable_scopes(connection, repository_id, &retained, MAX_SCOPE_STATUS_ROWS)?;
    if publication_history_incomplete || repository_set_protected {
        // A legacy publication history can exceed the fixed candidate window.
        // Audit compaction still advances this pass, but scope retirement waits
        // until the newest distinct publications can be proven without a scan.
        prunable.clear();
    }
    Ok(RetentionPlan {
        retained,
        prunable,
        scope_listing_truncated: scope_listing_truncated
            || user_pins_truncated
            || publication_history_incomplete,
        publication_history_incomplete,
        repository_set_protected,
        unfinished_tasks,
        latest_incremental_base,
    })
}

fn run_retention_pass(
    connection: &mut Connection,
    repository_id: &str,
    active_scope: &str,
    retain_recent_successful_scopes: usize,
    extra_retained_scopes: Vec<String>,
    repository_retention: Option<CodeRepositoryRetentionJobStatus>,
    complete_repository_retention: bool,
) -> Result<CodeScopeRetentionSummary, StorageError> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let plan = retention_plan(
        &transaction,
        repository_id,
        active_scope,
        retain_recent_successful_scopes,
        extra_retained_scopes,
        repository_retention.as_ref(),
    )?;
    let now_ms = current_timestamp_ms();
    let had_job = !retention_gc::jobs(&transaction, repository_id)?.is_empty();
    let mut pruned = Vec::new();
    if had_job {
        if let Some(scope) = retention_gc::process_one(&transaction, repository_id, now_ms)? {
            pruned.push(scope);
        }
    } else if let Some(scope) = plan.prunable.first() {
        // The protected set and logical retirement are committed atomically under
        // BEGIN IMMEDIATE, so a concurrent writer cannot publish a new pin between them.
        retention_gc::schedule(&transaction, repository_id, scope, now_ms)?;
    }
    // Each maintenance category receives one fixed batch on every pass. Scope
    // retirement still advances at most one job phase, while a large scope can
    // no longer starve terminal-task or commit-alias audit retention.
    prune_finished_task_history(&transaction, repository_id, None)?;
    let protected = protected_alias_commits(
        &transaction,
        repository_id,
        &plan.unfinished_tasks,
        plan.latest_incremental_base.as_ref(),
    )?;
    commit_scope::prune_repository_aliases(&transaction, repository_id, &protected)?;
    let retiring_jobs = retention_gc::jobs(&transaction, repository_id)?;
    let protected_aliases = protected_alias_commits(
        &transaction,
        repository_id,
        &plan.unfinished_tasks,
        plan.latest_incremental_base.as_ref(),
    )?;
    let audit_pending = finished_task_history_pending(&transaction, repository_id)?
        || commit_scope::repository_alias_pruning_pending(
            &transaction,
            repository_id,
            &protected_aliases,
        )?;
    let repository_retention_complete = plan.repository_set_protected
        || (complete_repository_retention
            && !plan.publication_history_incomplete
            && plan.prunable.is_empty()
            && retiring_jobs.is_empty());
    let repository_retention = if repository_retention_complete {
        if let Some(job) = &repository_retention {
            super::complete_repository_retention(&transaction, repository_id, job.cutoff_ms)?;
        }
        None
    } else {
        repository_retention
            .map(|mut job| {
                let (phase, last_error) = retiring_jobs.first().map_or_else(
                    || ("retiring_scopes".to_owned(), None),
                    |scope_job| {
                        (
                            format!("scope_gc:{}", scope_job.phase),
                            scope_job.last_error.clone(),
                        )
                    },
                );
                super::update_repository_retention(
                    &transaction,
                    repository_id,
                    job.cutoff_ms,
                    &phase,
                    last_error.as_deref(),
                    now_ms,
                )?;
                job.phase = phase;
                job.updated_at_ms = now_ms;
                job.last_error = last_error;
                Ok::<_, StorageError>(job)
            })
            .transpose()?
    };
    let summary = summary_from_plan(
        repository_id,
        plan,
        pruned,
        retiring_jobs,
        audit_pending,
        repository_retention,
    );
    transaction.commit()?;
    Ok(summary)
}

fn summary_from_plan(
    repository_id: &str,
    plan: RetentionPlan,
    pruned: Vec<String>,
    retiring_jobs: Vec<crate::domain::CodeScopeRetirementJobStatus>,
    audit_pending: bool,
    repository_retention_job: Option<CodeRepositoryRetentionJobStatus>,
) -> CodeScopeRetentionSummary {
    let maintenance_pending = !plan.prunable.is_empty()
        || !retiring_jobs.is_empty()
        || audit_pending
        || repository_retention_job.is_some();
    let mut retained_scopes = plan.retained.into_iter().collect::<Vec<_>>();
    let retained_scope_count = retained_scopes.len();
    let retained_truncated = retained_scopes.len() > MAX_SCOPE_STATUS_ROWS;
    retained_scopes.truncate(MAX_SCOPE_STATUS_ROWS);
    CodeScopeRetentionSummary {
        repository_id: repository_id.to_owned(),
        retained_scope_count,
        prunable_scope_count: plan.prunable.len(),
        pruned_scope_count: pruned.len(),
        scope_listing_truncated: plan.scope_listing_truncated || retained_truncated,
        retiring_job_count: retiring_jobs.len(),
        maintenance_pending,
        retained_scopes,
        prunable_scopes: plan.prunable,
        pruned_scopes: pruned,
        retiring_jobs,
        repository_retention_job,
    }
}

struct RetentionPlan {
    retained: BTreeSet<String>,
    prunable: Vec<String>,
    scope_listing_truncated: bool,
    publication_history_incomplete: bool,
    repository_set_protected: bool,
    unfinished_tasks: Vec<UnfinishedTask>,
    latest_incremental_base: Option<CommitReference>,
}

pub(super) struct ScopePage {
    pub(super) scopes: Vec<String>,
    pub(super) truncated: bool,
}

struct PublishedScope {
    source_scope: String,
    publication_generation: u64,
    published_at_ms: u64,
}

struct PublishedScopePage {
    scopes: Vec<String>,
    incomplete: bool,
}

fn prunable_scopes(
    connection: &Connection,
    repository_id: &str,
    retained: &BTreeSet<String>,
    limit: usize,
) -> Result<(Vec<String>, bool), StorageError> {
    let auto_set_id = workspace::workspace_set_id(repository_id);
    let query_limit = limit.saturating_add(1);
    let mut scopes = prunable_scope_rows(
        connection,
        "code_repository_scopes",
        "AND candidate.retiring = 0",
        repository_id,
        &auto_set_id,
        retained,
        query_limit,
    )?;
    scopes.extend(prunable_scope_rows(
        connection,
        "code_repository_index_checkpoints",
        "AND NOT EXISTS (
             SELECT 1 FROM code_repository_scopes scope
             WHERE scope.source_scope = candidate.source_scope
         )",
        repository_id,
        &auto_set_id,
        retained,
        query_limit,
    )?);
    scopes.sort();
    scopes.dedup();
    let truncated = scopes.len() > limit;
    scopes.truncate(limit);
    Ok((scopes, truncated))
}

fn prunable_scope_rows(
    connection: &Connection,
    table: &'static str,
    table_filter: &'static str,
    repository_id: &str,
    auto_set_id: &str,
    retained: &BTreeSet<String>,
    limit: usize,
) -> Result<Vec<String>, StorageError> {
    use rusqlite::types::Value;

    let mut values = vec![
        Value::Text(repository_id.to_owned()),
        Value::Text(repository_id.to_owned()),
        Value::Text(auto_set_id.to_owned()),
    ];
    let retained_clause = if retained.is_empty() {
        String::new()
    } else {
        values.extend(retained.iter().cloned().map(Value::Text));
        let placeholders = std::iter::repeat_n("?", retained.len())
            .collect::<Vec<_>>()
            .join(", ");
        format!("AND candidate.source_scope NOT IN ({placeholders})")
    };
    values.push(Value::Integer(limit as i64));
    let query = format!(
        "SELECT candidate.source_scope
             FROM {table} candidate
             WHERE candidate.repository_id = ?
               {table_filter}
               AND NOT EXISTS (
                 SELECT 1 FROM code_repository_scope_gc_jobs job
                 WHERE job.source_scope = candidate.source_scope
             )
               AND NOT EXISTS (
                   SELECT 1
                   FROM code_repository_set_members member
                   WHERE member.repository_id = ?
                     AND member.source_scope = candidate.source_scope
                     AND member.set_id <> ?
               )
               {retained_clause}
             ORDER BY candidate.source_scope ASC
             LIMIT ?"
    );
    let mut statement = connection.prepare(&query)?;
    statement
        .query_map(rusqlite::params_from_iter(values), |row| {
            row.get::<_, String>(0)
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn recent_successful_scopes(
    connection: &Connection,
    repository_id: &str,
    limit: usize,
) -> Result<PublishedScopePage, StorageError> {
    if limit > MAX_SCOPE_STATUS_ROWS {
        return Err(StorageError::InvalidInput(format!(
            "retain_recent_successful_scopes exceeds the bounded maximum of {MAX_SCOPE_STATUS_ROWS}"
        )));
    }
    if limit == 0 {
        return Ok(PublishedScopePage {
            scopes: Vec::new(),
            incomplete: false,
        });
    }
    let mut tasks = successful_task_candidates(connection, repository_id)?;
    let history_truncated = tasks.len() > MAX_PUBLICATION_CANDIDATE_ROWS;
    tasks.truncate(MAX_PUBLICATION_CANDIDATE_ROWS);
    tasks.extend(completed_checkpoint_candidates(
        connection,
        repository_id,
        limit.saturating_add(1),
    )?);
    let mut queryable = Vec::with_capacity(tasks.len());
    for candidate in tasks {
        if scope_is_queryable(connection, repository_id, &candidate.source_scope)? {
            queryable.push(candidate);
        }
    }
    let mut tasks = queryable;
    tasks.sort_by(|left, right| {
        (
            right.publication_generation > 0,
            right.publication_generation,
            right.published_at_ms,
            &right.source_scope,
        )
            .cmp(&(
                left.publication_generation > 0,
                left.publication_generation,
                left.published_at_ms,
                &left.source_scope,
            ))
    });
    let mut seen_scopes = BTreeSet::new();
    tasks.retain(|candidate| seen_scopes.insert(candidate.source_scope.clone()));
    let incomplete = history_truncated && tasks.len() < limit;
    tasks.truncate(limit);
    Ok(PublishedScopePage {
        scopes: tasks
            .into_iter()
            .map(|candidate| candidate.source_scope)
            .collect(),
        incomplete,
    })
}

fn successful_task_candidates(
    connection: &Connection,
    repository_id: &str,
) -> Result<Vec<PublishedScope>, StorageError> {
    let mut statement = connection.prepare(
        "SELECT source_scope, publication_generation, updated_at_ms
         FROM code_repository_index_tasks
              INDEXED BY code_repository_index_tasks_publication_retention
         WHERE repository_id = ?1 AND state = 'succeeded'
         ORDER BY publication_generation DESC, updated_at_ms DESC,
                  created_at_ms DESC, task_id DESC
         LIMIT ?2",
    )?;
    statement
        .query_map(
            params![repository_id, MAX_PUBLICATION_CANDIDATE_ROWS + 1],
            |row| {
                Ok(PublishedScope {
                    source_scope: row.get(0)?,
                    publication_generation: row.get(1)?,
                    published_at_ms: row.get(2)?,
                })
            },
        )?
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn completed_checkpoint_candidates(
    connection: &Connection,
    repository_id: &str,
    limit: usize,
) -> Result<Vec<PublishedScope>, StorageError> {
    let mut statement = connection.prepare(
        "SELECT source_scope, updated_at_ms
         FROM code_repository_index_checkpoints
              INDEXED BY code_repository_index_checkpoints_publication_retention
         WHERE repository_id = ?1 AND state IN ('complete', 'completed')
         ORDER BY updated_at_ms DESC, source_scope DESC
         LIMIT ?2",
    )?;
    statement
        .query_map(params![repository_id, limit], |row| {
            Ok(PublishedScope {
                source_scope: row.get(0)?,
                publication_generation: 0,
                published_at_ms: row.get(1)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn unfinished_tasks(
    connection: &Connection,
    repository_id: &str,
) -> Result<Vec<UnfinishedTask>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT source_scope, ref_selector, resolved_commit_sha,
               path_filters_json, language_filters_json, mode_json
        FROM code_repository_index_tasks
        WHERE repository_id = ?1 AND state IN ('queued', 'running', 'retrying')
        ",
    )?;
    let rows = statement.query_map(params![repository_id], |row| {
        Ok(UnfinishedTask {
            source_scope: row.get(0)?,
            ref_selector: row.get(1)?,
            resolved_commit_sha: row.get(2)?,
            path_filters_json: row.get(3)?,
            language_filters_json: row.get(4)?,
            mode_json: row.get(5)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn unfinished_task_scopes(
    connection: &Connection,
    repository_id: &str,
    tasks: &[UnfinishedTask],
) -> Result<Vec<String>, StorageError> {
    let mut retained = BTreeSet::new();
    for task in tasks {
        retained.insert(task.source_scope.clone());
        for base_ref in task.base_refs()? {
            retained.extend(scopes_for_commit(
                connection,
                repository_id,
                &base_ref,
                &task.path_filters_json,
                &task.language_filters_json,
            )?);
        }
    }

    Ok(retained.into_iter().collect())
}

fn scopes_for_commit(
    connection: &Connection,
    repository_id: &str,
    resolved_commit_sha: &str,
    path_filters_json: &str,
    language_filters_json: &str,
) -> Result<Vec<String>, StorageError> {
    compatible_non_retiring_scopes_for_commit(
        connection,
        repository_id,
        resolved_commit_sha,
        path_filters_json,
        language_filters_json,
    )
}

fn protected_alias_commits(
    connection: &Connection,
    repository_id: &str,
    unfinished_tasks: &[UnfinishedTask],
    latest_incremental_base: Option<&CommitReference>,
) -> Result<BTreeSet<String>, StorageError> {
    let mut protected = BTreeSet::new();
    if let Some(active_commit) = connection
        .query_row(
            "SELECT last_indexed_commit FROM code_repositories WHERE repository_id = ?1",
            params![repository_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten()
    {
        if let Some(base_commit) = worktree_overlay_base_commit(&active_commit) {
            protected.insert(base_commit.to_owned());
        }
        protected.insert(active_commit);
    }
    if let Some(base) = latest_incremental_base {
        protected.insert(base.resolved_commit_sha.clone());
    }
    for task in unfinished_tasks {
        protected.insert(task.resolved_commit_sha.clone());
        protected.extend(task.base_refs()?);
    }
    Ok(protected)
}

struct UnfinishedTask {
    source_scope: String,
    ref_selector: String,
    resolved_commit_sha: String,
    path_filters_json: String,
    language_filters_json: String,
    mode_json: String,
}

impl UnfinishedTask {
    fn base_refs(&self) -> Result<Vec<String>, StorageError> {
        let mode = serde_json::from_str::<CodeIndexMode>(&self.mode_json).map_err(|error| {
            StorageError::InvalidInput(format!(
                "unfinished code index task has invalid mode: {error}"
            ))
        })?;
        let mut refs = BTreeSet::new();
        match mode {
            CodeIndexMode::Incremental { base_ref, .. } => {
                refs.insert(base_ref);
            }
            CodeIndexMode::WorktreeOverlay => {
                refs.insert(self.ref_selector.clone());
                if let Some(base_ref) =
                    worktree_task_base_commit(&self.resolved_commit_sha, &self.ref_selector)
                {
                    refs.insert(base_ref.to_owned());
                }
            }
            CodeIndexMode::Full => {}
        }
        Ok(refs.into_iter().collect())
    }
}

pub(super) fn prune_finished_task_history(
    transaction: &rusqlite::Transaction<'_>,
    repository_id: &str,
    protected_task_id: Option<&str>,
) -> Result<bool, StorageError> {
    let deleted_succeeded = transaction.execute(
        "
        DELETE FROM code_repository_index_tasks
        WHERE task_id <> ?4 AND task_id IN (
            SELECT candidate.task_id
            FROM (
                SELECT task_id, source_scope, publication_generation,
                       updated_at_ms, created_at_ms
                FROM code_repository_index_tasks
                     INDEXED BY code_repository_index_tasks_publication_retention
                WHERE repository_id = ?1 AND state = 'succeeded'
                ORDER BY publication_generation DESC, updated_at_ms DESC,
                         created_at_ms DESC, task_id DESC
                LIMIT ?3 OFFSET ?2
            ) candidate
            WHERE NOT EXISTS (
                      SELECT 1
                      FROM code_repository_scopes scope
                      WHERE scope.repository_id = ?1
                        AND scope.source_scope = candidate.source_scope
                  )
               OR EXISTS (
                      SELECT 1
                      FROM code_repository_index_tasks newer
                      WHERE newer.repository_id = ?1
                        AND newer.state = 'succeeded'
                        AND newer.source_scope = candidate.source_scope
                        AND (
                            newer.publication_generation > candidate.publication_generation
                            OR (
                                newer.publication_generation = candidate.publication_generation
                                AND newer.updated_at_ms > candidate.updated_at_ms
                            )
                            OR (
                                newer.publication_generation = candidate.publication_generation
                                AND newer.updated_at_ms = candidate.updated_at_ms
                                AND newer.created_at_ms > candidate.created_at_ms
                            )
                            OR (
                                newer.publication_generation = candidate.publication_generation
                                AND newer.updated_at_ms = candidate.updated_at_ms
                                AND newer.created_at_ms = candidate.created_at_ms
                                AND newer.task_id > candidate.task_id
                            )
                        )
                  )
            ORDER BY candidate.task_id
            LIMIT ?3
        )
        ",
        params![
            repository_id,
            RETAIN_SUCCEEDED_TASK_AUDIT_ROWS,
            retention_gc::GC_ROW_BATCH_SIZE,
            protected_task_id.unwrap_or_default(),
        ],
    )?;
    let deleted_failed = transaction.execute(
        "
        DELETE FROM code_repository_index_tasks
        WHERE task_id <> ?4 AND task_id IN (
            SELECT task_id
            FROM code_repository_index_tasks
            WHERE repository_id = ?1
              AND state IN ('failed', 'dead_letter', 'cancelled')
            ORDER BY updated_at_ms DESC, created_at_ms DESC, task_id DESC
            LIMIT ?3
            OFFSET ?2
        )
        ",
        params![
            repository_id,
            RETAIN_FAILED_TASK_AUDIT_ROWS,
            retention_gc::GC_ROW_BATCH_SIZE,
            protected_task_id.unwrap_or_default(),
        ],
    )?;
    Ok(deleted_succeeded > 0 || deleted_failed > 0)
}

fn finished_task_history_pending(
    connection: &Connection,
    repository_id: &str,
) -> Result<bool, StorageError> {
    let succeeded = connection
        .query_row(
            "SELECT 1
             FROM (
                 SELECT task_id, source_scope, publication_generation,
                        updated_at_ms, created_at_ms
                 FROM code_repository_index_tasks
                      INDEXED BY code_repository_index_tasks_publication_retention
                 WHERE repository_id = ?1 AND state = 'succeeded'
                 ORDER BY publication_generation DESC, updated_at_ms DESC,
                          created_at_ms DESC, task_id DESC
                 LIMIT ?3 OFFSET ?2
             ) candidate
             WHERE NOT EXISTS (
                       SELECT 1
                       FROM code_repository_scopes scope
                       WHERE scope.repository_id = ?1
                         AND scope.source_scope = candidate.source_scope
                   )
                OR EXISTS (
                       SELECT 1
                       FROM code_repository_index_tasks newer
                       WHERE newer.repository_id = ?1
                         AND newer.state = 'succeeded'
                         AND newer.source_scope = candidate.source_scope
                         AND (
                             newer.publication_generation > candidate.publication_generation
                             OR (
                                 newer.publication_generation = candidate.publication_generation
                                 AND newer.updated_at_ms > candidate.updated_at_ms
                             )
                             OR (
                                 newer.publication_generation = candidate.publication_generation
                                 AND newer.updated_at_ms = candidate.updated_at_ms
                                 AND newer.created_at_ms > candidate.created_at_ms
                             )
                             OR (
                                 newer.publication_generation = candidate.publication_generation
                                 AND newer.updated_at_ms = candidate.updated_at_ms
                                 AND newer.created_at_ms = candidate.created_at_ms
                                 AND newer.task_id > candidate.task_id
                             )
                         )
                   )
             LIMIT 1",
            params![
                repository_id,
                RETAIN_SUCCEEDED_TASK_AUDIT_ROWS,
                retention_gc::GC_ROW_BATCH_SIZE
            ],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if succeeded {
        return Ok(true);
    }
    connection
        .query_row(
            "SELECT 1
             FROM code_repository_index_tasks
             WHERE repository_id = ?1
               AND state IN ('failed', 'dead_letter', 'cancelled')
             ORDER BY updated_at_ms DESC, created_at_ms DESC, task_id DESC
             LIMIT 1 OFFSET ?2",
            params![repository_id, RETAIN_FAILED_TASK_AUDIT_ROWS],
            |_| Ok(()),
        )
        .optional()
        .map(|row| row.is_some())
        .map_err(StorageError::from)
}

fn current_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn user_repository_set_member_scopes(
    connection: &Connection,
    repository_id: &str,
    limit: usize,
) -> Result<ScopePage, StorageError> {
    let auto_set_id = workspace::workspace_set_id(repository_id);
    let mut statement = connection.prepare(
        "SELECT DISTINCT source_scope
         FROM code_repository_set_members
         WHERE repository_id = ?1 AND set_id <> ?2
         ORDER BY source_scope
         LIMIT ?3",
    )?;
    let mut scopes = statement
        .query_map(
            params![repository_id, auto_set_id, limit.saturating_add(1)],
            |row| row.get::<_, String>(0),
        )?
        .collect::<Result<Vec<_>, _>>()?;
    let truncated = scopes.len() > limit;
    scopes.truncate(limit);
    Ok(ScopePage { scopes, truncated })
}

#[cfg(test)]
#[path = "retention_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "retention_fairness_tests.rs"]
mod fairness_tests;

//! Bounded, durable scope-retirement state machine.

#[cfg(test)]
#[path = "retention_gc_tests.rs"]
mod tests;

use rusqlite::{
    Connection, OptionalExtension, Transaction, params, params_from_iter, types::Value,
};

use crate::{domain::CodeScopeRetirementJobStatus, storage::StorageError};

pub(in crate::storage::sqlite::code) const GC_ROW_BATCH_SIZE: usize = 512;
const SEARCH_OWNER_BATCH_SIZE: usize = GC_ROW_BATCH_SIZE / 2;
const PHASES: &[&str] = &[
    "workspace_edges",
    "workspace_mappings",
    "workspace_members",
    "workspace_overlay",
    "catalog_route",
    "search_documents",
    "search_orphans",
    "reference_search_groups",
    "reference_search_manifest",
    "path_tombstones",
    "file_diagnostics",
    "chunks",
    "calls",
    "routes",
    "feature_flags",
    "framework_edges",
    "framework_nodes",
    "dependencies",
    "imports",
    "references",
    "symbols",
    "files",
    "software_components",
    "software_dependency_usages",
    "software_sdk_usages",
    "software_files",
    "software_topics",
    "software_relationships",
    "software_global_status",
    "software_build_targets",
    "software_iac_resources",
    "software_design_elements",
    "software_entities",
    "software_statements",
    "software_ontology_diagnostics",
    "business_mappings",
    "business_term_aliases",
    "business_terms",
    "business_domains",
    "business_knowledge_status",
    "commit_scopes",
    "index_batch_staging",
    "index_task_history",
    "checkpoint",
    "scope_metadata",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ScopeGcPhase(usize);

impl ScopeGcPhase {
    const fn initial() -> Self {
        Self(0)
    }

    fn decode(value: &str) -> Result<Self, StorageError> {
        PHASES
            .iter()
            .position(|candidate| *candidate == value)
            .map(Self)
            .ok_or_else(|| {
                StorageError::InvalidInput(format!("scope GC job has unknown phase '{value}'"))
            })
    }

    const fn name(self) -> &'static str {
        PHASES[self.0]
    }

    const fn next(self) -> Option<Self> {
        if self.0 + 1 < PHASES.len() {
            Some(Self(self.0 + 1))
        } else {
            None
        }
    }

    fn is_search_orphans(self) -> bool {
        self.name() == "search_orphans"
    }
}

pub(super) fn jobs(
    connection: &Connection,
    repository_id: &str,
) -> Result<Vec<CodeScopeRetirementJobStatus>, StorageError> {
    let mut statement = connection.prepare(
        "SELECT repository_id, source_scope, phase, deleted_rows, updated_at_ms, last_error
         FROM code_repository_scope_gc_jobs
         WHERE repository_id = ?1
         ORDER BY updated_at_ms ASC, source_scope ASC",
    )?;
    let rows = statement.query_map(params![repository_id], |row| {
        Ok(CodeScopeRetirementJobStatus {
            repository_id: row.get(0)?,
            source_scope: row.get(1)?,
            phase: row.get(2)?,
            deleted_rows: row.get(3)?,
            updated_at_ms: row.get(4)?,
            last_error: row.get(5)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

pub(super) fn schedule(
    transaction: &Transaction<'_>,
    repository_id: &str,
    source_scope: &str,
    now_ms: u64,
) -> Result<(), StorageError> {
    transaction.execute(
        "UPDATE code_repository_scopes
         SET retiring = 1
         WHERE repository_id = ?1 AND source_scope = ?2 AND retiring = 0",
        params![repository_id, source_scope],
    )?;
    transaction.execute(
        "INSERT OR IGNORE INTO code_repository_scope_gc_jobs (
             source_scope, repository_id, phase, search_rowid_cursor, deleted_rows,
             created_at_ms, updated_at_ms, last_error
         ) VALUES (?1, ?2, ?3, NULL, 0, ?4, ?4, NULL)",
        params![
            source_scope,
            repository_id,
            ScopeGcPhase::initial().name(),
            now_ms
        ],
    )?;
    transaction.execute(
        "UPDATE code_repositories
         SET last_indexed_scope_id = NULL,
             last_indexed_commit = NULL,
             tree_hash = NULL,
             state = 'registered',
             indexed_file_count = 0,
             symbol_count = 0,
             reference_count = 0,
             chunk_count = 0,
             stale = 1,
             degraded_reason = NULL
         WHERE repository_id = ?1 AND last_indexed_scope_id = ?2",
        params![repository_id, source_scope],
    )?;
    Ok(())
}

pub(super) fn process_one(
    transaction: &Transaction<'_>,
    repository_id: &str,
    now_ms: u64,
) -> Result<Option<String>, StorageError> {
    let Some((source_scope, phase, search_rowid_cursor)) = transaction
        .query_row(
            "SELECT source_scope, phase, search_rowid_cursor
             FROM code_repository_scope_gc_jobs
             WHERE repository_id = ?1
             ORDER BY updated_at_ms ASC, source_scope ASC
             LIMIT 1",
            params![repository_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                ))
            },
        )
        .optional()?
    else {
        return Ok(None);
    };

    let result = ScopeGcPhase::decode(&phase).and_then(|phase| {
        let progress = if phase.is_search_orphans() {
            delete_search_orphan_batch(transaction, &source_scope, search_rowid_cursor)
        } else {
            delete_phase_batch(transaction, repository_id, &source_scope, phase).map(
                |(deleted, has_more)| PhaseProgress {
                    deleted,
                    has_more,
                    search_rowid_cursor: None,
                },
            )
        }?;
        Ok((phase, progress))
    });
    let (phase, progress) = match result {
        Ok(result) => result,
        Err(error) => {
            transaction.execute(
                "UPDATE code_repository_scope_gc_jobs
                 SET updated_at_ms = ?2, last_error = ?3
                 WHERE source_scope = ?1",
                params![source_scope, now_ms, error.to_string()],
            )?;
            return Ok(None);
        }
    };
    let next = (!progress.has_more).then(|| phase.next()).flatten();
    if progress.has_more {
        update_progress(
            transaction,
            &source_scope,
            phase,
            progress.deleted,
            progress.search_rowid_cursor,
            now_ms,
        )?;
        return Ok(None);
    }
    if let Some(next) = next {
        update_progress(
            transaction,
            &source_scope,
            next,
            progress.deleted,
            None,
            now_ms,
        )?;
        return Ok(None);
    }

    transaction.execute(
        "DELETE FROM code_repository_scope_gc_jobs WHERE source_scope = ?1",
        params![source_scope],
    )?;
    Ok(Some(source_scope))
}

pub(in crate::storage::sqlite::code) fn reject_retiring_scope(
    connection: &Connection,
    source_scope: &str,
) -> Result<(), StorageError> {
    let retiring = connection.query_row(
        "SELECT EXISTS (
             SELECT 1 FROM code_repository_scope_gc_jobs WHERE source_scope = ?1
             UNION ALL
             SELECT 1 FROM code_repository_scopes
             WHERE source_scope = ?1 AND retiring != 0
         )",
        params![source_scope],
        |row| row.get::<_, bool>(0),
    )?;
    if retiring {
        return Err(StorageError::InvalidInput(format!(
            "code repository source scope '{source_scope}' is retiring; wait for bounded maintenance to complete before rebuilding or querying it"
        )));
    }
    Ok(())
}

fn update_progress(
    transaction: &Transaction<'_>,
    source_scope: &str,
    phase: ScopeGcPhase,
    deleted: usize,
    search_rowid_cursor: Option<i64>,
    now_ms: u64,
) -> Result<(), StorageError> {
    transaction.execute(
        "UPDATE code_repository_scope_gc_jobs
         SET phase = ?2, deleted_rows = deleted_rows + ?3,
             updated_at_ms = ?4, last_error = NULL,
             search_rowid_cursor = ?5
         WHERE source_scope = ?1",
        params![
            source_scope,
            phase.name(),
            deleted,
            now_ms,
            search_rowid_cursor
        ],
    )?;
    Ok(())
}

struct PhaseProgress {
    deleted: usize,
    has_more: bool,
    search_rowid_cursor: Option<i64>,
}

fn delete_phase_batch(
    transaction: &Transaction<'_>,
    repository_id: &str,
    source_scope: &str,
    phase: ScopeGcPhase,
) -> Result<(usize, bool), StorageError> {
    match phase.name() {
        "workspace_edges" => delete_cross_edges(transaction, source_scope),
        "workspace_mappings" => delete_table_batch(
            transaction,
            "code_workspace_package_mappings",
            "source_scope",
            source_scope,
        ),
        "workspace_members" => delete_table_batch(
            transaction,
            "code_repository_set_members",
            "source_scope",
            source_scope,
        ),
        "workspace_overlay" => delete_workspace_overlay(transaction, repository_id),
        "catalog_route" => delete_catalog_route(transaction, source_scope),
        "search_documents" => delete_search_batch(transaction, source_scope),
        "reference_search_groups" => delete_reference_search_group_batch(transaction, source_scope),
        "reference_search_manifest" => delete_scope_table(
            transaction,
            "code_repository_reference_search_manifests",
            source_scope,
        ),
        "path_tombstones" => {
            delete_scope_table(transaction, "code_repository_path_tombstones", source_scope)
        }
        "file_diagnostics" => delete_scope_table(
            transaction,
            "code_repository_file_diagnostics",
            source_scope,
        ),
        "chunks" => delete_scope_table(transaction, "code_repository_chunks", source_scope),
        "calls" => delete_scope_table(transaction, "code_repository_calls", source_scope),
        "routes" => delete_scope_table(transaction, "code_repository_routes", source_scope),
        "feature_flags" => {
            delete_scope_table(transaction, "code_repository_feature_flags", source_scope)
        }
        "framework_edges" => {
            delete_scope_table(transaction, "code_repository_framework_edges", source_scope)
        }
        "framework_nodes" => {
            delete_scope_table(transaction, "code_repository_framework_nodes", source_scope)
        }
        "dependencies" => {
            delete_scope_table(transaction, "code_repository_dependencies", source_scope)
        }
        "imports" => delete_scope_table(transaction, "code_repository_imports", source_scope),
        "references" => delete_scope_table(transaction, "code_repository_references", source_scope),
        "symbols" => delete_scope_table(transaction, "code_repository_symbols", source_scope),
        "files" => delete_scope_table(transaction, "code_repository_files", source_scope),
        "software_components" => {
            delete_scope_table(transaction, "software_components", source_scope)
        }
        "software_dependency_usages" => {
            delete_scope_table(transaction, "software_dependency_usages", source_scope)
        }
        "software_sdk_usages" => {
            delete_scope_table(transaction, "software_sdk_usages", source_scope)
        }
        "software_files" => delete_scope_table(transaction, "software_files", source_scope),
        "software_topics" => delete_scope_table(transaction, "software_topics", source_scope),
        "software_relationships" => {
            delete_scope_table(transaction, "software_relationships", source_scope)
        }
        "software_global_status" => {
            delete_scope_table(transaction, "software_global_status", source_scope)
        }
        "software_build_targets" => {
            delete_scope_table(transaction, "software_build_targets", source_scope)
        }
        "software_iac_resources" => {
            delete_scope_table(transaction, "software_iac_resources", source_scope)
        }
        "software_design_elements" => {
            delete_scope_table(transaction, "software_design_elements", source_scope)
        }
        "software_entities" => delete_scope_table(transaction, "software_entities", source_scope),
        "software_statements" => {
            delete_scope_table(transaction, "software_statements", source_scope)
        }
        "software_ontology_diagnostics" => {
            delete_scope_table(transaction, "software_ontology_diagnostics", source_scope)
        }
        "business_mappings" => delete_scope_table(transaction, "business_mappings", source_scope),
        "business_term_aliases" => {
            delete_scope_table(transaction, "business_term_aliases", source_scope)
        }
        "business_terms" => delete_scope_table(transaction, "business_terms", source_scope),
        "business_domains" => delete_scope_table(transaction, "business_domains", source_scope),
        "business_knowledge_status" => {
            delete_scope_table(transaction, "business_knowledge_status", source_scope)
        }
        "commit_scopes" => {
            delete_scope_table(transaction, "code_repository_commit_scopes", source_scope)
        }
        "index_batch_staging" => delete_scope_table(
            transaction,
            "code_repository_index_batch_staging",
            source_scope,
        ),
        "index_task_history" => delete_terminal_tasks(transaction, source_scope),
        "checkpoint" => delete_scope_table(
            transaction,
            "code_repository_index_checkpoints",
            source_scope,
        ),
        "scope_metadata" => delete_scope_table(transaction, "code_repository_scopes", source_scope),
        _ => Err(StorageError::Invariant(format!(
            "scope GC phase table has no handler for '{}'",
            phase.name()
        ))),
    }
}

fn delete_reference_search_group_batch(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<(usize, bool), StorageError> {
    let deleted = transaction.execute(
        "DELETE FROM code_repository_reference_search_groups
         WHERE source_scope = ?1 AND group_id IN (
             SELECT group_id FROM code_repository_reference_search_groups
             WHERE source_scope = ?1 ORDER BY group_id LIMIT ?2
         )",
        params![source_scope, GC_ROW_BATCH_SIZE],
    )?;
    let has_more = transaction.query_row(
        "SELECT EXISTS (
             SELECT 1 FROM code_repository_reference_search_groups
             WHERE source_scope = ?1 LIMIT 1
         )",
        params![source_scope],
        |row| row.get::<_, bool>(0),
    )?;
    Ok((deleted, has_more))
}

fn delete_scope_table(
    transaction: &Transaction<'_>,
    table: &'static str,
    source_scope: &str,
) -> Result<(usize, bool), StorageError> {
    delete_table_batch(transaction, table, "source_scope", source_scope)
}

fn delete_table_batch(
    transaction: &Transaction<'_>,
    table: &'static str,
    column: &'static str,
    value: &str,
) -> Result<(usize, bool), StorageError> {
    let deleted = transaction.execute(
        &format!(
            "DELETE FROM {table} WHERE rowid IN (
                 SELECT rowid FROM {table} WHERE {column} = ?1
                 ORDER BY rowid LIMIT ?2
             )"
        ),
        params![value, GC_ROW_BATCH_SIZE],
    )?;
    let has_more = transaction.query_row(
        &format!("SELECT EXISTS (SELECT 1 FROM {table} WHERE {column} = ?1 LIMIT 1)"),
        params![value],
        |row| row.get::<_, bool>(0),
    )?;
    Ok((deleted, has_more))
}

fn delete_cross_edges(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<(usize, bool), StorageError> {
    let deleted = transaction.execute(
        "DELETE FROM code_repository_cross_edges WHERE rowid IN (
             SELECT rowid FROM code_repository_cross_edges
             WHERE from_source_scope = ?1 OR to_source_scope = ?1
             ORDER BY rowid LIMIT ?2
         )",
        params![source_scope, GC_ROW_BATCH_SIZE],
    )?;
    let has_more = transaction.query_row(
        "SELECT EXISTS (SELECT 1 FROM code_repository_cross_edges
         WHERE from_source_scope = ?1 OR to_source_scope = ?1 LIMIT 1)",
        params![source_scope],
        |row| row.get::<_, bool>(0),
    )?;
    Ok((deleted, has_more))
}

fn delete_workspace_overlay(
    transaction: &Transaction<'_>,
    repository_id: &str,
) -> Result<(usize, bool), StorageError> {
    let set_id = super::super::workspace::workspace_set_id(repository_id);
    let mut deleted = transaction.execute(
        "DELETE FROM code_repository_set_overlay_status WHERE set_id = ?1",
        params![set_id],
    )?;
    deleted += transaction.execute(
        "DELETE FROM code_repository_sets
         WHERE set_id = ?1 AND NOT EXISTS (
             SELECT 1 FROM code_repository_set_members WHERE set_id = ?1
         )",
        params![set_id],
    )?;
    Ok((deleted, false))
}

fn delete_catalog_route(
    _transaction: &Transaction<'_>,
    _source_scope: &str,
) -> Result<(usize, bool), StorageError> {
    // The control-plane catalog route is also the durable capacity reservation
    // for a scope whose physical rows still exist in a repository shard. The
    // partitioned retention coordinator removes that route immediately before
    // the shard's final scope_metadata phase. Removing it from this generic
    // control-DB state machine would let admission reuse the slot while a large
    // shard scope was still being deleted in bounded batches.
    Ok((0, false))
}

fn delete_search_batch(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<(usize, bool), StorageError> {
    let mut statement = transaction.prepare(
        "SELECT rowid, search_rowid FROM code_repository_search_metadata
         WHERE source_scope = ?1 ORDER BY rowid LIMIT ?2",
    )?;
    let pairs = statement
        .query_map(params![source_scope, SEARCH_OWNER_BATCH_SIZE], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    let mut deleted = 0;
    if !pairs.is_empty() {
        deleted += delete_rowids(
            transaction,
            "code_repository_search",
            pairs.iter().map(|(_, search_rowid)| *search_rowid),
        )?;
        deleted += delete_rowids(
            transaction,
            "code_repository_search_metadata",
            pairs.iter().map(|(rowid, _)| *rowid),
        )?;
    }
    // Do not add an unbounded FTS5 UNINDEXED source_scope fallback here.
    // The next phase inspects the global rowid space in bounded pages and
    // refuses to delete any row that still has a metadata owner.
    let has_more = transaction.query_row(
        "SELECT EXISTS (SELECT 1 FROM code_repository_search_metadata
         WHERE source_scope = ?1 LIMIT 1)",
        params![source_scope],
        |row| row.get::<_, bool>(0),
    )?;
    Ok((deleted, has_more))
}

fn delete_search_orphan_batch(
    transaction: &Transaction<'_>,
    source_scope: &str,
    search_rowid_cursor: Option<i64>,
) -> Result<PhaseProgress, StorageError> {
    let page = search_orphan_page(transaction, search_rowid_cursor)?;
    let next_cursor = page.last().map(|candidate| candidate.rowid);
    if let Some(candidate) = page
        .iter()
        .find(|candidate| candidate.source_scope == source_scope && candidate.has_metadata_owner)
    {
        let ownership = if candidate.has_exact_metadata_owner {
            "an exact"
        } else {
            "a mismatched"
        };
        return Err(StorageError::Invariant(format!(
            "scope GC search-orphan page for retiring scope '{source_scope}' found {ownership} metadata owner for FTS rowid {}; metadata-owned rows must be removed by search_documents before orphan cleanup can advance",
            candidate.rowid
        )));
    }
    let deleted = delete_rowids(
        transaction,
        "code_repository_search",
        page.iter()
            .filter(|candidate| {
                candidate.source_scope == source_scope && !candidate.has_exact_metadata_owner
            })
            .map(|candidate| candidate.rowid),
    )?;
    let has_more = match next_cursor {
        Some(cursor) => transaction.query_row(
            "SELECT EXISTS (
                 SELECT 1 FROM code_repository_search WHERE rowid > ?1 LIMIT 1
             )",
            params![cursor],
            |row| row.get::<_, bool>(0),
        )?,
        None => false,
    };
    Ok(PhaseProgress {
        deleted,
        has_more,
        search_rowid_cursor: next_cursor,
    })
}

struct SearchOrphanCandidate {
    rowid: i64,
    source_scope: String,
    has_metadata_owner: bool,
    has_exact_metadata_owner: bool,
}

fn search_orphan_page(
    transaction: &Transaction<'_>,
    search_rowid_cursor: Option<i64>,
) -> Result<Vec<SearchOrphanCandidate>, StorageError> {
    let projection = "SELECT fts.rowid, fts.source_scope,
                EXISTS (
                    SELECT 1 FROM code_repository_search_metadata owner
                    WHERE owner.search_rowid = fts.rowid
                ),
                EXISTS (
                    SELECT 1 FROM code_repository_search_metadata owner
                    WHERE owner.search_rowid = fts.rowid
                      AND owner.source_scope = fts.source_scope
                      AND owner.document_kind = fts.document_kind
                      AND owner.record_id = fts.record_id
                      AND owner.path = fts.path
                )
         FROM code_repository_search fts";
    match search_rowid_cursor {
        Some(cursor) => {
            let mut statement = transaction.prepare(&format!(
                "{projection} WHERE fts.rowid > ?1 ORDER BY fts.rowid LIMIT ?2"
            ))?;
            collect_search_orphan_page(&mut statement, params![cursor, GC_ROW_BATCH_SIZE])
        }
        None => {
            let mut statement =
                transaction.prepare(&format!("{projection} ORDER BY fts.rowid LIMIT ?1"))?;
            collect_search_orphan_page(&mut statement, params![GC_ROW_BATCH_SIZE])
        }
    }
}

fn collect_search_orphan_page<P>(
    statement: &mut rusqlite::Statement<'_>,
    params: P,
) -> Result<Vec<SearchOrphanCandidate>, StorageError>
where
    P: rusqlite::Params,
{
    statement
        .query_map(params, |row| {
            Ok(SearchOrphanCandidate {
                rowid: row.get(0)?,
                source_scope: row.get(1)?,
                has_metadata_owner: row.get(2)?,
                has_exact_metadata_owner: row.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn delete_rowids(
    transaction: &Transaction<'_>,
    table: &'static str,
    rowids: impl IntoIterator<Item = i64>,
) -> Result<usize, StorageError> {
    let rowids = rowids.into_iter().collect::<Vec<_>>();
    if rowids.is_empty() {
        return Ok(0);
    }
    let placeholders = std::iter::repeat_n("?", rowids.len())
        .collect::<Vec<_>>()
        .join(", ");
    let values = rowids.into_iter().map(Value::Integer).collect::<Vec<_>>();
    transaction
        .execute(
            &format!("DELETE FROM {table} WHERE rowid IN ({placeholders})"),
            params_from_iter(values),
        )
        .map_err(StorageError::from)
}

fn delete_terminal_tasks(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<(usize, bool), StorageError> {
    let deleted = transaction.execute(
        "DELETE FROM code_repository_index_tasks WHERE task_id IN (
             SELECT task_id FROM code_repository_index_tasks
             WHERE source_scope = ?1
               AND state IN ('succeeded', 'failed', 'dead_letter', 'cancelled')
             ORDER BY updated_at_ms, task_id LIMIT ?2
         )",
        params![source_scope, GC_ROW_BATCH_SIZE],
    )?;
    let has_more = transaction.query_row(
        "SELECT EXISTS (SELECT 1 FROM code_repository_index_tasks
         WHERE source_scope = ?1
           AND state IN ('succeeded', 'failed', 'dead_letter', 'cancelled') LIMIT 1)",
        params![source_scope],
        |row| row.get::<_, bool>(0),
    )?;
    Ok((deleted, has_more))
}

//! Repository-set membership persistence and status mapping.

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{
    domain::{CodeRepositorySet, CodeRepositorySetMember, CodeRepositorySetMemberStatus},
    storage::{CodeRepositorySetMemberSeed, CodeRepositorySetSeed, StorageError},
};

use super::super::{super::evidence_identity::stable_id, status::parse_json_list};

pub(in super::super) fn create_set(
    connection: &mut Connection,
    seed: CodeRepositorySetSeed,
) -> Result<CodeRepositorySet, StorageError> {
    let set_id = stable_id("code-repository-set", &seed.alias);
    connection.execute(
        "
        INSERT INTO code_repository_sets (
            set_id, alias, description, default_ref_policy_json, created_at_ms, updated_at_ms
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?5)
        ON CONFLICT(alias) DO UPDATE SET
            description = excluded.description,
            default_ref_policy_json = excluded.default_ref_policy_json,
            updated_at_ms = excluded.updated_at_ms
        ",
        params![
            set_id,
            seed.alias,
            seed.description,
            seed.default_ref_policy_json,
            seed.now_ms,
        ],
    )?;

    set_by_alias(connection, &seed.alias)?.ok_or_else(|| {
        StorageError::InvalidInput("code repository set was not persisted".to_owned())
    })
}

pub(in super::super) fn add_member(
    connection: &mut Connection,
    seed: CodeRepositorySetMemberSeed,
) -> Result<CodeRepositorySetMember, StorageError> {
    let set = set_by_alias(connection, &seed.set_alias)?.ok_or_else(|| {
        StorageError::InvalidInput(format!(
            "code repository set '{}' is not registered",
            seed.set_alias
        ))
    })?;
    let scope_repository_id = connection
        .query_row(
            "
            SELECT repository_id
            FROM code_repository_scopes
            WHERE source_scope = ?1
            ",
            params![seed.source_scope],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or_else(|| {
            StorageError::InvalidInput(format!(
                "repository set member scope '{}' is not indexed",
                seed.source_scope
            ))
        })?;
    if scope_repository_id != seed.repository_id {
        return Err(StorageError::InvalidInput(format!(
            "repository set member scope '{}' belongs to repository '{}', not '{}'",
            seed.source_scope, scope_repository_id, seed.repository_id
        )));
    }

    let set_id = set.set_id.clone();
    let transaction = connection.transaction()?;
    transaction.execute(
        "
        DELETE FROM code_repository_set_members
        WHERE set_id = ?1 AND repository_id = ?2
        ",
        params![set_id, seed.repository_id],
    )?;
    transaction.execute(
        "DELETE FROM code_repository_cross_edges WHERE set_id = ?1",
        params![set_id],
    )?;
    transaction.execute(
        "DELETE FROM code_repository_set_overlay_status WHERE set_id = ?1",
        params![set_id],
    )?;
    transaction.execute(
        "
        INSERT INTO code_repository_set_members (
            set_id, repository_id, repository_alias, ref_selector, resolved_commit_sha,
            source_scope, path_filters_json, language_filters_json, priority
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        ",
        params![
            set_id,
            seed.repository_id,
            seed.repository_alias,
            seed.ref_selector,
            seed.resolved_commit_sha,
            seed.source_scope,
            json_list(&seed.path_filters)?,
            json_list(&seed.language_filters)?,
            seed.priority,
        ],
    )?;
    transaction.execute(
        "
        UPDATE code_repository_sets
        SET updated_at_ms = strftime('%s','now') * 1000
        WHERE set_id = ?1
        ",
        params![set_id],
    )?;
    transaction.commit()?;

    member_by_key(
        connection,
        &set.set_id,
        &seed.repository_id,
        &seed.source_scope,
    )?
    .ok_or_else(|| StorageError::InvalidInput("repository set member was not persisted".to_owned()))
}

pub(in super::super) fn remove_member(
    connection: &mut Connection,
    set_alias: &str,
    repository_alias: &str,
) -> Result<CodeRepositorySetMember, StorageError> {
    let set = set_by_alias(connection, set_alias)?.ok_or_else(|| {
        StorageError::InvalidInput(format!(
            "code repository set '{set_alias}' is not registered"
        ))
    })?;
    let removed = member_by_alias(connection, &set.set_id, repository_alias)?.ok_or_else(|| {
        StorageError::InvalidInput(format!(
            "repository set '{}' member '{}' is not registered",
            set.alias, repository_alias
        ))
    })?;
    let transaction = connection.transaction()?;
    transaction.execute(
        "
        DELETE FROM code_repository_set_members
        WHERE set_id = ?1 AND repository_alias = ?2
        ",
        params![set.set_id, repository_alias],
    )?;
    transaction.execute(
        "DELETE FROM code_repository_cross_edges WHERE set_id = ?1",
        params![set.set_id],
    )?;
    transaction.execute(
        "DELETE FROM code_repository_set_overlay_status WHERE set_id = ?1",
        params![set.set_id],
    )?;
    transaction.execute(
        "
        UPDATE code_repository_sets
        SET updated_at_ms = strftime('%s','now') * 1000
        WHERE set_id = ?1
        ",
        params![set.set_id],
    )?;
    transaction.commit()?;

    Ok(removed)
}

pub(in super::super) fn set_by_alias(
    connection: &mut Connection,
    alias: &str,
) -> Result<Option<CodeRepositorySet>, StorageError> {
    connection
        .query_row(
            "
            SELECT set_id, alias, description, default_ref_policy_json, created_at_ms, updated_at_ms
            FROM code_repository_sets
            WHERE alias = ?1
            ",
            params![alias],
            set_from_row,
        )
        .optional()
        .map_err(StorageError::from)
}

fn member_by_key(
    connection: &mut Connection,
    set_id: &str,
    repository_id: &str,
    source_scope: &str,
) -> Result<Option<CodeRepositorySetMember>, StorageError> {
    connection
        .query_row(
            "
            SELECT set_id, repository_id, repository_alias, ref_selector, resolved_commit_sha,
                   source_scope, path_filters_json, language_filters_json, priority
            FROM code_repository_set_members
            WHERE set_id = ?1 AND repository_id = ?2 AND source_scope = ?3
            ",
            params![set_id, repository_id, source_scope],
            member_from_row,
        )
        .optional()
        .map_err(StorageError::from)
}

fn member_by_alias(
    connection: &mut Connection,
    set_id: &str,
    repository_alias: &str,
) -> Result<Option<CodeRepositorySetMember>, StorageError> {
    connection
        .query_row(
            "
            SELECT set_id, repository_id, repository_alias, ref_selector, resolved_commit_sha,
                   source_scope, path_filters_json, language_filters_json, priority
            FROM code_repository_set_members
            WHERE set_id = ?1 AND repository_alias = ?2
            ",
            params![set_id, repository_alias],
            member_from_row,
        )
        .optional()
        .map_err(StorageError::from)
}

pub(super) fn member_statuses(
    connection: &mut Connection,
    set_id: &str,
) -> Result<Vec<CodeRepositorySetMemberStatus>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT member.set_id, member.repository_id, member.repository_alias, member.ref_selector,
               member.resolved_commit_sha, member.source_scope, member.path_filters_json,
               member.language_filters_json, member.priority, scope.tree_hash, scope.stale,
               scope.indexed_file_count, scope.symbol_count, scope.reference_count,
               scope.chunk_count, scope.degraded_reason, scope.path_filters_json,
               scope.language_filters_json
        FROM code_repository_set_members member
        JOIN code_repository_scopes scope ON scope.source_scope = member.source_scope
        WHERE member.set_id = ?1
        ORDER BY member.priority DESC, member.repository_alias ASC, member.source_scope ASC
        ",
    )?;
    let rows = statement.query_map(params![set_id], member_status_from_row)?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn set_from_row(row: &Row<'_>) -> rusqlite::Result<CodeRepositorySet> {
    Ok(CodeRepositorySet {
        set_id: row.get(0)?,
        alias: row.get(1)?,
        description: row.get(2)?,
        default_ref_policy_json: row.get(3)?,
        created_at_ms: row.get(4)?,
        updated_at_ms: row.get(5)?,
    })
}

fn member_from_row(row: &Row<'_>) -> rusqlite::Result<CodeRepositorySetMember> {
    Ok(CodeRepositorySetMember {
        set_id: row.get(0)?,
        repository_id: row.get(1)?,
        repository_alias: row.get(2)?,
        ref_selector: row.get(3)?,
        resolved_commit_sha: row.get(4)?,
        source_scope: row.get(5)?,
        path_filters: parse_json_list(row.get::<_, String>(6)?)?,
        language_filters: parse_json_list(row.get::<_, String>(7)?)?,
        priority: row.get(8)?,
    })
}

fn member_status_from_row(row: &Row<'_>) -> rusqlite::Result<CodeRepositorySetMemberStatus> {
    let stale = row.get::<_, i64>(10)? != 0;
    Ok(CodeRepositorySetMemberStatus {
        member: CodeRepositorySetMember {
            set_id: row.get(0)?,
            repository_id: row.get(1)?,
            repository_alias: row.get(2)?,
            ref_selector: row.get(3)?,
            resolved_commit_sha: row.get(4)?,
            source_scope: row.get(5)?,
            path_filters: parse_json_list(row.get::<_, String>(6)?)?,
            language_filters: parse_json_list(row.get::<_, String>(7)?)?,
            priority: row.get(8)?,
        },
        tree_hash: row.get(9)?,
        indexed_path_filters: parse_json_list(row.get::<_, String>(16)?)?,
        indexed_language_filters: parse_json_list(row.get::<_, String>(17)?)?,
        freshness_state: if stale {
            "stale".to_owned()
        } else {
            "fresh".to_owned()
        },
        stale,
        indexed_file_count: row.get(11)?,
        symbol_count: row.get(12)?,
        reference_count: row.get(13)?,
        chunk_count: row.get(14)?,
        degraded_reason: row.get(15)?,
    })
}

fn json_list(values: &[String]) -> Result<String, StorageError> {
    serde_json::to_string(values).map_err(|error| StorageError::InvalidInput(error.to_string()))
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod membership_tests;

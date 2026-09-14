use std::{collections::BTreeSet, io::Write};

use rusqlite::{OptionalExtension, Transaction, params};

use crate::{
    domain::{CodeIncrementalClonePhase, code_incremental_clone_state},
    storage::StorageError,
};

pub(super) const PROTOCOL_VERSION: usize = 1;
pub(super) const PHASE_TABLES: &str = "tables";
pub(super) const PHASE_SEARCH: &str = "search";
pub(super) const PHASE_CLONE_COMPLETE: &str = "clone_complete";

const INTEGER_STORAGE_BYTES: usize = 27 * 8 + 9;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CloneProgress {
    pub(super) source_scope: String,
    pub(super) repository_id: String,
    pub(super) base_scope: String,
    pub(super) task_id: String,
    pub(super) delta_digest: String,
    pub(super) phase: String,
    pub(super) table_ordinal: usize,
    pub(super) completed_page_ordinal: usize,
    pub(super) cursor_key: Option<String>,
    pub(super) cursor_tiebreaker: Option<String>,
    pub(super) completed_table_ordinal: Option<usize>,
    pub(super) expected_table_rows: Option<usize>,
    pub(super) scanned_table_rows: usize,
    pub(super) copied_table_rows: usize,
    pub(super) scanned_total_rows: usize,
    pub(super) copied_total_rows: usize,
    pub(super) copied_total_bytes: usize,
    pub(super) cloned_file_count: usize,
    pub(super) cloned_symbol_count: usize,
    pub(super) cloned_reference_count: usize,
    pub(super) cloned_chunk_count: usize,
    pub(super) cloned_diagnostic_count: usize,
    pub(super) cloned_reference_group_count: usize,
    pub(super) cloned_search_document_count: usize,
    pub(super) base_manifest_reference_count: usize,
    pub(super) base_manifest_group_count: usize,
    pub(super) scanned_reference_occurrence_count: usize,
    pub(super) scanned_reference_row_count: usize,
    pub(super) scanned_reference_group_count: usize,
    pub(super) scanned_reference_search_owner_count: usize,
    pub(super) base_source_fact_row_upper_bound: usize,
    pub(super) page_row_limit: usize,
    pub(super) page_byte_limit: usize,
}

impl CloneProgress {
    pub(super) fn typed_phase(&self) -> Result<CodeIncrementalClonePhase, StorageError> {
        match self.phase.as_str() {
            PHASE_TABLES => Ok(CodeIncrementalClonePhase::Tables),
            PHASE_SEARCH => Ok(CodeIncrementalClonePhase::Search),
            PHASE_CLONE_COMPLETE => Ok(CodeIncrementalClonePhase::CloneComplete),
            phase => Err(StorageError::Invariant(format!(
                "incremental clone scope '{}' has unknown phase '{phase}'",
                self.source_scope
            ))),
        }
    }
}

pub(super) fn load(
    connection: &rusqlite::Connection,
    source_scope: &str,
) -> Result<Option<CloneProgress>, StorageError> {
    connection
        .query_row(
            "SELECT source_scope, repository_id, base_scope, task_id, delta_digest,
                    protocol_version, phase, table_ordinal, completed_page_ordinal,
                    cursor_key, cursor_tiebreaker, completed_table_ordinal, expected_table_rows,
                    scanned_table_rows, copied_table_rows, scanned_total_rows,
                    copied_total_rows, copied_total_bytes,
                    cloned_file_count, cloned_symbol_count, cloned_reference_count,
                    cloned_chunk_count, cloned_diagnostic_count,
                    cloned_reference_group_count, cloned_search_document_count,
                    base_manifest_reference_count, base_manifest_group_count,
                    scanned_reference_occurrence_count,
                    scanned_reference_row_count, scanned_reference_group_count,
                    scanned_reference_search_owner_count,
                    base_source_fact_row_upper_bound,
                    page_row_limit, page_byte_limit
             FROM code_repository_incremental_clone_progress
             WHERE source_scope = ?1",
            [source_scope],
            |row| {
                let protocol_version = row.get::<_, usize>(5)?;
                if protocol_version != PROTOCOL_VERSION {
                    return Err(rusqlite::Error::FromSqlConversionFailure(
                        5,
                        rusqlite::types::Type::Integer,
                        Box::new(std::io::Error::other(format!(
                            "unsupported incremental clone protocol {protocol_version}"
                        ))),
                    ));
                }
                Ok(CloneProgress {
                    source_scope: row.get(0)?,
                    repository_id: row.get(1)?,
                    base_scope: row.get(2)?,
                    task_id: row.get(3)?,
                    delta_digest: row.get(4)?,
                    phase: row.get(6)?,
                    table_ordinal: row.get(7)?,
                    completed_page_ordinal: row.get(8)?,
                    cursor_key: row.get(9)?,
                    cursor_tiebreaker: row.get(10)?,
                    completed_table_ordinal: row.get(11)?,
                    expected_table_rows: row.get(12)?,
                    scanned_table_rows: row.get(13)?,
                    copied_table_rows: row.get(14)?,
                    scanned_total_rows: row.get(15)?,
                    copied_total_rows: row.get(16)?,
                    copied_total_bytes: row.get(17)?,
                    cloned_file_count: row.get(18)?,
                    cloned_symbol_count: row.get(19)?,
                    cloned_reference_count: row.get(20)?,
                    cloned_chunk_count: row.get(21)?,
                    cloned_diagnostic_count: row.get(22)?,
                    cloned_reference_group_count: row.get(23)?,
                    cloned_search_document_count: row.get(24)?,
                    base_manifest_reference_count: row.get(25)?,
                    base_manifest_group_count: row.get(26)?,
                    scanned_reference_occurrence_count: row.get(27)?,
                    scanned_reference_row_count: row.get(28)?,
                    scanned_reference_group_count: row.get(29)?,
                    scanned_reference_search_owner_count: row.get(30)?,
                    base_source_fact_row_upper_bound: row.get(31)?,
                    page_row_limit: row.get(32)?,
                    page_byte_limit: row.get(33)?,
                })
            },
        )
        .optional()
        .map_err(|error| match error {
            rusqlite::Error::FromSqlConversionFailure(..)
            | rusqlite::Error::IntegralValueOutOfRange(..)
            | rusqlite::Error::InvalidColumnType(..) => StorageError::Invariant(format!(
                "incremental clone progress for scope '{source_scope}' cannot be decoded: {error}"
            )),
            other => StorageError::from(other),
        })
}

pub(super) fn insert(
    transaction: &Transaction<'_>,
    progress: &CloneProgress,
    now_ms: u64,
) -> Result<(), StorageError> {
    transaction.execute(
        "INSERT INTO code_repository_incremental_clone_progress (
             source_scope, repository_id, base_scope, task_id, delta_digest,
             protocol_version, phase, table_ordinal, completed_page_ordinal,
             cursor_key, cursor_tiebreaker, completed_table_ordinal, expected_table_rows,
             scanned_table_rows, copied_table_rows, scanned_total_rows,
             copied_total_rows, copied_total_bytes,
             cloned_file_count, cloned_symbol_count, cloned_reference_count,
             cloned_chunk_count, cloned_diagnostic_count,
             cloned_reference_group_count, cloned_search_document_count,
             base_manifest_reference_count, base_manifest_group_count,
             scanned_reference_occurrence_count, scanned_reference_row_count,
             scanned_reference_group_count, scanned_reference_search_owner_count,
             base_source_fact_row_upper_bound,
             page_row_limit, page_byte_limit, updated_at_ms
         ) VALUES (
             ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9,
             ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18,
             ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28,
             ?29, ?30, ?31, ?32, ?33, ?34, ?35
         )",
        params![
            progress.source_scope,
            progress.repository_id,
            progress.base_scope,
            progress.task_id,
            progress.delta_digest,
            PROTOCOL_VERSION,
            progress.phase,
            progress.table_ordinal,
            progress.completed_page_ordinal,
            progress.cursor_key,
            progress.cursor_tiebreaker,
            progress.completed_table_ordinal,
            progress.expected_table_rows,
            progress.scanned_table_rows,
            progress.copied_table_rows,
            progress.scanned_total_rows,
            progress.copied_total_rows,
            progress.copied_total_bytes,
            progress.cloned_file_count,
            progress.cloned_symbol_count,
            progress.cloned_reference_count,
            progress.cloned_chunk_count,
            progress.cloned_diagnostic_count,
            progress.cloned_reference_group_count,
            progress.cloned_search_document_count,
            progress.base_manifest_reference_count,
            progress.base_manifest_group_count,
            progress.scanned_reference_occurrence_count,
            progress.scanned_reference_row_count,
            progress.scanned_reference_group_count,
            progress.scanned_reference_search_owner_count,
            progress.base_source_fact_row_upper_bound,
            progress.page_row_limit,
            progress.page_byte_limit,
            now_ms,
        ],
    )?;
    Ok(())
}

pub(super) fn compare_and_store(
    transaction: &Transaction<'_>,
    expected: &CloneProgress,
    next: &CloneProgress,
    now_ms: u64,
) -> Result<(), StorageError> {
    let changed = transaction.execute(
        "UPDATE code_repository_incremental_clone_progress
         SET phase = ?10, table_ordinal = ?11, completed_page_ordinal = ?12,
             cursor_key = ?13, cursor_tiebreaker = ?14, completed_table_ordinal = ?15,
             expected_table_rows = ?16, scanned_table_rows = ?17,
             copied_table_rows = ?18, scanned_total_rows = ?19,
             copied_total_rows = ?20, copied_total_bytes = ?21,
             cloned_file_count = ?22, cloned_symbol_count = ?23,
             cloned_reference_count = ?24, cloned_chunk_count = ?25,
             cloned_diagnostic_count = ?26, cloned_reference_group_count = ?27,
             cloned_search_document_count = ?28,
             base_manifest_reference_count = ?29, base_manifest_group_count = ?30,
             scanned_reference_occurrence_count = ?31,
             scanned_reference_row_count = ?32, scanned_reference_group_count = ?33,
             scanned_reference_search_owner_count = ?34,
             base_source_fact_row_upper_bound = ?35,
             updated_at_ms = ?36
         WHERE source_scope = ?1 AND repository_id = ?2 AND base_scope = ?3
           AND task_id = ?4 AND delta_digest = ?5 AND protocol_version = ?6
           AND phase = ?7 AND table_ordinal = ?8 AND completed_page_ordinal = ?9
           AND cursor_key IS ?37 AND cursor_tiebreaker IS ?38
           AND completed_table_ordinal IS ?39
           AND expected_table_rows IS ?40 AND scanned_table_rows = ?41
           AND copied_table_rows = ?42 AND scanned_total_rows = ?43
           AND copied_total_rows = ?44 AND copied_total_bytes = ?45
           AND cloned_file_count = ?46 AND cloned_symbol_count = ?47
           AND cloned_reference_count = ?48 AND cloned_chunk_count = ?49
           AND cloned_diagnostic_count = ?50 AND cloned_reference_group_count = ?51
           AND cloned_search_document_count = ?52
           AND base_manifest_reference_count = ?53
           AND base_manifest_group_count = ?54
           AND scanned_reference_occurrence_count = ?55
           AND scanned_reference_row_count = ?56
           AND scanned_reference_group_count = ?57
           AND scanned_reference_search_owner_count = ?58
           AND base_source_fact_row_upper_bound = ?59
           AND page_row_limit = ?60 AND page_byte_limit = ?61",
        params![
            expected.source_scope,
            expected.repository_id,
            expected.base_scope,
            expected.task_id,
            expected.delta_digest,
            PROTOCOL_VERSION,
            expected.phase,
            expected.table_ordinal,
            expected.completed_page_ordinal,
            next.phase,
            next.table_ordinal,
            next.completed_page_ordinal,
            next.cursor_key,
            next.cursor_tiebreaker,
            next.completed_table_ordinal,
            next.expected_table_rows,
            next.scanned_table_rows,
            next.copied_table_rows,
            next.scanned_total_rows,
            next.copied_total_rows,
            next.copied_total_bytes,
            next.cloned_file_count,
            next.cloned_symbol_count,
            next.cloned_reference_count,
            next.cloned_chunk_count,
            next.cloned_diagnostic_count,
            next.cloned_reference_group_count,
            next.cloned_search_document_count,
            next.base_manifest_reference_count,
            next.base_manifest_group_count,
            next.scanned_reference_occurrence_count,
            next.scanned_reference_row_count,
            next.scanned_reference_group_count,
            next.scanned_reference_search_owner_count,
            next.base_source_fact_row_upper_bound,
            now_ms,
            expected.cursor_key,
            expected.cursor_tiebreaker,
            expected.completed_table_ordinal,
            expected.expected_table_rows,
            expected.scanned_table_rows,
            expected.copied_table_rows,
            expected.scanned_total_rows,
            expected.copied_total_rows,
            expected.copied_total_bytes,
            expected.cloned_file_count,
            expected.cloned_symbol_count,
            expected.cloned_reference_count,
            expected.cloned_chunk_count,
            expected.cloned_diagnostic_count,
            expected.cloned_reference_group_count,
            expected.cloned_search_document_count,
            expected.base_manifest_reference_count,
            expected.base_manifest_group_count,
            expected.scanned_reference_occurrence_count,
            expected.scanned_reference_row_count,
            expected.scanned_reference_group_count,
            expected.scanned_reference_search_owner_count,
            expected.base_source_fact_row_upper_bound,
            expected.page_row_limit,
            expected.page_byte_limit,
        ],
    )?;
    if changed != 1 {
        return Err(StorageError::Invariant(format!(
            "incremental clone progress for scope '{}' changed before page {} could commit",
            expected.source_scope, expected.completed_page_ordinal
        )));
    }
    let expected_state = checkpoint_state(expected)?;
    let next_state = checkpoint_state(next)?;
    let checkpoint_changed = transaction.execute(
        "UPDATE code_repository_index_checkpoints
         SET state = ?3, updated_at_ms = ?4
         WHERE source_scope = ?1 AND state = ?2",
        params![expected.source_scope, expected_state, next_state, now_ms],
    )?;
    if checkpoint_changed == 1 {
        return Ok(());
    }
    Err(StorageError::Invariant(format!(
        "incremental clone checkpoint for scope '{}' changed before page {} could commit",
        expected.source_scope, expected.completed_page_ordinal
    )))
}

pub(super) fn checkpoint_state(progress: &CloneProgress) -> Result<String, StorageError> {
    let phase = match progress.phase.as_str() {
        PHASE_TABLES => CodeIncrementalClonePhase::Tables,
        PHASE_SEARCH => CodeIncrementalClonePhase::Search,
        PHASE_CLONE_COMPLETE => CodeIncrementalClonePhase::CloneComplete,
        other => {
            return Err(StorageError::Invariant(format!(
                "incremental clone progress has no checkpoint phase for '{other}'"
            )));
        }
    };
    code_incremental_clone_state(
        phase,
        progress.table_ordinal,
        progress.completed_page_ordinal,
        progress.scanned_total_rows,
        &cursor_digest(progress)?,
    )
    .ok_or_else(|| {
        StorageError::Invariant(format!(
            "incremental clone progress for scope '{}' cannot form a canonical checkpoint",
            progress.source_scope
        ))
    })
}

pub(super) fn cleanup_surface(
    progress: &CloneProgress,
    affected_paths: &BTreeSet<String>,
) -> Result<(usize, usize), StorageError> {
    let rows = affected_paths.len().checked_add(1).ok_or_else(|| {
        StorageError::CapacityExceeded("incremental clone cleanup overflowed".to_owned())
    })?;
    let mut bytes = [
        progress.source_scope.as_str(),
        progress.repository_id.as_str(),
        progress.base_scope.as_str(),
        progress.task_id.as_str(),
        progress.delta_digest.as_str(),
        progress.phase.as_str(),
        progress.cursor_key.as_deref().unwrap_or_default(),
        progress.cursor_tiebreaker.as_deref().unwrap_or_default(),
    ]
    .iter()
    .try_fold(
        super::admission::ROW_STORAGE_OVERHEAD_BYTES + INTEGER_STORAGE_BYTES,
        |total, value| {
            total.checked_add(value.len()).ok_or_else(|| {
                StorageError::CapacityExceeded("incremental clone cleanup overflowed".to_owned())
            })
        },
    )?;
    for path in affected_paths {
        bytes = bytes
            .checked_add(super::admission::ROW_STORAGE_OVERHEAD_BYTES)
            .and_then(|value| value.checked_add(progress.source_scope.len()))
            .and_then(|value| value.checked_add(path.len()))
            .ok_or_else(|| {
                StorageError::CapacityExceeded("incremental clone cleanup overflowed".to_owned())
            })?;
    }
    Ok((rows, bytes))
}

fn cursor_digest(progress: &CloneProgress) -> Result<String, StorageError> {
    if progress.cursor_key.is_none()
        && progress.cursor_tiebreaker.is_none()
        && progress.completed_table_ordinal.is_none()
        && progress.expected_table_rows.is_none()
    {
        return Ok("none".to_owned());
    }
    if progress.cursor_key.is_none() && progress.cursor_tiebreaker.is_some() {
        return Err(StorageError::Invariant(
            "incremental clone cursor tiebreaker has no key".to_owned(),
        ));
    }
    let mut writer = crate::storage::sqlite::evidence_identity::StableIdWriter::new();
    write_digest_usize(&mut writer, progress.completed_table_ordinal)?;
    write_digest_usize(&mut writer, progress.expected_table_rows)?;
    for value in [
        progress.copied_total_rows,
        progress.copied_total_bytes,
        progress.cloned_file_count,
        progress.cloned_symbol_count,
        progress.cloned_reference_count,
        progress.cloned_chunk_count,
        progress.cloned_diagnostic_count,
        progress.cloned_reference_group_count,
        progress.cloned_search_document_count,
        progress.scanned_reference_occurrence_count,
        progress.scanned_reference_row_count,
        progress.scanned_reference_group_count,
        progress.scanned_reference_search_owner_count,
        progress.base_source_fact_row_upper_bound,
    ] {
        write_digest_usize(&mut writer, Some(value))?;
    }
    if let Some(key) = progress.cursor_key.as_deref() {
        write_digest_part(&mut writer, key)?;
    }
    if let Some(tiebreaker) = progress.cursor_tiebreaker.as_deref() {
        write_digest_part(&mut writer, tiebreaker)?;
    }
    Ok(writer.finish_hex())
}

fn write_digest_usize(
    writer: &mut crate::storage::sqlite::evidence_identity::StableIdWriter,
    value: Option<usize>,
) -> Result<(), StorageError> {
    let encoded = value
        .map(u64::try_from)
        .transpose()
        .map_err(|_| {
            StorageError::CapacityExceeded("incremental clone proof overflowed".to_owned())
        })?
        .unwrap_or(u64::MAX);
    writer
        .write_all(&encoded.to_le_bytes())
        .map_err(|error| StorageError::Invariant(error.to_string()))
}

fn write_digest_part(
    writer: &mut crate::storage::sqlite::evidence_identity::StableIdWriter,
    value: &str,
) -> Result<(), StorageError> {
    let length = u64::try_from(value.len()).map_err(|_| {
        StorageError::CapacityExceeded("incremental clone cursor length overflowed".to_owned())
    })?;
    writer
        .write_all(&length.to_le_bytes())
        .and_then(|_| writer.write_all(value.as_bytes()))
        .map_err(|error| StorageError::Invariant(error.to_string()))
}

use rusqlite::{Connection, params};

use crate::{
    domain::{EvidenceExtractionMetadata, GraphVersion, GraphVersionRange},
    storage::{MutationLogEntry, StorageError},
};

use super::indexing;

pub(super) fn source_hash_for_evidence(
    extraction: &EvidenceExtractionMetadata,
    source_scope: &str,
    source_path: Option<&str>,
    content: &str,
) -> String {
    extraction
        .source_hash
        .clone()
        .unwrap_or_else(|| indexing::source_hash(source_scope, source_path, content))
}

pub(super) fn count_rows(
    connection: &Connection,
    table: &'static str,
) -> Result<usize, StorageError> {
    let sql = format!("SELECT COUNT(*) FROM {table}");
    let count = connection.query_row(&sql, [], |row| row.get::<_, usize>(0))?;

    Ok(count)
}

pub(super) fn read_mutations_after(
    connection: &mut Connection,
    graph_version: GraphVersion,
    limit: usize,
) -> Result<Vec<MutationLogEntry>, StorageError> {
    if limit == 0 {
        return Err(StorageError::InvalidInput(
            "mutation log limit must be greater than zero".to_owned(),
        ));
    }

    let mut statement = connection.prepare(
        "
        SELECT graph_version, evidence_count, entity_count,
               relation_count, claim_count, event_count,
               affected_scopes_json, affected_entity_ids_json,
               evidence_ids_json, source_hashes_json
        FROM graph_mutations
        WHERE graph_version > ?1
        ORDER BY graph_version ASC
        LIMIT ?2
        ",
    )?;
    let rows = statement.query_map(params![graph_version.get(), limit], |row| {
        Ok((
            row.get::<_, u64>(0)?,
            row.get::<_, usize>(1)?,
            row.get::<_, usize>(2)?,
            row.get::<_, usize>(3)?,
            row.get::<_, usize>(4)?,
            row.get::<_, usize>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, String>(7)?,
            row.get::<_, String>(8)?,
            row.get::<_, String>(9)?,
        ))
    })?;
    rows.collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(
            |(
                graph_version,
                evidence_count,
                entity_count,
                relation_count,
                claim_count,
                event_count,
                affected_scopes,
                affected_entity_ids,
                evidence_ids,
                source_hashes,
            )| {
                Ok(MutationLogEntry {
                    graph_version: GraphVersion::new(graph_version),
                    evidence_count,
                    entity_count,
                    relation_count,
                    claim_count,
                    event_count,
                    affected_scopes: indexing::parse_json_array(affected_scopes)?,
                    affected_entity_ids: indexing::parse_json_array(affected_entity_ids)?,
                    evidence_ids: indexing::parse_json_array(evidence_ids)?,
                    source_hashes: indexing::parse_json_array(source_hashes)?,
                })
            },
        )
        .collect()
}

pub(super) fn stable_id(prefix: &str, value: &str) -> String {
    let normalized = value.to_lowercase();

    format!("{prefix}:{:016x}", stable_hash64(normalized.as_bytes()))
}

pub(super) fn storage_version_range(
    range: GraphVersionRange,
    commit_version: GraphVersion,
) -> GraphVersionRange {
    if range.valid_from == GraphVersion::ZERO && range.valid_until.is_none() {
        GraphVersionRange::open_from(commit_version)
    } else {
        range
    }
}

fn stable_hash64(bytes: &[u8]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET_BASIS;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }

    hash
}

//! Reads persisted graph mutation records after a requested graph version.

use rusqlite::{Connection, params};

use crate::{
    domain::GraphVersion,
    storage::{MutationLogEntry, StorageError},
};

use super::indexing;

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

#[cfg(test)]
mod mod_tests;

//! Bounded statement-endpoint expansion for the all-kind projection.

use std::collections::BTreeSet;

use rusqlite::Connection;

use crate::{
    domain::{SoftwareEntity, SoftwareGlobalRequest, SoftwareStatement},
    storage::StorageError,
};

const STATEMENT_ENTITY_TARGET_QUERY_BATCH_SIZE: usize = 256;

pub(super) fn append_statement_targets(
    connection: &Connection,
    source_scope: &str,
    request: &SoftwareGlobalRequest,
    entities: &mut Vec<SoftwareEntity>,
    statements: &[SoftwareStatement],
) -> Result<(), StorageError> {
    let mut seen_keys = entities
        .iter()
        .map(|entity| entity.entity_key.clone())
        .collect::<BTreeSet<_>>();
    let target_keys = statements
        .iter()
        .flat_map(|statement| {
            std::iter::once(statement.subject_id.as_str()).chain(statement.object_id.as_deref())
        })
        .filter(|entity_key| seen_keys.insert((*entity_key).to_owned()))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut remaining = request.limit;

    for batch in target_keys.chunks(STATEMENT_ENTITY_TARGET_QUERY_BATCH_SIZE) {
        if remaining == 0 {
            break;
        }
        let targets = super::super::ontology::entities_by_keys_for_scope(
            connection,
            source_scope,
            request,
            batch,
            remaining,
        )?;
        remaining = remaining.saturating_sub(targets.len());
        entities.extend(targets);
    }
    Ok(())
}

#[cfg(test)]
#[path = "entity_targets_tests.rs"]
mod tests;

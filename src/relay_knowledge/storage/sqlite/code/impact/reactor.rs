//! Maps snapshot-bound module dependency chains to impact evidence hits.

use super::super::query::{HitParts, hit_from_parts, required_scope};
use crate::{
    domain::{CodeImpactRequest, CodeRepositoryStatus, CodeRetrievalHit, RepositoryCodeRange},
    storage::StorageError,
};
use rusqlite::Connection;
use std::collections::BTreeSet;

pub(super) fn module_impacts(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeImpactRequest,
    changed: &BTreeSet<String>,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let impacts = crate::storage::sqlite::maven::reactor::downstream(
        connection,
        required_scope(status)?,
        changed,
        &request.repository.path_filters,
    )?;
    Ok(impacts
        .into_iter()
        .filter(|impact| {
            super::path_selection::impact_row_allowed(&impact.path, "xml", status, request)
        })
        .map(|impact| {
            hit_from_parts(
                status,
                HitParts {
                    path: impact.path,
                    language_id: "xml".into(),
                    byte_range: RepositoryCodeRange { start: 0, end: 0 },
                    line_range: RepositoryCodeRange {
                        start: impact.line,
                        end: impact.line,
                    },
                    symbol_snapshot_id: None,
                    canonical_symbol_id: None,
                    file_id: None,
                    retrieval_layers: Vec::new(),
                    score: 12.0,
                    excerpt: format!("Maven downstream module dependency: {}", impact.chain),
                    is_generated: false,
                    degraded_reason: None,
                    edge_kind: Some("module_depends_on".into()),
                    edge_resolution_state: Some("resolved".into()),
                    edge_target_hint: Some(impact.chain),
                    edge_confidence_basis_points: Some(10_000),
                    edge_confidence_tier: Some("extracted".into()),
                },
            )
        })
        .collect())
}

#[cfg(test)]
#[path = "reactor_tests.rs"]
mod tests;

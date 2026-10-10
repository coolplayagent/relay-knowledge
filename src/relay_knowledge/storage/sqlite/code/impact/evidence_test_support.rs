//! Runs individual evidence owners with their own bounded result collector.

use std::collections::BTreeSet;

use rusqlite::Connection;

use crate::{
    domain::{CodeImpactRequest, CodeRepositoryStatus, CodeRetrievalHit},
    storage::StorageError,
};

use super::{evidence, evidence_budget::ImpactHits};

pub(super) fn chunks_for_paths(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    paths: &BTreeSet<String>,
    request: &CodeImpactRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let mut hits = ImpactHits::new(request.limit);
    evidence::chunks_for_paths(connection, status, paths, request, &mut hits)?;
    Ok(hits.finish())
}

pub(super) fn callers_for_symbols(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    symbols: &[String],
    deleted_names: &[String],
    request: &CodeImpactRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let mut hits = ImpactHits::new(request.limit);
    evidence::callers_for_symbols(
        connection,
        status,
        symbols,
        deleted_names,
        request,
        &mut hits,
    )?;
    Ok(hits.finish())
}

pub(super) fn importers_for_modules(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    modules: &[String],
    request: &CodeImpactRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let mut hits = ImpactHits::new(request.limit);
    evidence::importers_for_modules(connection, status, modules, request, &mut hits)?;
    Ok(hits.finish())
}

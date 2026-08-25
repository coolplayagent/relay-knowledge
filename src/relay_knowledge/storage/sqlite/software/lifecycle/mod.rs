//! Build, infrastructure, and design projection lifecycle orchestration.

use std::collections::HashSet;

use rusqlite::Connection;

use crate::{
    domain::{GraphVersion, SoftwareBuildTarget, SoftwareDesignElement, SoftwareIacResource},
    storage::StorageError,
};

pub(super) use build::build_targets_for_scope;
pub(super) use design::design_elements_for_scope;
pub(super) use iac::iac_resources_for_scope;

mod build;
mod design;
mod document;
mod iac;
mod syntax;

pub(super) struct LifecycleProjection {
    pub(super) build_targets: Vec<SoftwareBuildTarget>,
    pub(super) iac_resources: Vec<SoftwareIacResource>,
    pub(super) design_elements: Vec<SoftwareDesignElement>,
}

struct BoundedFacts<T> {
    values: Vec<T>,
    identities: HashSet<String>,
    limit: usize,
    label: &'static str,
}

impl<T> BoundedFacts<T> {
    fn new(limit: usize, label: &'static str) -> Self {
        Self {
            values: Vec::new(),
            identities: HashSet::new(),
            limit,
            label,
        }
    }

    fn insert(&mut self, identity: String, value: T) -> Result<(), StorageError> {
        if self.identities.contains(&identity) {
            return Ok(());
        }
        if self.values.len() >= self.limit {
            return Err(StorageError::CapacityExceeded(format!(
                "software lifecycle {} exceed the bounded limit {}",
                self.label, self.limit
            )));
        }
        self.identities.insert(identity);
        self.values.push(value);
        Ok(())
    }

    fn as_slice(&self) -> &[T] {
        &self.values
    }

    fn into_vec(self) -> Vec<T> {
        self.values
    }
}

pub(super) fn initialize_schema(connection: &Connection) -> Result<(), StorageError> {
    build::initialize_schema(connection)?;
    iac::initialize_schema(connection)?;
    design::initialize_schema(connection)
}

pub(super) fn delete_scope(
    connection: &Connection,
    source_scope: &str,
) -> Result<(), StorageError> {
    build::delete_scope(connection, source_scope)?;
    iac::delete_scope(connection, source_scope)?;
    design::delete_scope(connection, source_scope)
}

pub(super) fn refresh_projection(
    connection: &Connection,
    source_scope: &str,
    graph_version: GraphVersion,
) -> Result<LifecycleProjection, StorageError> {
    let mut build_targets = build::begin_refresh(connection, source_scope)?;
    let mut iac_resources = iac::new_resources();
    let mut design_elements = design::new_elements();
    let stats = document::visit_candidates(connection, source_scope, |candidate| {
        build::collect(&candidate, graph_version, &mut build_targets)?;
        iac::collect(&candidate, graph_version, &mut iac_resources)?;
        design::collect(&candidate, graph_version, &mut design_elements)
    })?;
    build::persist(connection, source_scope, graph_version, &mut build_targets)?;
    iac::persist(connection, iac_resources.as_slice())?;
    design::persist(connection, design_elements.as_slice())?;
    tracing::debug!(
        source_scope,
        candidate_document_count = stats.document_count,
        candidate_chunk_count = stats.chunk_count,
        candidate_materialized_bytes = stats.materialized_bytes,
        "software lifecycle candidate stream completed"
    );

    Ok(LifecycleProjection {
        build_targets: build_targets.into_vec(),
        iac_resources: iac_resources.into_vec(),
        design_elements: design_elements.into_vec(),
    })
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;

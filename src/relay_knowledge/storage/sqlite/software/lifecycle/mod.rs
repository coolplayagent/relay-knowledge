//! Build, infrastructure, and design projection lifecycle orchestration.

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
    let documents = document::load(connection, source_scope)?;
    let build_targets = build::refresh(connection, source_scope, graph_version, &documents)?;
    let iac_resources = iac::refresh(connection, graph_version, &documents)?;
    let design_elements = design::refresh(connection, graph_version, &documents)?;

    Ok(LifecycleProjection {
        build_targets,
        iac_resources,
        design_elements,
    })
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;

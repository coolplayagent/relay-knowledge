use std::collections::BTreeSet;

use rusqlite::Connection;

use crate::{
    domain::{
        GraphVersion, SoftwareComponent, SoftwareDependencyUsage, SoftwareDependencyUsageInput,
    },
    storage::StorageError,
};

mod matching;
mod persistence;
mod python;
mod schema;

use matching::{
    DependencyMatchIndex, component_alias_keys, import_match_candidates_with_python_locals,
};
use persistence::import_evidence;
pub(super) use persistence::{delete_scope, insert_usage, usages_for_scope};
pub(super) use schema::initialize_schema;

pub(super) fn derive_dependency_usages(
    connection: &Connection,
    source_scope: &str,
    graph_version: GraphVersion,
    components: &[SoftwareComponent],
) -> Result<Vec<SoftwareDependencyUsage>, StorageError> {
    let alias_keys = component_alias_keys(connection, source_scope)?;
    let index = DependencyMatchIndex::new(components, &alias_keys);
    if index.is_empty() {
        return Ok(Vec::new());
    }

    let imports = import_evidence(connection, source_scope)?;
    let python_local_modules = python::local_modules(connection, source_scope)?;
    let mut seen_usage_ids = BTreeSet::new();
    let mut usages = Vec::new();
    for import in imports {
        for candidate in import_match_candidates_with_python_locals(
            &import.language_id,
            &import.module,
            import.target_hint.as_deref(),
            &import.resolution_state,
            Some(&python_local_modules),
        ) {
            let matches = index.matching_components(
                &import.language_id,
                &candidate.value,
                &import.evidence_path,
            );
            for component_match in matches {
                let confidence = import
                    .confidence_basis_points
                    .min(candidate.confidence_basis_points)
                    .min(component_match.confidence_basis_points);
                let component = component_match.component;
                let usage = SoftwareDependencyUsage::new(SoftwareDependencyUsageInput {
                    component_id: component.component_id.clone(),
                    repository_id: import.repository_id.clone(),
                    source_scope: import.source_scope.clone(),
                    ecosystem: component.ecosystem.clone(),
                    package_name: component.name.clone(),
                    language_id: import.language_id.clone(),
                    module: import.module.clone(),
                    target_hint: import.target_hint.clone(),
                    resolution_state: import.resolution_state.clone(),
                    evidence_path: import.evidence_path.clone(),
                    evidence_line_range: import.evidence_line_range.clone(),
                    confidence_basis_points: confidence,
                    created_graph_version: graph_version,
                })
                .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
                if seen_usage_ids.insert(usage.usage_id.clone()) {
                    usages.push(usage);
                }
            }
        }
    }

    usages.sort_by(|left, right| {
        left.ecosystem
            .cmp(&right.ecosystem)
            .then_with(|| left.package_name.cmp(&right.package_name))
            .then_with(|| left.evidence_path.cmp(&right.evidence_path))
            .then_with(|| {
                left.evidence_line_range
                    .start
                    .cmp(&right.evidence_line_range.start)
            })
    });
    Ok(usages)
}

#[cfg(test)]
#[path = "workflow_tests.rs"]
mod tests;

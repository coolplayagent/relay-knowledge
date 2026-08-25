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
pub(super) use persistence::{delete_scope, insert_usages, usages_for_scope};
pub(super) use schema::initialize_schema;

const MAX_COMPONENT_ALIAS_EVIDENCE_PER_SCOPE: usize = 65_536;
const MAX_IMPORT_EVIDENCE_PER_SCOPE: usize = 131_072;
const MAX_PYTHON_FILES_PER_SCOPE: usize = 131_072;
const MAX_PYTHON_LOCAL_MODULES_PER_SCOPE: usize = 262_144;
const MAX_DEPENDENCY_USAGES_PER_SCOPE: usize = 131_072;
const MAX_MATCH_CANDIDATES_PER_IMPORT: usize = 128;

pub(super) fn derive_dependency_usages(
    connection: &Connection,
    source_scope: &str,
    graph_version: GraphVersion,
    components: &[SoftwareComponent],
) -> Result<Vec<SoftwareDependencyUsage>, StorageError> {
    let alias_keys = component_alias_keys(
        connection,
        source_scope,
        MAX_COMPONENT_ALIAS_EVIDENCE_PER_SCOPE,
    )?;
    let index = DependencyMatchIndex::new(components, &alias_keys);
    if index.is_empty() {
        return Ok(Vec::new());
    }

    let imports = import_evidence(connection, source_scope, MAX_IMPORT_EVIDENCE_PER_SCOPE)?;
    let python_local_modules = python::local_modules(
        connection,
        source_scope,
        MAX_PYTHON_FILES_PER_SCOPE,
        MAX_PYTHON_LOCAL_MODULES_PER_SCOPE,
    )?;
    let mut seen_usage_ids = BTreeSet::new();
    let mut usages = Vec::new();
    for import in imports {
        let candidates = import_match_candidates_with_python_locals(
            &import.language_id,
            &import.module,
            import.target_hint.as_deref(),
            &import.resolution_state,
            Some(&python_local_modules),
            MAX_MATCH_CANDIDATES_PER_IMPORT,
        )?;
        for candidate in candidates {
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
                insert_bounded_usage(
                    &mut usages,
                    &mut seen_usage_ids,
                    usage,
                    MAX_DEPENDENCY_USAGES_PER_SCOPE,
                )?;
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

fn insert_bounded_usage(
    usages: &mut Vec<SoftwareDependencyUsage>,
    seen_usage_ids: &mut BTreeSet<String>,
    usage: SoftwareDependencyUsage,
    limit: usize,
) -> Result<(), StorageError> {
    if !seen_usage_ids.insert(usage.usage_id.clone()) {
        return Ok(());
    }
    if usages.len() >= limit {
        return Err(StorageError::CapacityExceeded(format!(
            "software dependency usages exceed the bounded limit {limit}"
        )));
    }
    usages.push(usage);
    Ok(())
}

#[cfg(test)]
#[path = "workflow_tests.rs"]
mod tests;

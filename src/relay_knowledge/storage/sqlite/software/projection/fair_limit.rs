//! Fair strict-total budgeting for the combined software projection.

use std::collections::{HashMap, HashSet};

use super::ProjectionSlices;

pub(super) fn apply_fair_total_limit(slices: &mut ProjectionSlices, total_limit: usize) {
    let [
        components,
        dependency_usages,
        sdk_usages,
        files,
        topics,
        relationships,
        build_targets,
        iac_resources,
        design_elements,
        entities,
        statements,
        diagnostics,
    ] = round_robin_slice_budgets(
        [
            slices.components.len(),
            slices.dependency_usages.len(),
            slices.sdk_usages.len(),
            slices.files.len(),
            slices.topics.len(),
            slices.relationships.len(),
            slices.build_targets.len(),
            slices.iac_resources.len(),
            slices.design_elements.len(),
            slices.entities.len(),
            slices.statements.len(),
            slices.diagnostics.len(),
        ],
        total_limit,
    );

    retain_components_referenced_by_dependency_usages(slices, components, dependency_usages);
    slices.sdk_usages.truncate(sdk_usages);
    slices.files.truncate(files);
    slices.topics.truncate(topics);
    slices.relationships.truncate(relationships);
    slices.build_targets.truncate(build_targets);
    slices.iac_resources.truncate(iac_resources);
    slices.design_elements.truncate(design_elements);
    slices.entities.truncate(entities);
    slices.statements.truncate(statements);
    slices.diagnostics.truncate(diagnostics);
}

fn retain_components_referenced_by_dependency_usages(
    slices: &mut ProjectionSlices,
    component_limit: usize,
    dependency_usage_limit: usize,
) {
    let component_candidates = slices
        .components
        .iter()
        .cloned()
        .map(|component| (component.component_id.clone(), component))
        .collect::<HashMap<_, _>>();
    slices.components.truncate(component_limit);
    let mut retained_component_ids = slices
        .components
        .iter()
        .map(|component| component.component_id.clone())
        .collect::<HashSet<_>>();
    let mut required_component_ids = HashSet::new();
    let mut retained_usages = Vec::with_capacity(dependency_usage_limit);

    for usage in slices.dependency_usages.drain(..dependency_usage_limit) {
        if !retained_component_ids.contains(&usage.component_id) {
            let Some(component) = component_candidates.get(&usage.component_id).cloned() else {
                continue;
            };
            let Some(index) = slices
                .components
                .iter()
                .position(|component| !required_component_ids.contains(&component.component_id))
            else {
                continue;
            };
            let replaced = std::mem::replace(&mut slices.components[index], component);
            retained_component_ids.remove(&replaced.component_id);
            retained_component_ids.insert(usage.component_id.clone());
        }
        required_component_ids.insert(usage.component_id.clone());
        retained_usages.push(usage);
    }
    slices.dependency_usages = retained_usages;
}

pub(super) fn round_robin_slice_budgets<const N: usize>(
    available_rows: [usize; N],
    total_limit: usize,
) -> [usize; N] {
    let mut budgets = [0; N];
    let mut remaining = total_limit;
    while remaining > 0 {
        let mut allocated_in_round = false;
        for index in 0..N {
            if budgets[index] < available_rows[index] {
                budgets[index] += 1;
                remaining -= 1;
                allocated_in_round = true;
                if remaining == 0 {
                    break;
                }
            }
        }
        if !allocated_in_round {
            break;
        }
    }
    budgets
}

//! Fair strict-total budgeting for the combined software projection.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use super::ProjectionSlices;

pub(super) fn apply_fair_total_limit(slices: &mut ProjectionSlices, total_limit: usize) {
    let component_candidates = slices
        .components
        .iter()
        .cloned()
        .map(|component| (component.component_id.clone(), component))
        .collect::<HashMap<_, _>>();
    slices
        .dependency_usages
        .retain(|usage| component_candidates.contains_key(&usage.component_id));
    let initial_budgets = round_robin_slice_budgets(
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
    retain_entities_referenced_by_statements(slices, initial_budgets[9], initial_budgets[10]);
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

    retain_components_referenced_by_dependency_usages(
        slices,
        components,
        dependency_usages,
        &component_candidates,
    );
    slices.sdk_usages.truncate(sdk_usages);
    slices.files.truncate(files);
    slices.topics.truncate(topics);
    slices.relationships.truncate(relationships);
    slices.build_targets.truncate(build_targets);
    slices.iac_resources.truncate(iac_resources);
    slices.design_elements.truncate(design_elements);
    debug_assert_eq!(
        slices.entities.len(),
        entities,
        "filtering later-priority statements cannot reduce the entity allocation"
    );
    slices.statements.truncate(statements);
    slices.diagnostics.truncate(diagnostics);
}

fn retain_entities_referenced_by_statements(
    slices: &mut ProjectionSlices,
    entity_limit: usize,
    statement_limit: usize,
) {
    let entity_candidates = slices
        .entities
        .iter()
        .cloned()
        .map(|entity| (entity.entity_key.clone(), entity))
        .collect::<HashMap<_, _>>();
    slices.entities.truncate(entity_limit);
    let mut retained_entity_counts =
        slices
            .entities
            .iter()
            .fold(BTreeMap::<String, usize>::new(), |mut counts, entity| {
                *counts.entry(entity.entity_key.clone()).or_default() += 1;
                counts
            });
    let mut required_entity_keys = BTreeSet::new();
    let mut retained_statements = Vec::with_capacity(statement_limit);

    for statement in slices.statements.drain(..) {
        if retained_statements.len() == statement_limit {
            break;
        }
        let referenced_entity_keys = std::iter::once(statement.subject_id.as_str())
            .chain(statement.object_id.as_deref())
            .collect::<BTreeSet<_>>();
        if !referenced_entity_keys
            .iter()
            .all(|entity_key| entity_candidates.contains_key(*entity_key))
        {
            continue;
        }
        let missing_entity_keys = referenced_entity_keys
            .iter()
            .filter(|entity_key| !retained_entity_counts.contains_key(**entity_key))
            .copied()
            .collect::<Vec<_>>();
        let mut replaceable_entity_counts = retained_entity_counts.clone();
        let replacement_indices = slices
            .entities
            .iter()
            .enumerate()
            .filter_map(|(index, entity)| {
                let remaining_count = replaceable_entity_counts
                    .get_mut(&entity.entity_key)
                    .expect("replacement candidates must be retained");
                let entity_key_is_required = required_entity_keys.contains(&entity.entity_key)
                    || referenced_entity_keys.contains(entity.entity_key.as_str());
                if entity_key_is_required && *remaining_count == 1 {
                    return None;
                }
                *remaining_count -= 1;
                Some(index)
            })
            .take(missing_entity_keys.len())
            .collect::<Vec<_>>();
        if replacement_indices.len() != missing_entity_keys.len() {
            continue;
        }
        for (index, entity_key) in replacement_indices.into_iter().zip(missing_entity_keys) {
            let replacement = entity_candidates
                .get(entity_key)
                .expect("statements were filtered to known entity candidates")
                .clone();
            let replaced = std::mem::replace(&mut slices.entities[index], replacement);
            let should_remove = {
                let count = retained_entity_counts
                    .get_mut(&replaced.entity_key)
                    .expect("replaced entities must be retained");
                *count -= 1;
                *count == 0
            };
            if should_remove {
                retained_entity_counts.remove(&replaced.entity_key);
            }
            *retained_entity_counts
                .entry(entity_key.to_owned())
                .or_default() += 1;
        }
        required_entity_keys.extend(referenced_entity_keys.into_iter().map(str::to_owned));
        retained_statements.push(statement);
    }
    slices.statements = retained_statements;
}

fn retain_components_referenced_by_dependency_usages(
    slices: &mut ProjectionSlices,
    component_limit: usize,
    dependency_usage_limit: usize,
    component_candidates: &HashMap<String, super::SoftwareComponent>,
) {
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
            let component = component_candidates
                .get(&usage.component_id)
                .expect("dependency usages were filtered to known components")
                .clone();
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

#[cfg(test)]
#[path = "fair_limit_tests.rs"]
mod tests;

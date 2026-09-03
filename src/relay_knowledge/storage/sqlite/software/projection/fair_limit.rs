//! Fair strict-total budgeting for the combined software projection.

use std::collections::{BTreeSet, HashMap, HashSet};

use super::ProjectionSlices;

pub(super) fn apply_fair_total_limit(slices: &mut ProjectionSlices, total_limit: usize) {
    let component_candidates = std::mem::take(&mut slices.components);
    let component_indices_by_id = component_candidates.iter().enumerate().fold(
        HashMap::<&str, usize>::new(),
        |mut indices, (index, component)| {
            indices.insert(component.component_id.as_str(), index);
            indices
        },
    );
    slices
        .dependency_usages
        .retain(|usage| component_indices_by_id.contains_key(usage.component_id.as_str()));
    let initial_budgets = round_robin_slice_budgets(
        [
            component_candidates.len(),
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
    let retained_component_indices = retain_components_referenced_by_dependency_usages(
        &component_candidates,
        &component_indices_by_id,
        &mut slices.dependency_usages,
        initial_budgets[0],
        initial_budgets[1],
    );
    let entity_candidates = std::mem::take(&mut slices.entities);
    let retained_entity_indices = retain_entities_referenced_by_statements(
        &entity_candidates,
        &mut slices.statements,
        initial_budgets[9],
        initial_budgets[10],
    );
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
            component_candidates.len(),
            slices.dependency_usages.len(),
            slices.sdk_usages.len(),
            slices.files.len(),
            slices.topics.len(),
            slices.relationships.len(),
            slices.build_targets.len(),
            slices.iac_resources.len(),
            slices.design_elements.len(),
            entity_candidates.len(),
            slices.statements.len(),
            slices.diagnostics.len(),
        ],
        total_limit,
    );

    slices.components = selected_components(
        &component_candidates,
        &retained_component_indices,
        components,
    );
    debug_assert_eq!(slices.components.len(), components);
    slices.dependency_usages.truncate(dependency_usages);
    slices.sdk_usages.truncate(sdk_usages);
    slices.files.truncate(files);
    slices.topics.truncate(topics);
    slices.relationships.truncate(relationships);
    slices.build_targets.truncate(build_targets);
    slices.iac_resources.truncate(iac_resources);
    slices.design_elements.truncate(design_elements);
    slices.entities = selected_entities(&entity_candidates, &retained_entity_indices, entities);
    debug_assert_eq!(slices.entities.len(), entities);
    slices.statements.truncate(statements);
    slices.diagnostics.truncate(diagnostics);
}

fn retain_entities_referenced_by_statements(
    entity_candidates: &[super::SoftwareEntity],
    statements: &mut Vec<super::SoftwareStatement>,
    entity_limit: usize,
    statement_limit: usize,
) -> BTreeSet<usize> {
    let entity_indices_by_key = entity_candidates.iter().enumerate().fold(
        HashMap::<&str, Vec<usize>>::new(),
        |mut indices, (index, entity)| {
            indices
                .entry(entity.entity_key.as_str())
                .or_default()
                .push(index);
            indices
        },
    );
    let mut retained_indices =
        (0..entity_candidates.len().min(entity_limit)).collect::<BTreeSet<_>>();
    let mut retained_entity_counts =
        retained_indices
            .iter()
            .fold(HashMap::<&str, usize>::new(), |mut counts, index| {
                *counts
                    .entry(entity_candidates[*index].entity_key.as_str())
                    .or_default() += 1;
                counts
            });
    let mut required_entity_keys = BTreeSet::new();
    let mut retained_statements = Vec::with_capacity(statement_limit);

    for statement in std::mem::take(statements) {
        if retained_statements.len() == statement_limit {
            break;
        }
        let referenced_entity_keys = std::iter::once(statement.subject_id.clone())
            .chain(statement.object_id.iter().cloned())
            .collect::<BTreeSet<_>>();
        if !referenced_entity_keys
            .iter()
            .all(|entity_key| entity_indices_by_key.contains_key(entity_key.as_str()))
        {
            continue;
        }
        let missing_entity_keys = referenced_entity_keys
            .iter()
            .filter(|entity_key| !retained_entity_counts.contains_key(entity_key.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        let mut replaceable_entity_counts = retained_entity_counts.clone();
        let replacement_indices = retained_indices
            .iter()
            .filter_map(|index| {
                let entity = &entity_candidates[*index];
                let remaining_count = replaceable_entity_counts
                    .get_mut(entity.entity_key.as_str())
                    .expect("replacement candidates must be retained");
                let entity_key_is_required = required_entity_keys
                    .contains(entity.entity_key.as_str())
                    || referenced_entity_keys.contains(entity.entity_key.as_str());
                if entity_key_is_required && *remaining_count == 1 {
                    return None;
                }
                *remaining_count -= 1;
                Some(*index)
            })
            .take(missing_entity_keys.len())
            .collect::<Vec<_>>();
        if replacement_indices.len() != missing_entity_keys.len() {
            continue;
        }
        for (index, entity_key) in replacement_indices.into_iter().zip(missing_entity_keys) {
            let replacement_index = entity_indices_by_key
                .get(entity_key.as_str())
                .and_then(|indices| indices.first())
                .copied()
                .expect("statements were filtered to known entity candidates");
            let replaced = &entity_candidates[index];
            let replacement = &entity_candidates[replacement_index];
            retained_indices.remove(&index);
            let inserted = retained_indices.insert(replacement_index);
            debug_assert!(inserted, "missing entity keys cannot already be retained");
            let should_remove = {
                let count = retained_entity_counts
                    .get_mut(replaced.entity_key.as_str())
                    .expect("replaced entities must be retained");
                *count -= 1;
                *count == 0
            };
            if should_remove {
                retained_entity_counts.remove(replaced.entity_key.as_str());
            }
            *retained_entity_counts
                .entry(replacement.entity_key.as_str())
                .or_default() += 1;
        }
        required_entity_keys.extend(referenced_entity_keys);
        retained_statements.push(statement);
    }
    *statements = retained_statements;
    retained_indices
}

fn selected_entities(
    entity_candidates: &[super::SoftwareEntity],
    retained_indices: &BTreeSet<usize>,
    entity_limit: usize,
) -> Vec<super::SoftwareEntity> {
    debug_assert!(
        retained_indices.len() <= entity_limit,
        "redistributing rejected statement capacity cannot reduce the entity allocation"
    );
    let mut selected_indices = retained_indices.clone();
    for index in 0..entity_candidates.len() {
        if selected_indices.len() == entity_limit {
            break;
        }
        selected_indices.insert(index);
    }
    selected_indices
        .into_iter()
        .map(|index| entity_candidates[index].clone())
        .collect()
}

fn retain_components_referenced_by_dependency_usages(
    component_candidates: &[super::SoftwareComponent],
    component_indices_by_id: &HashMap<&str, usize>,
    dependency_usages: &mut Vec<super::SoftwareDependencyUsage>,
    component_limit: usize,
    dependency_usage_limit: usize,
) -> BTreeSet<usize> {
    let mut retained_indices =
        (0..component_candidates.len().min(component_limit)).collect::<BTreeSet<_>>();
    let mut retained_component_ids = retained_indices
        .iter()
        .map(|index| component_candidates[*index].component_id.as_str())
        .collect::<HashSet<_>>();
    let mut required_component_ids = HashSet::new();
    let mut retained_usages = Vec::with_capacity(dependency_usage_limit);

    for usage in std::mem::take(dependency_usages)
        .into_iter()
        .take(dependency_usage_limit)
    {
        let component_id = usage.component_id.as_str();
        let Some(target_index) = component_indices_by_id.get(component_id).copied() else {
            continue;
        };
        if !retained_component_ids.contains(component_id) {
            let Some(index) = retained_indices
                .iter()
                .find(|index| {
                    !required_component_ids
                        .contains(component_candidates[**index].component_id.as_str())
                })
                .copied()
            else {
                continue;
            };
            let replaced = &component_candidates[index];
            retained_indices.remove(&index);
            let inserted = retained_indices.insert(target_index);
            debug_assert!(inserted, "missing components cannot already be retained");
            retained_component_ids.remove(replaced.component_id.as_str());
            retained_component_ids.insert(component_candidates[target_index].component_id.as_str());
        }
        required_component_ids.insert(component_candidates[target_index].component_id.as_str());
        retained_usages.push(usage);
    }
    *dependency_usages = retained_usages;
    retained_indices
}

fn selected_components(
    component_candidates: &[super::SoftwareComponent],
    retained_indices: &BTreeSet<usize>,
    component_limit: usize,
) -> Vec<super::SoftwareComponent> {
    debug_assert!(
        retained_indices.len() <= component_limit,
        "filtering dependency usages cannot reduce the component allocation"
    );
    let mut selected_indices = retained_indices.clone();
    for index in 0..component_candidates.len() {
        if selected_indices.len() == component_limit {
            break;
        }
        selected_indices.insert(index);
    }
    selected_indices
        .into_iter()
        .map(|index| component_candidates[index].clone())
        .collect()
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

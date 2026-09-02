//! Fair strict-total budgeting for the combined software projection.

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

    slices.components.truncate(components);
    slices.dependency_usages.truncate(dependency_usages);
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

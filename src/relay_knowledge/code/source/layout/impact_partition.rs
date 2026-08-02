use std::path::PathBuf;

use crate::{
    code::{
        CodeIndexError,
        source::repository::{source_commit_is_filesystem, source_snapshot},
    },
    domain::{CodeImpactPathGroups, CodeRepositoryRegistration, CodeRepositorySelector},
};

use super::{
    discovery::discover_source_layout,
    scoped_snapshot::{filesystem_policy_for_selector, scoped_source_snapshot},
    selection::selection_exclusion_reason_for_source,
};

/// Splits diff paths by the same selector rules used by indexing and impact.
pub fn partition_changed_paths_for_selector(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    paths: Vec<String>,
) -> Result<CodeImpactPathGroups, CodeIndexError> {
    if paths.is_empty() {
        return Ok(CodeImpactPathGroups {
            in_scope_changed_paths: Vec::new(),
            out_of_scope_changed_paths: Vec::new(),
        });
    }
    let root = PathBuf::from(&registration.root_path);
    let filesystem_policy = filesystem_policy_for_selector(registration, selector);
    let (source_layout, source_kind) = if source_commit_is_filesystem(&selector.ref_selector) {
        let snapshot =
            scoped_source_snapshot(registration, selector, &root, &selector.ref_selector)?;
        (discover_source_layout(&snapshot.entries), snapshot.kind)
    } else {
        let snapshot = source_snapshot(&root, &selector.ref_selector, filesystem_policy)?;
        (discover_source_layout(&snapshot.entries), snapshot.kind)
    };
    let mut in_scope_changed_paths = Vec::new();
    let mut out_of_scope_changed_paths = Vec::new();
    for path in paths {
        if selection_exclusion_reason_for_source(
            &path,
            registration,
            selector,
            &source_layout,
            source_kind,
        )
        .is_none()
        {
            in_scope_changed_paths.push(path);
        } else {
            out_of_scope_changed_paths.push(path);
        }
    }
    in_scope_changed_paths.sort();
    in_scope_changed_paths.dedup();
    out_of_scope_changed_paths.sort();
    out_of_scope_changed_paths.dedup();

    Ok(CodeImpactPathGroups {
        in_scope_changed_paths,
        out_of_scope_changed_paths,
    })
}

#[cfg(test)]
#[path = "impact_partition_tests.rs"]
mod tests;

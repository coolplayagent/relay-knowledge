use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use crate::domain::{CodeRepositoryRegistration, CodeRepositorySelector};

use super::{MAX_INCREMENTAL_GITLINK_EXPANDED_PATHS, tracked_entry_scope_for_selector};
use crate::code::{
    CodeIndexError,
    changes::{self, GitChange, diff_changes, tracked_entries_with_scope},
    git::resolve_ref,
    parser::parse_indexed_file,
    scope::{self, discover_source_layout, path_is_selected_with_layout, path_scope_overlaps},
    snapshot::SnapshotBuild,
    source::{
        gitlink as source_gitlink, gitlink::paths as source_gitlink_paths,
        source_bytes_after_content_verification, source_commit_is_filesystem, source_kind,
    },
};

/// Extracts symbol names removed by a diff so impact can include deleted APIs.
pub fn deleted_symbol_names_for_diff(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    base_ref: &str,
    head_ref: &str,
) -> Result<Vec<String>, CodeIndexError> {
    let root = PathBuf::from(&registration.root_path);
    if source_commit_is_filesystem(base_ref) || source_commit_is_filesystem(head_ref) {
        return Ok(Vec::new());
    }
    if source_kind(&root)?.is_filesystem() {
        return Ok(Vec::new());
    }
    let base_commit = resolve_ref(&root, base_ref)?;
    let head_commit = resolve_ref(&root, head_ref)?;
    let changes = diff_changes(&root, base_ref, head_ref)?;
    let entry_scope = tracked_entry_scope_for_selector(registration, selector);
    let base_entries = tracked_entries_with_scope(&root, &base_commit, &entry_scope)?;
    let source_layout = discover_source_layout(&base_entries);
    let context = DeletedSymbolContext {
        registration,
        selector,
        root: &root,
        base_commit: &base_commit,
        source_layout: &source_layout,
    };
    let mut names = Vec::new();

    for change in changes {
        match change {
            GitChange::Deleted { path } | GitChange::Renamed { old_path: path, .. } => {
                append_deleted_symbol_names_for_removed_path(
                    &mut names,
                    &context,
                    &base_entries,
                    &path,
                )?;
            }
            GitChange::AddedOrModified { path } | GitChange::TypeChanged { path } => {
                append_deleted_symbol_names_for_gitlink_update(
                    &mut names,
                    &context,
                    &head_commit,
                    &path,
                )?;
            }
            GitChange::Copied { .. } => {}
        }
    }
    names.sort();
    names.dedup();

    Ok(names)
}

struct DeletedSymbolContext<'a> {
    registration: &'a CodeRepositoryRegistration,
    selector: &'a CodeRepositorySelector,
    root: &'a Path,
    base_commit: &'a str,
    source_layout: &'a scope::SourceLayoutDiscovery,
}

fn append_deleted_symbol_names_for_removed_path(
    names: &mut Vec<String>,
    context: &DeletedSymbolContext<'_>,
    base_entries: &[changes::GitTreeEntry],
    path: &str,
) -> Result<(), CodeIndexError> {
    if source_gitlink::gitlink_commit_at_tree(context.root, context.base_commit, path)?.is_some() {
        let include_expanded_path = |path: &str| {
            path_is_selected_with_layout(
                path,
                context.registration,
                context.selector,
                context.source_layout,
            )
        };
        let paths = source_gitlink_paths::bounded_expanded_paths_under_with_selector(
            base_entries,
            path,
            MAX_INCREMENTAL_GITLINK_EXPANDED_PATHS,
            &source_gitlink::GitlinkPathSelector::new(
                &include_expanded_path,
                &include_expanded_path,
            ),
        )?;
        for path in paths {
            append_deleted_symbol_names_for_path(names, context, context.base_commit, &path)?;
        }
        return Ok(());
    }

    append_deleted_symbol_names_for_path(names, context, context.base_commit, path)
}

fn append_deleted_symbol_names_for_gitlink_update(
    names: &mut Vec<String>,
    context: &DeletedSymbolContext<'_>,
    head_commit: &str,
    path: &str,
) -> Result<(), CodeIndexError> {
    if !path_scope_overlaps(path, context.registration, context.selector) {
        return Ok(());
    }
    let include_expanded_path = |path: &str| {
        path_is_selected_with_layout(
            path,
            context.registration,
            context.selector,
            context.source_layout,
        )
    };
    let expanded_scope_overlaps =
        |path: &str| path_scope_overlaps(path, context.registration, context.selector);
    let child_filters = |path: &str| {
        scope::submodule_child_scope_filters(path, context.registration, context.selector)
    };
    let Some(expansion) = source_gitlink::changed_gitlink_path_expansion(
        context.root,
        path,
        context.base_commit,
        head_commit,
        MAX_INCREMENTAL_GITLINK_EXPANDED_PATHS,
        &source_gitlink::GitlinkPathSelector::new_with_child_filters(
            &include_expanded_path,
            &expanded_scope_overlaps,
            &child_filters,
        ),
    )?
    else {
        return Ok(());
    };
    if !expansion.base_is_gitlink {
        append_deleted_symbol_names_for_path(names, context, context.base_commit, path)?;
        return Ok(());
    }
    if expansion.base_paths.is_empty() {
        return Ok(());
    }
    for path in expansion.base_paths {
        if !path_is_selected_with_layout(
            &path,
            context.registration,
            context.selector,
            context.source_layout,
        ) {
            continue;
        }
        let mut removed = symbol_names_for_path(context, context.base_commit, &path)?;
        if expansion.head_paths.contains(&path) {
            let retained = symbol_names_for_path(context, head_commit, &path)?;
            removed.retain(|name| !retained.contains(name));
        }
        names.extend(removed);
    }

    Ok(())
}

fn append_deleted_symbol_names_for_path(
    names: &mut Vec<String>,
    context: &DeletedSymbolContext<'_>,
    commit: &str,
    path: &str,
) -> Result<(), CodeIndexError> {
    if !path_is_selected_with_layout(
        path,
        context.registration,
        context.selector,
        context.source_layout,
    ) {
        return Ok(());
    }
    names.extend(symbol_names_for_path(context, commit, path)?);

    Ok(())
}

fn symbol_names_for_path(
    context: &DeletedSymbolContext<'_>,
    commit: &str,
    path: &str,
) -> Result<BTreeSet<String>, CodeIndexError> {
    let bytes = source_bytes_after_content_verification(context.root, commit, path, None)?;
    let mut build = SnapshotBuild::new_with_selector(
        context.registration,
        context.selector,
        commit.to_owned(),
        "deleted-symbol-seed".to_owned(),
        true,
        1,
        0,
    );
    parse_indexed_file(&mut build, path, &bytes)?;

    Ok(build
        .symbols
        .into_iter()
        .map(|symbol| symbol.name)
        .collect())
}

//! Go module and `go.work` workspace prefix discovery.

use std::collections::{BTreeMap, BTreeSet};

use super::{ManifestChunk, module_key::ModulePrefix, normalize_module_key, path};

#[derive(Debug, Clone)]
pub(super) struct Workspace {
    root_path_prefix: String,
    module_dirs: BTreeSet<String>,
}

pub(super) fn workspaces(chunks: &[ManifestChunk]) -> Vec<Workspace> {
    chunks
        .iter()
        .filter(|chunk| path::is_go_work(&chunk.path))
        .filter_map(|chunk| workspace(&chunk.path, &chunk.content))
        .collect()
}

fn workspace(manifest_path: &str, content: &str) -> Option<Workspace> {
    let root_path_prefix = path::parent(manifest_path);
    let mut module_dirs = BTreeSet::new();
    collect_work_dirs(&root_path_prefix, content, &mut module_dirs);
    (!module_dirs.is_empty()).then_some(Workspace {
        root_path_prefix,
        module_dirs,
    })
}

pub(super) fn module_allowed(manifest_path: &str, workspaces: &[Workspace]) -> bool {
    if workspaces.is_empty() {
        return true;
    }
    let module_dir = manifest_path_prefix(manifest_path);
    let mut governed_by_workspace = false;
    for workspace in workspaces {
        if path::is_at_or_below_root(&module_dir, &workspace.root_path_prefix) {
            governed_by_workspace = true;
            if workspace.module_dirs.contains(&module_dir) {
                return true;
            }
        }
    }

    !governed_by_workspace
}

pub(super) fn collect_module_prefixes(
    manifest_path: &str,
    content: &str,
    prefixes: &mut Vec<ModulePrefix>,
) {
    let source_path_prefix = manifest_path_prefix(manifest_path);
    for line in content.lines() {
        let Some(module_key) = module_prefix(line) else {
            continue;
        };
        let prefix = ModulePrefix {
            source_path_prefix: source_path_prefix.clone(),
            module_key,
            path_aliases: BTreeMap::new(),
            path_alias_patterns: Vec::new(),
            exposes_package_paths: true,
            exposes_root_package_key: true,
        };
        if !prefixes.contains(&prefix) {
            prefixes.push(prefix);
        }
    }
}

fn collect_work_dirs(root: &str, content: &str, dirs: &mut BTreeSet<String>) {
    let mut in_use_block = false;
    for line in content.lines() {
        let line = line.split("//").next().unwrap_or_default().trim();
        if line.is_empty() {
            continue;
        }
        if in_use_block {
            if line.starts_with(')') {
                in_use_block = false;
                continue;
            }
            if let Some(joined) = path::join_workspace_path(root, work_path_token(line)) {
                dirs.insert(joined);
            }
            continue;
        }
        let Some(rest) = line
            .strip_prefix("use")
            .filter(|rest| rest.starts_with(char::is_whitespace))
        else {
            continue;
        };
        let rest = rest.trim();
        if rest.starts_with('(') {
            in_use_block = true;
            continue;
        }
        if let Some(joined) = path::join_workspace_path(root, work_path_token(rest)) {
            dirs.insert(joined);
        }
    }
}

fn work_path_token(value: &str) -> &str {
    value
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim_matches(['"', '\'', '`'])
}

fn module_prefix(line: &str) -> Option<String> {
    let line = line.split("//").next()?.trim();
    let module = line
        .strip_prefix("module")
        .filter(|rest| rest.starts_with(char::is_whitespace))?
        .trim();
    if module.is_empty() {
        return None;
    }
    let normalized = normalize_module_key(module.trim_matches(['"', '\'', '`']));

    (!normalized.is_empty() && normalized.contains('.')).then_some(normalized)
}

fn manifest_path_prefix(manifest_path: &str) -> String {
    let manifest_path = path::clean(manifest_path);
    manifest_path
        .strip_suffix("/go.mod")
        .filter(|prefix| !prefix.is_empty())
        .unwrap_or_default()
        .to_owned()
}

#[cfg(test)]
#[path = "go_tests.rs"]
mod tests;

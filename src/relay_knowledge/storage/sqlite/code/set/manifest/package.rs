//! pnpm workspace and package.json export-prefix discovery.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::Value;

use super::{
    ManifestChunk,
    module_key::{ModulePrefix, PathAliasPattern},
    normalize_module_key, path,
};

#[derive(Debug, Deserialize)]
struct PnpmWorkspaceManifest {
    packages: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct PackageJsonManifest {
    name: Option<String>,
    main: Option<String>,
    module: Option<String>,
    types: Option<String>,
    typings: Option<String>,
    exports: Option<Value>,
}

#[derive(Debug, Clone)]
pub(super) struct Workspace {
    root_path_prefix: String,
    includes: Vec<String>,
    excludes: Vec<String>,
}

pub(super) fn workspaces(chunks: &[ManifestChunk]) -> Vec<Workspace> {
    chunks
        .iter()
        .filter(|chunk| path::is_pnpm_workspace(&chunk.path))
        .filter_map(|chunk| workspace(&chunk.path, &chunk.content))
        .collect()
}

fn workspace(manifest_path: &str, content: &str) -> Option<Workspace> {
    let manifest = serde_norway::from_str::<PnpmWorkspaceManifest>(content).ok()?;
    let mut includes = vec![".".to_owned()];
    let mut excludes = Vec::new();
    if let Some(packages) = manifest.packages {
        for package in packages {
            let package = package.trim().trim_matches(['"', '\'']).to_owned();
            if package.is_empty() {
                continue;
            }
            if let Some(excluded) = package.strip_prefix('!') {
                excludes.push(path::clean(excluded));
            } else {
                includes.push(path::clean(&package));
            }
        }
    }
    Some(Workspace {
        root_path_prefix: path::parent(manifest_path),
        includes,
        excludes,
    })
}

pub(super) fn collect_prefixes(
    manifest_path: &str,
    content: &str,
    workspaces: &[Workspace],
    prefixes: &mut Vec<ModulePrefix>,
) {
    let source_path_prefix = path::parent(manifest_path);
    if path::package_is_ignored(&source_path_prefix)
        || !package_allowed_by_workspace(&source_path_prefix, workspaces)
    {
        return;
    }
    let Ok(manifest) = serde_json::from_str::<PackageJsonManifest>(content) else {
        return;
    };
    let Some(module_key) = manifest
        .name
        .as_deref()
        .map(normalize_module_key)
        .filter(|name| !name.is_empty())
    else {
        return;
    };
    let (path_aliases, path_alias_patterns) = package_path_aliases(&manifest, &module_key);
    let prefix = ModulePrefix {
        source_path_prefix,
        module_key,
        path_aliases,
        path_alias_patterns,
        exposes_package_paths: manifest.exports.is_none(),
        exposes_root_package_key: false,
    };
    if !prefixes.contains(&prefix) {
        prefixes.push(prefix);
    }
}

fn package_path_aliases(
    manifest: &PackageJsonManifest,
    module_key: &str,
) -> (BTreeMap<String, BTreeSet<String>>, Vec<PathAliasPattern>) {
    let mut aliases = BTreeMap::<String, BTreeSet<String>>::new();
    let mut patterns = Vec::new();
    if let Some(exports) = &manifest.exports {
        add_export_aliases(&mut aliases, &mut patterns, module_key, exports);
    } else {
        let mut has_explicit_entry = false;
        for path in [
            manifest.main.as_deref(),
            manifest.module.as_deref(),
            manifest.types.as_deref(),
            manifest.typings.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            has_explicit_entry |= add_package_path_alias(&mut aliases, path, module_key);
        }
        if !has_explicit_entry {
            for path in default_package_entry_paths() {
                add_package_path_alias(&mut aliases, path, module_key);
            }
        }
    }

    (aliases, patterns)
}

fn default_package_entry_paths() -> [&'static str; 8] {
    [
        "index.ts",
        "index.tsx",
        "index.js",
        "index.jsx",
        "src/index.ts",
        "src/index.tsx",
        "src/index.js",
        "src/index.jsx",
    ]
}

fn add_export_aliases(
    aliases: &mut BTreeMap<String, BTreeSet<String>>,
    patterns: &mut Vec<PathAliasPattern>,
    module_key: &str,
    exports: &Value,
) {
    let Some(object) = exports.as_object() else {
        add_export_target_alias(aliases, exports, module_key);
        return;
    };
    let has_subpath_keys = object.keys().any(|key| key == "." || key.starts_with("./"));
    if !has_subpath_keys {
        add_export_target_alias(aliases, exports, module_key);
        return;
    }
    for (key, value) in object {
        if key == "." {
            add_export_target_alias(aliases, value, module_key);
        } else if let Some(subpath) = key
            .strip_prefix("./")
            .filter(|subpath| !subpath.is_empty() && !subpath.contains(".."))
        {
            if subpath.contains('*') {
                add_export_pattern_alias(patterns, module_key, subpath, value);
            } else {
                let alias = format!("{module_key}.{}", normalize_module_key(subpath));
                add_export_target_alias(aliases, value, &alias);
            }
        }
    }
}

fn add_export_target_alias(
    aliases: &mut BTreeMap<String, BTreeSet<String>>,
    value: &Value,
    alias_key: &str,
) -> bool {
    match value {
        Value::String(path) => add_package_path_alias(aliases, path, alias_key),
        Value::Array(values) => values
            .iter()
            .any(|value| add_export_target_alias(aliases, value, alias_key)),
        Value::Object(entries) => {
            for condition in export_condition_priority() {
                if let Some(value) = entries.get(*condition)
                    && add_export_target_alias(aliases, value, alias_key)
                {
                    return true;
                }
            }
            entries.iter().any(|(condition, value)| {
                !is_prioritized_export_condition(condition)
                    && add_export_target_alias(aliases, value, alias_key)
            })
        }
        _ => false,
    }
}

fn add_export_pattern_alias(
    patterns: &mut Vec<PathAliasPattern>,
    module_key: &str,
    subpath: &str,
    value: &Value,
) {
    let Some((alias_prefix, alias_suffix)) = export_alias_pattern(module_key, subpath) else {
        return;
    };
    add_export_target_pattern_alias(patterns, value, &alias_prefix, &alias_suffix);
}

fn add_export_target_pattern_alias(
    patterns: &mut Vec<PathAliasPattern>,
    value: &Value,
    alias_prefix: &str,
    alias_suffix: &str,
) -> bool {
    match value {
        Value::String(path) => {
            let Some((path_prefix, path_suffix)) = package_entry_pattern(path) else {
                return false;
            };
            let pattern = PathAliasPattern {
                path_prefix,
                path_suffix,
                alias_prefix: alias_prefix.to_owned(),
                alias_suffix: alias_suffix.to_owned(),
            };
            if !patterns.contains(&pattern) {
                patterns.push(pattern);
            }
            true
        }
        Value::Array(values) => values.iter().any(|value| {
            add_export_target_pattern_alias(patterns, value, alias_prefix, alias_suffix)
        }),
        Value::Object(entries) => {
            for condition in export_condition_priority() {
                if let Some(value) = entries.get(*condition)
                    && add_export_target_pattern_alias(patterns, value, alias_prefix, alias_suffix)
                {
                    return true;
                }
            }
            entries.iter().any(|(condition, value)| {
                !is_prioritized_export_condition(condition)
                    && add_export_target_pattern_alias(patterns, value, alias_prefix, alias_suffix)
            })
        }
        _ => false,
    }
}

fn add_package_path_alias(
    aliases: &mut BTreeMap<String, BTreeSet<String>>,
    path: &str,
    alias_key: &str,
) -> bool {
    let Some(path) = package_entry_path(path) else {
        return false;
    };
    aliases
        .entry(path)
        .or_default()
        .insert(alias_key.to_owned())
}

fn export_condition_priority() -> &'static [&'static str] {
    &[
        "import", "default", "require", "node", "browser", "types", "typings",
    ]
}

fn is_prioritized_export_condition(condition: &str) -> bool {
    export_condition_priority().contains(&condition)
}

fn export_alias_pattern(module_key: &str, subpath: &str) -> Option<(String, String)> {
    let (prefix, suffix) = split_single_wildcard(subpath)?;
    if prefix.contains("..") || suffix.contains("..") {
        return None;
    }
    let normalized_prefix = normalize_module_key(prefix.trim_matches('/'));
    let mut alias_prefix = module_key.to_owned();
    if !normalized_prefix.is_empty() {
        alias_prefix.push('.');
        alias_prefix.push_str(&normalized_prefix);
    }
    Some((alias_prefix, normalize_module_key(suffix.trim_matches('/'))))
}

fn package_entry_pattern(value: &str) -> Option<(String, String)> {
    let path = path::clean(value.trim().trim_matches(['"', '\'']));
    if path.is_empty()
        || path.starts_with('#')
        || path.starts_with('@')
        || path.contains("://")
        || path.split('/').any(|segment| segment == "..")
    {
        return None;
    }
    let (prefix, suffix) = split_single_wildcard(&path)?;
    Some((prefix.to_owned(), suffix.to_owned()))
}

fn split_single_wildcard(value: &str) -> Option<(&str, &str)> {
    let (prefix, suffix) = value.split_once('*')?;
    (!suffix.contains('*')).then_some((prefix, suffix))
}

fn package_entry_path(value: &str) -> Option<String> {
    let path = path::clean(value.trim().trim_matches(['"', '\'']));
    if path.is_empty()
        || path.contains('*')
        || path.starts_with('#')
        || path.starts_with('@')
        || path.contains("://")
        || path.split('/').any(|segment| segment == "..")
    {
        return None;
    }

    Some(path)
}

fn package_allowed_by_workspace(package_path_prefix: &str, workspaces: &[Workspace]) -> bool {
    if workspaces.is_empty() {
        return true;
    }
    let mut governed_by_workspace = false;
    for workspace in workspaces {
        let Some(relative_path) =
            path::workspace_relative_path(package_path_prefix, &workspace.root_path_prefix)
        else {
            continue;
        };
        governed_by_workspace = true;
        if workspace.includes.iter().any(|pattern| {
            path::workspace_pattern_matches(pattern, &relative_path)
                && !workspace
                    .excludes
                    .iter()
                    .any(|pattern| path::workspace_pattern_matches(pattern, &relative_path))
        }) {
            return true;
        }
    }

    !governed_by_workspace
}

#[cfg(test)]
#[path = "package_tests.rs"]
mod tests;

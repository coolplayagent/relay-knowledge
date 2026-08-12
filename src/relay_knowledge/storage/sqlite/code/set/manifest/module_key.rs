//! Manifest-derived module identity and source-path alias expansion.

use std::collections::{BTreeMap, BTreeSet};

use super::path;

#[derive(Debug, Clone, Eq, PartialEq)]
pub(in super::super) struct ModulePrefix {
    pub(super) source_path_prefix: String,
    pub(super) module_key: String,
    pub(super) path_aliases: BTreeMap<String, BTreeSet<String>>,
    pub(super) path_alias_patterns: Vec<PathAliasPattern>,
    pub(super) exposes_package_paths: bool,
    pub(super) exposes_root_package_key: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) struct PathAliasPattern {
    pub(super) path_prefix: String,
    pub(super) path_suffix: String,
    pub(super) alias_prefix: String,
    pub(super) alias_suffix: String,
}

pub(super) fn module_prefix_item_count(prefixes: &[ModulePrefix]) -> usize {
    prefixes.iter().fold(0usize, |total, prefix| {
        let alias_count = prefix
            .path_aliases
            .values()
            .fold(0usize, |count, aliases| count.saturating_add(aliases.len()));
        total
            .saturating_add(1)
            .saturating_add(prefix.path_aliases.len())
            .saturating_add(alias_count)
            .saturating_add(prefix.path_alias_patterns.len())
    })
}

impl PathAliasPattern {
    fn alias_for_path(&self, path: &str) -> Option<String> {
        if !path.starts_with(&self.path_prefix) || !path.ends_with(&self.path_suffix) {
            return None;
        }
        let capture_end = path.len().checked_sub(self.path_suffix.len())?;
        let capture = path.get(self.path_prefix.len()..capture_end)?;
        if capture.is_empty() || capture.split('/').any(|segment| segment == "..") {
            return None;
        }
        let capture = normalize_module_key(capture);
        if capture.is_empty() {
            return None;
        }
        let mut alias = self.alias_prefix.clone();
        alias.push('.');
        alias.push_str(&capture);
        if !self.alias_suffix.is_empty() {
            alias.push('.');
            alias.push_str(&self.alias_suffix);
        }
        Some(alias)
    }
}

pub(in super::super) fn module_keys_for_path_with_prefixes(
    path: &str,
    prefixes: &[ModulePrefix],
) -> BTreeSet<String> {
    module_keys_for_path_with_prefixes_inner(path, prefixes, true)
}

pub(in super::super) fn module_keys_for_symbol_path_with_prefixes(
    path: &str,
    prefixes: &[ModulePrefix],
) -> BTreeSet<String> {
    module_keys_for_path_with_prefixes_inner(path, prefixes, false)
}

fn module_keys_for_path_with_prefixes_inner(
    path: &str,
    prefixes: &[ModulePrefix],
    include_path_aliases: bool,
) -> BTreeSet<String> {
    let mut keys = module_keys_for_path(path);
    for prefix in prefixes {
        let Some(relative_path) = path_relative_to_module(path, &prefix.source_path_prefix) else {
            continue;
        };
        let relative_path = path::clean(&relative_path);
        if include_path_aliases && let Some(aliases) = prefix.path_aliases.get(&relative_path) {
            keys.extend(aliases.iter().cloned());
        }
        if include_path_aliases {
            keys.extend(
                prefix
                    .path_alias_patterns
                    .iter()
                    .filter_map(|pattern| pattern.alias_for_path(&relative_path)),
            );
        }
        if prefix.exposes_package_paths {
            extend_with_module_prefix(
                &mut keys,
                &prefix.module_key,
                &relative_path,
                prefix.exposes_root_package_key,
            );
        }
    }
    keys
}

pub(in super::super) fn normalize_module_key(value: &str) -> String {
    let mut value = value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim_end_matches(';')
        .trim();
    if let Some(stripped) = value.strip_prefix("use ") {
        value = stripped.trim();
    } else if let Some(stripped) = value.strip_prefix("import ") {
        value = stripped.trim();
    }
    value
        .replace("::", ".")
        .replace(['/', '\\', '-'], ".")
        .replace(['{', '}', ','], ".")
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim_matches('.')
        .to_lowercase()
}

fn module_keys_for_path(path: &str) -> BTreeSet<String> {
    let without_extension = path
        .rsplit_once('.')
        .map(|(left, _)| left)
        .unwrap_or(path)
        .trim_start_matches("./");
    let normalized = normalize_module_key(without_extension);
    let mut keys = BTreeSet::new();
    keys.insert(normalized.clone());
    if let Some(last) = normalized.rsplit('.').next() {
        keys.insert(last.to_owned());
    }
    keys
}

fn extend_with_module_prefix(
    keys: &mut BTreeSet<String>,
    prefix: &str,
    relative_path: &str,
    include_root_package_key: bool,
) {
    if let Some(package_key) = package_key_for_path(relative_path) {
        if package_key.is_empty() {
            if include_root_package_key {
                keys.insert(prefix.to_owned());
            }
        } else {
            keys.insert(format!("{prefix}.{package_key}"));
        }
    }
    for key in module_keys_for_path(relative_path) {
        keys.insert(format!("{prefix}.{key}"));
    }
}

fn package_key_for_path(path: &str) -> Option<String> {
    let path = path::clean(path);
    if path == "go.mod" || path.ends_with("/go.mod") {
        return None;
    }
    if path == "package.json" || path.ends_with("/package.json") {
        return None;
    }
    let Some((directory, _)) = path.rsplit_once('/') else {
        return Some(String::new());
    };
    Some(normalize_module_key(directory))
}

fn path_relative_to_module(path: &str, source_path_prefix: &str) -> Option<String> {
    let path = path::clean(path);
    if source_path_prefix.is_empty() {
        return Some(path);
    }
    let stripped = path.strip_prefix(source_path_prefix)?;
    let relative = stripped.strip_prefix('/')?;
    (!relative.is_empty()).then_some(relative.to_owned())
}

#[cfg(test)]
#[path = "module_key_tests.rs"]
mod tests;

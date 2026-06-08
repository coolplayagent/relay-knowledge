use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, params};
use serde::Deserialize;
use serde_json::Value;

use crate::{domain::CodeRepositorySetMemberStatus, storage::StorageError};

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) struct ModulePrefix {
    source_path_prefix: String,
    module_key: String,
    path_aliases: BTreeMap<String, BTreeSet<String>>,
    path_alias_patterns: Vec<PathAliasPattern>,
    exposes_package_paths: bool,
    exposes_root_package_key: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct PathAliasPattern {
    path_prefix: String,
    path_suffix: String,
    alias_prefix: String,
    alias_suffix: String,
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

pub(super) fn manifest_module_prefixes_for_members(
    connection: &mut Connection,
    members: &[CodeRepositorySetMemberStatus],
) -> Result<BTreeMap<String, Vec<ModulePrefix>>, StorageError> {
    let mut prefixes_by_scope = BTreeMap::new();
    for member in members {
        let mut statement = connection.prepare(
            "
            SELECT path, content
            FROM code_repository_chunks
            WHERE source_scope = ?1
              AND (
                  path = 'go.mod' OR path LIKE '%/go.mod'
                  OR path = 'go.work' OR path LIKE '%/go.work'
                  OR path = 'pnpm-workspace.yaml' OR path LIKE '%/pnpm-workspace.yaml'
                  OR path = 'pnpm-workspace.yml' OR path LIKE '%/pnpm-workspace.yml'
                  OR path = 'package.json' OR path LIKE '%/package.json'
              )
            ORDER BY path ASC, chunk_id ASC
            ",
        )?;
        let rows = statement.query_map(params![member.member.source_scope], manifest_chunk)?;
        let mut prefixes = Vec::new();
        let chunks = rows.collect::<Result<Vec<_>, _>>()?;
        let go_workspaces = go_workspaces(&chunks);
        let pnpm_workspaces = pnpm_workspaces(&chunks);
        for chunk in &chunks {
            if is_go_mod_path(&chunk.path) && go_module_allowed(&chunk.path, &go_workspaces) {
                collect_go_module_prefixes(&chunk.path, &chunk.content, &mut prefixes);
            } else if is_package_json_path(&chunk.path) {
                collect_package_prefixes(
                    &chunk.path,
                    &chunk.content,
                    &pnpm_workspaces,
                    &mut prefixes,
                );
            }
        }
        if !prefixes.is_empty() {
            prefixes_by_scope.insert(member.member.source_scope.clone(), prefixes);
        }
    }

    Ok(prefixes_by_scope)
}

pub(super) fn module_keys_for_path_with_prefixes(
    path: &str,
    prefixes: &[ModulePrefix],
) -> BTreeSet<String> {
    module_keys_for_path_with_prefixes_inner(path, prefixes, true)
}

pub(super) fn module_keys_for_symbol_path_with_prefixes(
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
        let relative_path = clean_manifest_path(&relative_path);
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

pub(super) fn normalize_module_key(value: &str) -> String {
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

#[derive(Debug, Clone)]
struct ManifestChunk {
    path: String,
    content: String,
}

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
struct GoWorkspace {
    root_path_prefix: String,
    module_dirs: BTreeSet<String>,
}

#[derive(Debug, Clone)]
struct PnpmWorkspace {
    root_path_prefix: String,
    includes: Vec<String>,
    excludes: Vec<String>,
}

fn manifest_chunk(row: &rusqlite::Row<'_>) -> rusqlite::Result<ManifestChunk> {
    Ok(ManifestChunk {
        path: row.get(0)?,
        content: row.get(1)?,
    })
}

fn go_workspaces(chunks: &[ManifestChunk]) -> Vec<GoWorkspace> {
    chunks
        .iter()
        .filter(|chunk| is_go_work_path(&chunk.path))
        .filter_map(|chunk| go_workspace(&chunk.path, &chunk.content))
        .collect()
}

fn go_workspace(path: &str, content: &str) -> Option<GoWorkspace> {
    let root_path_prefix = manifest_parent_path(path);
    let mut module_dirs = BTreeSet::new();
    collect_go_work_dirs(&root_path_prefix, content, &mut module_dirs);
    (!module_dirs.is_empty()).then_some(GoWorkspace {
        root_path_prefix,
        module_dirs,
    })
}

fn go_module_allowed(path: &str, workspaces: &[GoWorkspace]) -> bool {
    if workspaces.is_empty() {
        return true;
    }
    let module_dir = go_manifest_path_prefix(path);
    let mut governed_by_workspace = false;
    for workspace in workspaces {
        if path_is_at_or_below_root(&module_dir, &workspace.root_path_prefix) {
            governed_by_workspace = true;
            if workspace.module_dirs.contains(&module_dir) {
                return true;
            }
        }
    }

    !governed_by_workspace
}

fn collect_go_module_prefixes(path: &str, content: &str, prefixes: &mut Vec<ModulePrefix>) {
    let source_path_prefix = go_manifest_path_prefix(path);
    for line in content.lines() {
        let Some(module_key) = go_module_prefix(line) else {
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

fn collect_go_work_dirs(root: &str, content: &str, dirs: &mut BTreeSet<String>) {
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
            if let Some(joined) = workspace_path_join(root, go_work_path_token(line)) {
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
        if let Some(joined) = workspace_path_join(root, go_work_path_token(rest)) {
            dirs.insert(joined);
        }
    }
}

fn go_work_path_token(value: &str) -> &str {
    value
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim_matches(['"', '\'', '`'])
}

fn pnpm_workspaces(chunks: &[ManifestChunk]) -> Vec<PnpmWorkspace> {
    chunks
        .iter()
        .filter(|chunk| is_pnpm_workspace_path(&chunk.path))
        .filter_map(|chunk| pnpm_workspace(&chunk.path, &chunk.content))
        .collect()
}

fn pnpm_workspace(path: &str, content: &str) -> Option<PnpmWorkspace> {
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
                excludes.push(clean_manifest_path(excluded));
            } else {
                includes.push(clean_manifest_path(&package));
            }
        }
    }
    Some(PnpmWorkspace {
        root_path_prefix: manifest_parent_path(path),
        includes,
        excludes,
    })
}

fn collect_package_prefixes(
    path: &str,
    content: &str,
    workspaces: &[PnpmWorkspace],
    prefixes: &mut Vec<ModulePrefix>,
) {
    let source_path_prefix = manifest_parent_path(path);
    if package_path_is_ignored(&source_path_prefix)
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
    let path = clean_manifest_path(value.trim().trim_matches(['"', '\'']));
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
    let path = clean_manifest_path(value.trim().trim_matches(['"', '\'']));
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

fn go_module_prefix(line: &str) -> Option<String> {
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

fn go_manifest_path_prefix(path: &str) -> String {
    let path = clean_manifest_path(path);
    path.strip_suffix("/go.mod")
        .filter(|prefix| !prefix.is_empty())
        .unwrap_or_default()
        .to_owned()
}

fn package_allowed_by_workspace(package_path_prefix: &str, workspaces: &[PnpmWorkspace]) -> bool {
    if workspaces.is_empty() {
        return true;
    }
    let mut governed_by_workspace = false;
    for workspace in workspaces {
        let Some(relative_path) =
            workspace_relative_path(package_path_prefix, &workspace.root_path_prefix)
        else {
            continue;
        };
        governed_by_workspace = true;
        if workspace.includes.iter().any(|pattern| {
            workspace_pattern_matches(pattern, &relative_path)
                && !workspace
                    .excludes
                    .iter()
                    .any(|pattern| workspace_pattern_matches(pattern, &relative_path))
        }) {
            return true;
        }
    }

    !governed_by_workspace
}

fn workspace_relative_path(path: &str, root_path_prefix: &str) -> Option<String> {
    let path = clean_manifest_path(path);
    let root_path_prefix = clean_manifest_path(root_path_prefix);
    if root_path_prefix.is_empty() {
        return Some(path);
    }
    if path == root_path_prefix {
        return Some(String::new());
    }
    let stripped = path.strip_prefix(&root_path_prefix)?.strip_prefix('/')?;
    Some(stripped.to_owned())
}

fn path_is_at_or_below_root(path: &str, root_path_prefix: &str) -> bool {
    let path = clean_manifest_path(path);
    let root_path_prefix = clean_manifest_path(root_path_prefix);
    root_path_prefix.is_empty()
        || path == root_path_prefix
        || path
            .strip_prefix(&root_path_prefix)
            .is_some_and(|relative| relative.starts_with('/'))
}

fn workspace_pattern_matches(pattern: &str, path: &str) -> bool {
    let pattern = clean_manifest_path(pattern);
    let path = clean_manifest_path(path);
    if pattern == "." {
        return path.is_empty();
    }
    let pattern_segments = path_segments(&pattern);
    let path_segments = path_segments(&path);
    glob_segments_match(&pattern_segments, &path_segments)
}

fn glob_segments_match(pattern: &[&str], path: &[&str]) -> bool {
    match (pattern, path) {
        ([], []) => true,
        ([], _) => false,
        ([head, tail @ ..], _) if *head == "**" => {
            glob_segments_match(tail, path)
                || (!path.is_empty() && glob_segments_match(pattern, &path[1..]))
        }
        ([head, tail @ ..], [path_head, path_tail @ ..]) => {
            wildcard_segment_matches(head, path_head) && glob_segments_match(tail, path_tail)
        }
        _ => false,
    }
}

fn wildcard_segment_matches(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return !value.is_empty();
    }
    if !pattern.contains('*') {
        return pattern == value;
    }
    let mut remainder = value;
    let mut parts = pattern.split('*').peekable();
    if let Some(first) = parts.next().filter(|part| !part.is_empty()) {
        let Some(stripped) = remainder.strip_prefix(first) else {
            return false;
        };
        remainder = stripped;
    }
    while let Some(part) = parts.next() {
        if part.is_empty() {
            continue;
        }
        if parts.peek().is_none() {
            return remainder.ends_with(part);
        }
        let Some(position) = remainder.find(part) else {
            return false;
        };
        remainder = &remainder[position + part.len()..];
    }

    true
}

fn path_segments(path: &str) -> Vec<&str> {
    path.split('/')
        .filter(|segment| !segment.is_empty())
        .collect()
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
    let path = clean_manifest_path(path);
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
    let path = clean_manifest_path(path);
    if source_path_prefix.is_empty() {
        return Some(path);
    }
    let stripped = path.strip_prefix(source_path_prefix)?;
    let relative = stripped.strip_prefix('/')?;
    (!relative.is_empty()).then_some(relative.to_owned())
}

fn workspace_path_join(root: &str, child: &str) -> Option<String> {
    let child = clean_manifest_path(child);
    if child.is_empty() || child.starts_with('/') || child.split('/').any(|part| part == "..") {
        return None;
    }
    let root = clean_manifest_path(root);
    if root.is_empty() || child == "." {
        return Some(if child == "." { root } else { child });
    }

    Some(format!("{root}/{child}"))
}

fn manifest_parent_path(path: &str) -> String {
    let path = clean_manifest_path(path);
    path.rsplit_once('/')
        .map(|(parent, _)| parent.to_owned())
        .unwrap_or_default()
}

fn package_path_is_ignored(path: &str) -> bool {
    path.split('/')
        .any(|segment| matches!(segment, "node_modules" | ".pnpm"))
}

fn is_go_mod_path(path: &str) -> bool {
    clean_manifest_path(path)
        .rsplit('/')
        .next()
        .is_some_and(|name| name == "go.mod")
}

fn is_go_work_path(path: &str) -> bool {
    clean_manifest_path(path)
        .rsplit('/')
        .next()
        .is_some_and(|name| name == "go.work")
}

fn is_pnpm_workspace_path(path: &str) -> bool {
    clean_manifest_path(path)
        .rsplit('/')
        .next()
        .is_some_and(|name| matches!(name, "pnpm-workspace.yaml" | "pnpm-workspace.yml"))
}

fn is_package_json_path(path: &str) -> bool {
    clean_manifest_path(path)
        .rsplit('/')
        .next()
        .is_some_and(|name| name == "package.json")
}

fn clean_manifest_path(path: &str) -> String {
    path.replace('\\', "/").trim_start_matches("./").to_owned()
}

#[cfg(test)]
#[path = "manifest_tests.rs"]
mod tests;

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, params};

use crate::{domain::CodeRepositorySetMemberStatus, storage::StorageError};

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) struct ModulePrefix {
    source_path_prefix: String,
    module_key: String,
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
              AND (path = 'go.mod' OR path LIKE '%/go.mod')
            ORDER BY path ASC, chunk_id ASC
            ",
        )?;
        let rows = statement.query_map(params![member.member.source_scope], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut prefixes = Vec::new();
        for row in rows {
            let (path, content) = row?;
            collect_go_module_prefixes(&path, &content, &mut prefixes);
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
    let mut keys = module_keys_for_path(path);
    for prefix in prefixes {
        let Some(relative_path) = path_relative_to_module(path, &prefix.source_path_prefix) else {
            continue;
        };
        extend_with_module_prefix(&mut keys, &prefix.module_key, &relative_path);
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

fn collect_go_module_prefixes(path: &str, content: &str, prefixes: &mut Vec<ModulePrefix>) {
    let source_path_prefix = go_manifest_path_prefix(path);
    for line in content.lines() {
        let Some(module_key) = go_module_prefix(line) else {
            continue;
        };
        let prefix = ModulePrefix {
            source_path_prefix: source_path_prefix.clone(),
            module_key,
        };
        if !prefixes.contains(&prefix) {
            prefixes.push(prefix);
        }
    }
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

fn extend_with_module_prefix(keys: &mut BTreeSet<String>, prefix: &str, relative_path: &str) {
    if let Some(package_key) = package_key_for_path(relative_path) {
        if package_key.is_empty() {
            keys.insert(prefix.to_owned());
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

fn clean_manifest_path(path: &str) -> String {
    path.replace('\\', "/").trim_start_matches("./").to_owned()
}

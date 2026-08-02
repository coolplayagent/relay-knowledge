//! Indexes and normalizes bounded module-path candidates for import resolution.

use std::collections::BTreeMap;

use super::ImportResolution;
use crate::code::source_roots::{
    c_family_module_candidates, go_module_candidates, source_module_candidates,
};

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;

pub(super) fn index(files: &BTreeMap<String, String>) -> BTreeMap<String, Vec<String>> {
    let mut module_paths = BTreeMap::<String, Vec<String>>::new();
    for (path, language_id) in files {
        let candidates = match language_id.as_str() {
            "c" | "cpp" => c_family_module_candidates(path),
            "go" => go_module_candidates(path),
            _ => source_module_candidates(path),
        };
        for module_path in candidates {
            module_paths
                .entry(module_path)
                .or_default()
                .push(path.clone());
        }
    }

    module_paths
}

pub(super) fn resolve_first_file(
    candidates: &[String],
    allow_source_root_match: bool,
    module_paths: &BTreeMap<String, Vec<String>>,
) -> ImportResolution {
    for candidate in candidates {
        match resolve_file(candidate, allow_source_root_match, module_paths) {
            ImportResolution::Resolved(path) => return ImportResolution::Resolved(path),
            ImportResolution::Ambiguous => return ImportResolution::Ambiguous,
            ImportResolution::Unresolved => {}
        }
    }

    ImportResolution::Unresolved
}

pub(super) fn resolve_file(
    module_path: &str,
    allow_source_root_match: bool,
    module_paths: &BTreeMap<String, Vec<String>>,
) -> ImportResolution {
    let key = normalize(module_path);
    let Some(files) = module_paths.get(&key) else {
        return ImportResolution::Unresolved;
    };
    let exact = files
        .iter()
        .filter(|path| path.as_str() == module_path)
        .take(2)
        .collect::<Vec<_>>();
    if exact.len() == 1 {
        return ImportResolution::Resolved(exact[0].to_string());
    }
    if !allow_source_root_match {
        if files.len() == 1 && key == module_path {
            return ImportResolution::Resolved(files[0].clone());
        }
        return ImportResolution::Unresolved;
    }
    let source_root = files
        .iter()
        .filter(|path| {
            source_module_candidates(path)
                .iter()
                .any(|candidate| candidate == &key)
        })
        .take(2)
        .collect::<Vec<_>>();
    if source_root.len() == 1 {
        return ImportResolution::Resolved(source_root[0].to_string());
    }
    if files.len() == 1 {
        return ImportResolution::Resolved(files[0].clone());
    }

    ImportResolution::Ambiguous
}

pub(super) fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

pub(super) fn normalize_join(parent: &str, child: &str) -> Option<String> {
    let mut parts = Vec::<&str>::new();
    if child.starts_with('/') {
        return None;
    }
    for part in parent
        .split('/')
        .chain(child.split('/'))
        .filter(|part| !part.is_empty() && *part != ".")
    {
        if part == ".." {
            parts.pop()?;
        } else {
            parts.push(part);
        }
    }
    if parts.is_empty() {
        return None;
    }

    Some(parts.join("/"))
}

pub(super) fn normalize(path: &str) -> String {
    let mut normalized = path.trim();
    while let Some(stripped) = normalized.strip_prefix("./") {
        normalized = stripped;
    }
    normalized.trim_end_matches('/').to_owned()
}

pub(super) fn push_unique(candidates: &mut Vec<String>, candidate: String) {
    if !candidates.contains(&candidate) {
        candidates.push(candidate);
    }
}

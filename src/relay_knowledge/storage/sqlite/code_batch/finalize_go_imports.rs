use std::collections::BTreeMap;

use super::ImportResolution;

pub(super) fn resolve_import(
    statement: &str,
    indexed_module_paths: &BTreeMap<String, Vec<String>>,
) -> ImportResolution {
    let Some(import_path) = go_import_path(statement) else {
        return ImportResolution::Unresolved;
    };

    resolve_directory_with_go_files(&import_path, indexed_module_paths)
}

fn go_import_path(statement: &str) -> Option<String> {
    let path = statement
        .trim()
        .trim_end_matches(';')
        .split_whitespace()
        .last()?;
    let path = path
        .trim_matches('"')
        .trim_matches('`')
        .trim_matches('\'')
        .trim();
    (!path.is_empty()).then(|| path.to_owned())
}

fn resolve_directory_with_go_files(
    directory_path: &str,
    indexed_module_paths: &BTreeMap<String, Vec<String>>,
) -> ImportResolution {
    let directory = normalize_module_path(directory_path);
    let prefix = if directory.is_empty() {
        String::new()
    } else {
        format!("{directory}/")
    };
    let mut matching_directories = Vec::new();
    for (module_path, paths) in indexed_module_paths
        .range(prefix.clone()..)
        .take_while(|(path, _)| prefix.is_empty() || path.starts_with(&prefix))
    {
        if parent_dir(module_path) != directory || !module_path.ends_with(".go") {
            continue;
        }
        for path in paths {
            let directory = parent_dir(path).to_owned();
            if !matching_directories.contains(&directory) {
                matching_directories.push(directory);
            }
            if matching_directories.len() > 1 {
                return ImportResolution::Ambiguous;
            }
        }
    }

    match matching_directories.as_slice() {
        [directory] => ImportResolution::Resolved(directory.clone()),
        [] => ImportResolution::Unresolved,
        _ => ImportResolution::Ambiguous,
    }
}

fn normalize_module_path(path: &str) -> &str {
    path.trim().trim_start_matches("./").trim_end_matches('/')
}

fn parent_dir(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

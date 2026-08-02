use std::collections::BTreeMap;

use crate::{
    code::source_roots::{
        normalized_module_candidates, source_module_candidates, source_relative_path,
    },
    domain::RepositoryCodeFileRecord,
};

use super::{ImportContext, ModuleFileResolution};

impl ImportContext<'_> {
    pub(in crate::code::identity) fn module_file_exists(&self, module_path: &str) -> bool {
        normalized_module_candidates(module_path)
            .iter()
            .any(|candidate| self.module_paths.contains_key(candidate))
    }

    pub(in crate::code::identity) fn any_module_file_exists(
        &self,
        module_paths: &[String],
    ) -> bool {
        module_paths
            .iter()
            .any(|module_path| self.module_file_exists(module_path))
    }

    pub(in crate::code::identity) fn resolve_first_module_file(
        &self,
        module_paths: &[String],
        allow_source_root_match: bool,
    ) -> ModuleFileResolution {
        for module_path in module_paths {
            match self.resolve_module_file(module_path, allow_source_root_match) {
                ModuleFileResolution::Resolved(path) => {
                    return ModuleFileResolution::Resolved(path);
                }
                ModuleFileResolution::Ambiguous => return ModuleFileResolution::Ambiguous,
                ModuleFileResolution::Unresolved => {}
            }
        }

        ModuleFileResolution::Unresolved
    }

    pub(in crate::code::identity) fn resolve_first_exact_module_file(
        &self,
        module_paths: &[String],
    ) -> ModuleFileResolution {
        for module_path in module_paths {
            if self.file_languages.contains_key(module_path.as_str()) {
                return ModuleFileResolution::Resolved(module_path.clone());
            }
        }

        ModuleFileResolution::Unresolved
    }

    pub(in crate::code::identity) fn directory_has_language_files(
        &self,
        directory_path: &str,
        language_id: &str,
    ) -> bool {
        normalized_module_candidates(directory_path)
            .iter()
            .any(|directory| {
                directory_has_language_files(&self.module_paths, directory, language_id)
            })
    }

    pub(in crate::code::identity) fn resolve_go_directory_with_language_files(
        &self,
        directory_path: &str,
    ) -> ModuleFileResolution {
        resolve_directory_from_modules(
            &self.go_module_paths,
            &normalized_module_candidates(directory_path),
            "go",
        )
    }

    pub(in crate::code::identity) fn resolve_directory_with_language_files(
        &self,
        directory_path: &str,
        language_id: &str,
    ) -> ModuleFileResolution {
        resolve_directory_from_modules(
            &self.module_paths,
            &normalized_module_candidates(directory_path),
            language_id,
        )
    }

    pub(in crate::code::identity) fn resolve_directory_tree_with_language_files(
        &self,
        directory_path: &str,
        language_id: &str,
    ) -> ModuleFileResolution {
        resolve_directory_tree_from_modules(
            &self.module_paths,
            &normalized_module_candidates(directory_path),
            language_id,
        )
    }
}

impl ImportContext<'_> {
    fn resolve_module_file(
        &self,
        module_path: &str,
        allow_source_root_match: bool,
    ) -> ModuleFileResolution {
        for normalized_path in normalized_module_candidates(module_path) {
            let Some(files) = self.module_paths.get(&normalized_path) else {
                continue;
            };
            if let Some(path) = unique_file_match(
                files
                    .iter()
                    .copied()
                    .filter(|file| file.path == module_path),
            ) {
                return ModuleFileResolution::Resolved(path);
            }
            if !allow_source_root_match {
                if files.len() == 1 && normalized_path == module_path {
                    return ModuleFileResolution::Resolved(files[0].path.clone());
                }
                continue;
            }
            if let Some(path) = unique_file_match(files.iter().copied().filter(|file| {
                source_module_candidates(&file.path)
                    .iter()
                    .any(|candidate| candidate == &normalized_path)
            })) {
                return ModuleFileResolution::Resolved(path);
            }
            if files.len() == 1 {
                return ModuleFileResolution::Resolved(files[0].path.clone());
            }

            return ModuleFileResolution::Ambiguous;
        }

        ModuleFileResolution::Unresolved
    }
}

fn unique_file_match<'a>(
    files: impl IntoIterator<Item = &'a RepositoryCodeFileRecord>,
) -> Option<String> {
    let mut matches = files.into_iter();
    let first = matches.next()?;
    matches.next().is_none().then(|| first.path.clone())
}

pub(in crate::code::identity) fn parse_quoted_specifier(statement: &str) -> Option<&str> {
    let start = statement.find(['"', '\''])?;
    let quote = statement.as_bytes()[start] as char;
    let rest = &statement[start + 1..];
    let end = rest.find(quote)?;

    Some(&rest[..end])
}

pub(in crate::code::identity) fn parent_dir(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

pub(in crate::code::identity) fn normalize_join(parent: &str, child: &str) -> Option<String> {
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

pub(in crate::code::identity) fn strip_source_root(path: &str) -> String {
    source_relative_path(path)
}

fn resolve_directory_from_modules(
    module_paths: &BTreeMap<String, Vec<&RepositoryCodeFileRecord>>,
    directories: &[String],
    language_id: &str,
) -> ModuleFileResolution {
    let mut resolved_directories = Vec::new();
    for directory in directories {
        match resolve_single_directory_from_modules(module_paths, directory, language_id) {
            ModuleFileResolution::Resolved(directory) => {
                if !resolved_directories.contains(&directory) {
                    resolved_directories.push(directory);
                }
                if resolved_directories.len() > 1 {
                    return ModuleFileResolution::Ambiguous;
                }
            }
            ModuleFileResolution::Ambiguous => return ModuleFileResolution::Ambiguous,
            ModuleFileResolution::Unresolved => {}
        }
    }

    match resolved_directories.as_slice() {
        [directory] => ModuleFileResolution::Resolved(directory.clone()),
        [] => ModuleFileResolution::Unresolved,
        _ => ModuleFileResolution::Ambiguous,
    }
}

fn resolve_single_directory_from_modules(
    module_paths: &BTreeMap<String, Vec<&RepositoryCodeFileRecord>>,
    directory: &str,
    language_id: &str,
) -> ModuleFileResolution {
    let prefix = if directory.is_empty() {
        String::new()
    } else {
        format!("{directory}/")
    };
    let mut matching_directories = Vec::new();
    for (module_path, files) in module_paths
        .range(prefix.clone()..)
        .take_while(|(path, _)| prefix.is_empty() || path.starts_with(&prefix))
    {
        if parent_dir(module_path) != directory {
            continue;
        }
        for file in files.iter().filter(|file| file.language_id == language_id) {
            let directory = parent_dir(&file.path).to_owned();
            if !matching_directories.contains(&directory) {
                matching_directories.push(directory);
            }
            if matching_directories.len() > 1 {
                return ModuleFileResolution::Ambiguous;
            }
        }
    }

    match matching_directories.as_slice() {
        [directory] => ModuleFileResolution::Resolved(directory.clone()),
        [] => ModuleFileResolution::Unresolved,
        _ => ModuleFileResolution::Ambiguous,
    }
}

fn directory_has_language_files(
    module_paths: &BTreeMap<String, Vec<&RepositoryCodeFileRecord>>,
    directory: &str,
    language_id: &str,
) -> bool {
    let prefix = if directory.is_empty() {
        String::new()
    } else {
        format!("{directory}/")
    };
    module_paths
        .range(prefix.clone()..)
        .take_while(|(path, _)| prefix.is_empty() || path.starts_with(&prefix))
        .any(|(_, files)| files.iter().any(|file| file.language_id == language_id))
}

fn resolve_directory_tree_from_modules(
    module_paths: &BTreeMap<String, Vec<&RepositoryCodeFileRecord>>,
    directories: &[String],
    language_id: &str,
) -> ModuleFileResolution {
    let mut matching_roots = Vec::new();
    for directory in directories {
        let prefix = if directory.is_empty() {
            String::new()
        } else {
            format!("{directory}/")
        };
        for (module_path, files) in module_paths
            .range(prefix.clone()..)
            .take_while(|(path, _)| prefix.is_empty() || path.starts_with(&prefix))
        {
            for file in files.iter().filter(|file| file.language_id == language_id) {
                let Some(root) = physical_directory_tree_root(&file.path, module_path, directory)
                else {
                    continue;
                };
                if !matching_roots.contains(&root) {
                    matching_roots.push(root);
                }
                if matching_roots.len() > 1 {
                    return ModuleFileResolution::Ambiguous;
                }
            }
        }
    }

    match matching_roots.as_slice() {
        [root] => ModuleFileResolution::Resolved(root.clone()),
        [] => ModuleFileResolution::Unresolved,
        _ => ModuleFileResolution::Ambiguous,
    }
}

fn physical_directory_tree_root(
    file_path: &str,
    module_path: &str,
    directory: &str,
) -> Option<String> {
    let suffix = module_path.strip_prefix(directory)?.trim_start_matches('/');
    if suffix.is_empty() {
        return Some(file_path.to_owned());
    }
    let root_len = file_path.len().checked_sub(suffix.len() + 1)?;
    Some(file_path[..root_len].to_owned())
}

#[cfg(test)]
#[path = "module_paths_tests.rs"]
mod tests;

use std::collections::BTreeMap;

use crate::domain::{CodeImportRecord, RepositoryCodeFileRecord, RepositoryCodeSymbolRecord};

use super::super::source_roots::{
    c_family_module_candidates, go_module_candidates, normalized_module_candidates,
    source_module_candidates, source_relative_path,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ImportResolution {
    Resolved,
    Ambiguous,
    Unresolved,
}

pub(super) enum ModuleFileResolution {
    Resolved(String),
    Ambiguous,
    Unresolved,
}

pub(super) struct ImportContext<'a> {
    file_languages: BTreeMap<&'a str, &'a str>,
    module_paths: BTreeMap<String, Vec<&'a RepositoryCodeFileRecord>>,
    go_module_paths: BTreeMap<String, Vec<&'a RepositoryCodeFileRecord>>,
    symbols_by_name: BTreeMap<&'a str, Vec<&'a RepositoryCodeSymbolRecord>>,
}

impl<'a> ImportContext<'a> {
    pub(super) fn new(
        files: &'a [RepositoryCodeFileRecord],
        symbols: &'a [RepositoryCodeSymbolRecord],
    ) -> Self {
        let mut file_languages = BTreeMap::new();
        let mut module_paths = BTreeMap::<String, Vec<&RepositoryCodeFileRecord>>::new();
        let mut go_module_paths = BTreeMap::<String, Vec<&RepositoryCodeFileRecord>>::new();
        for file in files {
            file_languages.insert(file.path.as_str(), file.language_id.as_str());
            let candidates = if matches!(file.language_id.as_str(), "c" | "cpp") {
                c_family_module_candidates(&file.path)
            } else {
                source_module_candidates(&file.path)
            };
            for module_path in candidates {
                module_paths.entry(module_path).or_default().push(file);
            }
            if file.language_id == "go" {
                for module_path in go_module_candidates(&file.path) {
                    go_module_paths.entry(module_path).or_default().push(file);
                }
            }
        }

        let mut symbols_by_name = BTreeMap::<&str, Vec<&RepositoryCodeSymbolRecord>>::new();
        for symbol in symbols {
            symbols_by_name
                .entry(symbol.name.as_str())
                .or_default()
                .push(symbol);
        }

        Self {
            file_languages,
            module_paths,
            go_module_paths,
            symbols_by_name,
        }
    }

    pub(super) fn language_for_path(&self, path: &str) -> Option<&'a str> {
        self.file_languages.get(path).copied()
    }

    pub(super) fn module_file_exists(&self, module_path: &str) -> bool {
        normalized_module_candidates(module_path)
            .iter()
            .any(|candidate| self.module_paths.contains_key(candidate))
    }

    pub(super) fn any_module_file_exists(&self, module_paths: &[String]) -> bool {
        module_paths
            .iter()
            .any(|module_path| self.module_file_exists(module_path))
    }

    pub(super) fn resolve_first_module_file(
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

    pub(super) fn directory_has_language_files(
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

    pub(super) fn resolve_go_directory_with_language_files(
        &self,
        directory_path: &str,
    ) -> ModuleFileResolution {
        resolve_directory_from_modules(
            &self.go_module_paths,
            &normalized_module_candidates(directory_path),
            "go",
        )
    }

    pub(super) fn resolve_directory_with_language_files(
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

    pub(super) fn resolve_directory_tree_with_language_files(
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

    pub(super) fn resolve_name_in_paths(
        &self,
        name: &str,
        module_paths: &[String],
    ) -> ImportResolution {
        let Some(candidates) = self.symbols_by_name.get(name) else {
            return ImportResolution::Unresolved;
        };
        let module_paths = module_paths
            .iter()
            .flat_map(|module_path| normalized_module_candidates(module_path))
            .collect::<Vec<_>>();
        let match_count = candidates
            .iter()
            .filter(|symbol| {
                module_paths
                    .iter()
                    .any(|module_path| path_matches_candidate(&symbol.path, module_path))
            })
            .take(2)
            .count();

        resolution_from_count(match_count)
    }

    pub(super) fn resolve_name_in_paths_for_language_and_kinds(
        &self,
        name: &str,
        module_paths: &[String],
        language_id: &str,
        allowed_kinds: &[&str],
    ) -> ImportResolution {
        let Some(candidates) = self.symbols_by_name.get(name) else {
            return ImportResolution::Unresolved;
        };
        let module_paths = module_paths
            .iter()
            .flat_map(|module_path| normalized_module_candidates(module_path))
            .collect::<Vec<_>>();
        let match_count = candidates
            .iter()
            .filter(|symbol| {
                symbol.language_id == language_id
                    && allowed_kinds.contains(&symbol.kind.as_str())
                    && module_paths
                        .iter()
                        .any(|module_path| path_matches_candidate(&symbol.path, module_path))
            })
            .take(2)
            .count();

        resolution_from_count(match_count)
    }

    pub(super) fn resolve_name_in_directory_tree(
        &self,
        name: &str,
        directory_path: &str,
        language_id: &str,
    ) -> ImportResolution {
        let Some(candidates) = self.symbols_by_name.get(name) else {
            return ImportResolution::Unresolved;
        };
        let directory_paths = normalized_module_candidates(directory_path);
        let match_count = candidates
            .iter()
            .filter(|symbol| {
                symbol.language_id == language_id
                    && directory_paths.iter().any(|directory| {
                        source_module_candidates(&symbol.path).iter().any(|path| {
                            path == directory || path.starts_with(&format!("{directory}/"))
                        })
                    })
            })
            .take(2)
            .count();

        resolution_from_count(match_count)
    }

    pub(super) fn resolve_name_in_directory(
        &self,
        name: &str,
        directory_path: &str,
        language_id: &str,
    ) -> ImportResolution {
        let Some(candidates) = self.symbols_by_name.get(name) else {
            return ImportResolution::Unresolved;
        };
        let directory_paths = normalized_module_candidates(directory_path);
        let match_count = candidates
            .iter()
            .filter(|symbol| {
                symbol.language_id == language_id
                    && directory_paths.iter().any(|directory| {
                        source_module_candidates(&symbol.path)
                            .iter()
                            .any(|path| parent_dir(path) == directory)
                    })
            })
            .take(2)
            .count();

        resolution_from_count(match_count)
    }

    pub(super) fn resolve_name_in_directory_for_language_and_kinds(
        &self,
        name: &str,
        directory_path: &str,
        language_id: &str,
        allowed_kinds: &[&str],
    ) -> ImportResolution {
        let Some(candidates) = self.symbols_by_name.get(name) else {
            return ImportResolution::Unresolved;
        };
        let directory_paths = normalized_module_candidates(directory_path);
        let match_count = candidates
            .iter()
            .filter(|symbol| {
                symbol.language_id == language_id
                    && allowed_kinds.contains(&symbol.kind.as_str())
                    && directory_paths.iter().any(|directory| {
                        source_module_candidates(&symbol.path)
                            .iter()
                            .any(|path| parent_dir(path) == directory)
                    })
            })
            .take(2)
            .count();

        resolution_from_count(match_count)
    }

    pub(super) fn resolve_name_in_directory_tree_for_language_and_kinds(
        &self,
        name: &str,
        directory_path: &str,
        language_id: &str,
        allowed_kinds: &[&str],
    ) -> ImportResolution {
        let Some(candidates) = self.symbols_by_name.get(name) else {
            return ImportResolution::Unresolved;
        };
        let directory_paths = normalized_module_candidates(directory_path);
        let match_count = candidates
            .iter()
            .filter(|symbol| {
                symbol.language_id == language_id
                    && allowed_kinds.contains(&symbol.kind.as_str())
                    && directory_paths.iter().any(|directory| {
                        source_module_candidates(&symbol.path).iter().any(|path| {
                            path == directory || path.starts_with(&format!("{directory}/"))
                        })
                    })
            })
            .take(2)
            .count();

        resolution_from_count(match_count)
    }

    pub(super) fn resolve_name(&self, name: &str) -> ImportResolution {
        let count = self
            .symbols_by_name
            .get(name)
            .map_or(0, |candidates| candidates.iter().take(2).count());

        resolution_from_count(count)
    }

    pub(super) fn resolve_name_in_namespace(
        &self,
        namespace: &str,
        name: &str,
    ) -> ImportResolution {
        let Some(candidates) = self.symbols_by_name.get(name) else {
            return ImportResolution::Unresolved;
        };
        let namespace = namespace.replace("::", ".");
        let suffix = format!(".{namespace}.{name}");
        let match_count = candidates
            .iter()
            .filter(|symbol| normalize_qualified_name(&symbol.qualified_name).ends_with(&suffix))
            .take(2)
            .count();

        resolution_from_count(match_count)
    }

    pub(super) fn resolve_name_in_namespace_for_language(
        &self,
        namespace: &str,
        name: &str,
        language_id: &str,
    ) -> ImportResolution {
        let Some(candidates) = self.symbols_by_name.get(name) else {
            return ImportResolution::Unresolved;
        };
        let namespace = namespace.replace("::", ".");
        let suffix = format!(".{namespace}.{name}");
        let match_count = candidates
            .iter()
            .filter(|symbol| {
                symbol.language_id == language_id
                    && normalize_qualified_name(&symbol.qualified_name).ends_with(&suffix)
            })
            .take(2)
            .count();

        resolution_from_count(match_count)
    }

    pub(super) fn resolve_name_in_namespace_for_language_and_kinds(
        &self,
        namespace: &str,
        name: &str,
        language_id: &str,
        allowed_kinds: &[&str],
    ) -> ImportResolution {
        let Some(candidates) = self.symbols_by_name.get(name) else {
            return ImportResolution::Unresolved;
        };
        let namespace = namespace.replace("::", ".");
        let suffix = format!(".{namespace}.{name}");
        let match_count = candidates
            .iter()
            .filter(|symbol| {
                symbol.language_id == language_id
                    && allowed_kinds.contains(&symbol.kind.as_str())
                    && normalize_qualified_name(&symbol.qualified_name).ends_with(&suffix)
            })
            .take(2)
            .count();

        resolution_from_count(match_count)
    }

    pub(super) fn namespace_exists(&self, namespace: &str) -> bool {
        let last_segment = namespace
            .rsplit("::")
            .next()
            .filter(|segment| !segment.is_empty())
            .unwrap_or(namespace);
        if self
            .symbols_by_name
            .get(last_segment)
            .is_some_and(|symbols| symbols.iter().any(|symbol| symbol.kind == "module"))
        {
            return true;
        }

        let namespace = namespace.replace("::", ".");
        let marker = format!(".{namespace}.");
        self.symbols_by_name.values().flatten().any(|symbol| {
            normalize_qualified_name(&symbol.qualified_name).contains(marker.as_str())
        })
    }

    pub(super) fn namespace_exists_for_language(&self, namespace: &str, language_id: &str) -> bool {
        let normalized_namespace = namespace.replace("::", ".");
        let suffix = format!(".{normalized_namespace}");
        self.symbols_by_name.values().flatten().any(|symbol| {
            if symbol.language_id != language_id {
                return false;
            }
            let qualified_name = normalize_qualified_name(&symbol.qualified_name);
            symbol.kind == "module"
                && (qualified_name == normalized_namespace
                    || qualified_name.ends_with(suffix.as_str()))
        })
    }

    pub(super) fn package_declaration_conflicts_for_language(
        &self,
        package_path: &str,
        language_id: &str,
    ) -> bool {
        let expected_package = package_path.replace('/', ".");
        let expected_leaf = expected_package
            .rsplit('.')
            .next()
            .filter(|segment| !segment.is_empty())
            .unwrap_or(expected_package.as_str());
        self.symbols_by_name.values().flatten().any(|symbol| {
            if symbol.language_id != language_id
                || symbol.kind != "module"
                || !symbol.signature.trim_start().starts_with("package ")
                || !package_declaration_matches(symbol, package_path)
            {
                return false;
            }
            symbol.name != expected_package && symbol.name != expected_leaf
        })
    }
}

fn package_declaration_matches(symbol: &RepositoryCodeSymbolRecord, package_path: &str) -> bool {
    source_module_candidates(&symbol.path)
        .iter()
        .any(|path| parent_dir(path) == package_path)
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

pub(super) fn apply_resolution(import: &mut CodeImportRecord, resolution: ImportResolution) {
    match resolution {
        ImportResolution::Resolved => {
            import.resolution_state = "resolved".to_owned();
            import.confidence_basis_points = 8_000;
            import.confidence_tier = "inferred".to_owned();
        }
        ImportResolution::Ambiguous => {
            import.resolution_state = "ambiguous".to_owned();
            import.confidence_basis_points = 5_000;
            import.confidence_tier = "ambiguous".to_owned();
        }
        ImportResolution::Unresolved => {
            import.resolution_state = "unresolved".to_owned();
            import.confidence_basis_points = 2_500;
            import.confidence_tier = "ambiguous".to_owned();
        }
    }
}

pub(super) fn combined_resolution(
    results: impl IntoIterator<Item = ImportResolution>,
) -> ImportResolution {
    let mut total = 0usize;
    let mut resolved = 0usize;
    let mut ambiguous = false;
    for result in results {
        total += 1;
        match result {
            ImportResolution::Resolved => resolved += 1,
            ImportResolution::Ambiguous => ambiguous = true,
            ImportResolution::Unresolved => {}
        }
    }
    if total == 0 {
        return ImportResolution::Unresolved;
    }
    if ambiguous || (resolved > 0 && resolved < total) {
        return ImportResolution::Ambiguous;
    }
    if resolved == total {
        return ImportResolution::Resolved;
    }

    ImportResolution::Unresolved
}

pub(super) fn module_file_resolution(
    resolution: ModuleFileResolution,
) -> (ImportResolution, Option<String>) {
    match resolution {
        ModuleFileResolution::Resolved(target_hint) => {
            (ImportResolution::Resolved, Some(target_hint))
        }
        ModuleFileResolution::Ambiguous => (ImportResolution::Ambiguous, None),
        ModuleFileResolution::Unresolved => (ImportResolution::Unresolved, None),
    }
}

pub(super) fn parse_quoted_specifier(statement: &str) -> Option<&str> {
    let start = statement.find(['"', '\''])?;
    let quote = statement.as_bytes()[start] as char;
    let rest = &statement[start + 1..];
    let end = rest.find(quote)?;

    Some(&rest[..end])
}

pub(super) fn parent_dir(path: &str) -> &str {
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

pub(super) fn strip_source_root(path: &str) -> String {
    source_relative_path(path)
}

fn path_matches_candidate(path: &str, candidate: &str) -> bool {
    let candidates = source_module_candidates(path);
    path == candidate
        || candidates
            .iter()
            .any(|module_path| module_path == candidate)
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

fn normalize_qualified_name(value: &str) -> String {
    let mut normalized = value.replace("::", ".").replace(['/', '\\'], ".");
    for extension in [
        ".rs.", ".py.", ".js.", ".jsx.", ".ts.", ".tsx.", ".php.", ".phtml.", ".cs.", ".kt.",
        ".kts.", ".scala.", ".swift.",
    ] {
        normalized = normalized.replace(extension, ".");
    }

    normalized
}

fn resolution_from_count(count: usize) -> ImportResolution {
    match count {
        0 => ImportResolution::Unresolved,
        1 => ImportResolution::Resolved,
        _ => ImportResolution::Ambiguous,
    }
}

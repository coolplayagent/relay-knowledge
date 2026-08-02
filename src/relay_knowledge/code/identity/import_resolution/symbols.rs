use crate::{
    code::source_roots::{normalized_module_candidates, source_module_candidates},
    domain::RepositoryCodeSymbolRecord,
};

use super::{ImportContext, ImportResolution, parent_dir, resolution_from_count};

impl ImportContext<'_> {
    pub(in crate::code::identity) fn resolve_name_in_paths(
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

    pub(in crate::code::identity) fn resolve_name_in_paths_for_language_and_kinds(
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

    pub(in crate::code::identity) fn resolve_name_in_directory_tree(
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

    pub(in crate::code::identity) fn resolve_name_in_directory(
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

    pub(in crate::code::identity) fn resolve_name_in_directory_for_language_and_kinds(
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

    pub(in crate::code::identity) fn resolve_name_in_directory_tree_for_language_and_kinds(
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

    pub(in crate::code::identity) fn resolve_name_in_directory_tree_for_language_and_kinds_with_hint(
        &self,
        name: &str,
        directory_path: &str,
        language_id: &str,
        allowed_kinds: &[&str],
    ) -> (ImportResolution, Option<String>) {
        let Some(candidates) = self.symbols_by_name.get(name) else {
            return (ImportResolution::Unresolved, None);
        };
        let directory_paths = normalized_module_candidates(directory_path);
        let matches = candidates
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
            .collect::<Vec<_>>();

        match matches.as_slice() {
            [symbol] => (ImportResolution::Resolved, Some(symbol.path.clone())),
            [.., _] => (ImportResolution::Ambiguous, None),
            [] => (ImportResolution::Unresolved, None),
        }
    }

    pub(in crate::code::identity) fn resolve_name(&self, name: &str) -> ImportResolution {
        let count = self
            .symbols_by_name
            .get(name)
            .map_or(0, |candidates| candidates.iter().take(2).count());

        resolution_from_count(count)
    }

    pub(in crate::code::identity) fn resolve_name_in_namespace(
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

    pub(in crate::code::identity) fn resolve_name_in_namespace_for_language(
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

    pub(in crate::code::identity) fn resolve_name_in_namespace_for_language_and_kinds(
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

    pub(in crate::code::identity) fn namespace_exists(&self, namespace: &str) -> bool {
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

    pub(in crate::code::identity) fn namespace_exists_for_language(
        &self,
        namespace: &str,
        language_id: &str,
    ) -> bool {
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

    pub(in crate::code::identity) fn package_declaration_conflicts_for_language(
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

fn path_matches_candidate(path: &str, candidate: &str) -> bool {
    let candidates = source_module_candidates(path);
    path == candidate
        || candidates
            .iter()
            .any(|module_path| module_path == candidate)
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

#[cfg(test)]
#[path = "symbols_tests.rs"]
mod tests;

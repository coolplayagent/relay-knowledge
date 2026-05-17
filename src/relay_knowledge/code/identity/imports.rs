use std::collections::BTreeMap;

use crate::domain::{CodeImportRecord, RepositoryCodeFileRecord, RepositoryCodeSymbolRecord};

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
    symbols_by_name: BTreeMap<&'a str, Vec<&'a RepositoryCodeSymbolRecord>>,
}

impl<'a> ImportContext<'a> {
    pub(super) fn new(
        files: &'a [RepositoryCodeFileRecord],
        symbols: &'a [RepositoryCodeSymbolRecord],
    ) -> Self {
        let mut file_languages = BTreeMap::new();
        let mut module_paths = BTreeMap::<String, Vec<&RepositoryCodeFileRecord>>::new();
        for file in files {
            file_languages.insert(file.path.as_str(), file.language_id.as_str());
            module_paths
                .entry(strip_source_root(&file.path).to_owned())
                .or_default()
                .push(file);
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
            symbols_by_name,
        }
    }

    pub(super) fn language_for_path(&self, path: &str) -> Option<&'a str> {
        self.file_languages.get(path).copied()
    }

    pub(super) fn module_file_exists(&self, module_path: &str) -> bool {
        self.module_paths
            .contains_key(normalize_module_path(module_path))
    }

    pub(super) fn module_file_exists_with_language(
        &self,
        module_path: &str,
        language_id: &str,
    ) -> bool {
        self.module_paths
            .get(normalize_module_path(module_path))
            .is_some_and(|files| files.iter().any(|file| file.language_id == language_id))
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
        let directory = normalize_module_path(directory_path);
        let prefix = if directory.is_empty() {
            String::new()
        } else {
            format!("{directory}/")
        };
        self.module_paths
            .range(prefix.clone()..)
            .take_while(|(path, _)| prefix.is_empty() || path.starts_with(&prefix))
            .any(|(_, files)| files.iter().any(|file| file.language_id == language_id))
    }

    pub(super) fn resolve_name_in_paths(
        &self,
        name: &str,
        module_paths: &[String],
    ) -> ImportResolution {
        let Some(candidates) = self.symbols_by_name.get(name) else {
            return ImportResolution::Unresolved;
        };
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
}

impl ImportContext<'_> {
    fn resolve_module_file(
        &self,
        module_path: &str,
        allow_source_root_match: bool,
    ) -> ModuleFileResolution {
        let Some(files) = self.module_paths.get(normalize_module_path(module_path)) else {
            return ModuleFileResolution::Unresolved;
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
            return ModuleFileResolution::Unresolved;
        }
        if let Some(path) = unique_file_match(
            files
                .iter()
                .copied()
                .filter(|file| strip_source_root(&file.path) == module_path),
        ) {
            return ModuleFileResolution::Resolved(path);
        }
        if files.len() == 1 {
            return ModuleFileResolution::Resolved(files[0].path.clone());
        }

        ModuleFileResolution::Ambiguous
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

pub(super) fn strip_source_root(path: &str) -> &str {
    for prefix in [
        "src/main/java/",
        "src/test/java/",
        "src/main/kotlin/",
        "src/test/kotlin/",
        "src/main/scala/",
        "src/test/scala/",
        "src/main/groovy/",
        "src/test/groovy/",
        "src/",
    ] {
        if let Some(stripped) = path.strip_prefix(prefix) {
            return stripped;
        }
    }

    path
}

fn normalize_module_path(path: &str) -> &str {
    strip_source_root(path.trim_start_matches("./"))
}

fn path_matches_candidate(path: &str, candidate: &str) -> bool {
    let candidate = normalize_module_path(candidate);
    path == candidate || strip_source_root(path) == candidate
}

fn normalize_qualified_name(value: &str) -> String {
    value.replace("::", ".")
}

fn resolution_from_count(count: usize) -> ImportResolution {
    match count {
        0 => ImportResolution::Unresolved,
        1 => ImportResolution::Resolved,
        _ => ImportResolution::Ambiguous,
    }
}

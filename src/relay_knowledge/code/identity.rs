use std::collections::{BTreeMap, BTreeSet};

use crate::domain::{
    CodeImportRecord, RepositoryCodeFileRecord, RepositoryCodeReferenceRecord,
    RepositoryCodeSymbolRecord,
};

pub(super) fn enrich_symbol_identities(
    repository_id: &str,
    symbols: &mut [RepositoryCodeSymbolRecord],
) {
    let symbol_metadata = symbols
        .iter()
        .enumerate()
        .map(|(index, symbol)| SymbolIdentityMetadata {
            index,
            path: symbol.path.clone(),
            name: symbol.name.clone(),
            kind: symbol.kind.clone(),
            line_start: symbol.line_range.start,
            line_end: symbol.line_range.end,
            prefix: path_prefix(&symbol.qualified_name).to_owned(),
        })
        .collect::<Vec<_>>();
    let mut by_path = BTreeMap::<&str, Vec<usize>>::new();
    for (metadata_index, metadata) in symbol_metadata.iter().enumerate() {
        by_path
            .entry(metadata.path.as_str())
            .or_default()
            .push(metadata_index);
    }

    for metadata_indices in by_path.values_mut() {
        metadata_indices.sort_by(|left, right| {
            let left = &symbol_metadata[*left];
            let right = &symbol_metadata[*right];
            left.line_start
                .cmp(&right.line_start)
                .then_with(|| right.line_end.cmp(&left.line_end))
                .then_with(|| left.name.cmp(&right.name))
        });
        let mut container_stack = Vec::<usize>::new();
        for metadata_index in metadata_indices {
            let metadata = &symbol_metadata[*metadata_index];
            while container_stack
                .last()
                .is_some_and(|ancestor| symbol_metadata[*ancestor].line_end < metadata.line_end)
            {
                container_stack.pop();
            }
            let mut segments = container_stack
                .iter()
                .map(|ancestor| symbol_metadata[*ancestor].name.clone())
                .collect::<Vec<_>>();
            segments.push(metadata.name.clone());
            symbols[metadata.index].qualified_name =
                format!("{}::{}", metadata.prefix, segments.join("."));
            symbols[metadata.index].canonical_symbol_id = format!(
                "repo://{repository_id}/{}",
                symbols[metadata.index].qualified_name
            );
            if container_kind(&metadata.kind) {
                container_stack.push(*metadata_index);
            }
        }
    }
}

struct SymbolIdentityMetadata {
    index: usize,
    path: String,
    name: String,
    kind: String,
    line_start: u32,
    line_end: u32,
    prefix: String,
}

pub(super) fn resolve_reference_targets(
    symbols: &[RepositoryCodeSymbolRecord],
    references: &mut [RepositoryCodeReferenceRecord],
) {
    let mut by_name = BTreeMap::<&str, Vec<&RepositoryCodeSymbolRecord>>::new();
    for symbol in symbols {
        by_name.entry(&symbol.name).or_default().push(symbol);
    }
    for reference in references {
        reference.target_hint = Some(reference.name.clone());
        match resolve_reference_target(
            reference,
            by_name
                .get(reference.name.as_str())
                .map(std::vec::Vec::as_slice),
        ) {
            Resolution::Resolved(symbol) => {
                reference.target_symbol_snapshot_id = Some(symbol.symbol_snapshot_id.clone());
                reference.resolution_state = "resolved".to_owned();
                reference.confidence_basis_points = 8_000;
                reference.confidence_tier = "inferred".to_owned();
            }
            Resolution::Ambiguous => {
                reference.target_symbol_snapshot_id = None;
                reference.resolution_state = "ambiguous".to_owned();
                reference.confidence_basis_points = 5_000;
                reference.confidence_tier = "ambiguous".to_owned();
            }
            Resolution::Unresolved => {
                reference.target_symbol_snapshot_id = None;
                reference.resolution_state = "unresolved".to_owned();
                reference.confidence_basis_points = 2_500;
                reference.confidence_tier = "ambiguous".to_owned();
            }
        }
    }
}

pub(super) fn resolve_import_targets(
    files: &[RepositoryCodeFileRecord],
    symbols: &[RepositoryCodeSymbolRecord],
    imports: &mut [CodeImportRecord],
) {
    let local_modules = local_python_modules(files);
    let mut by_name = BTreeMap::<&str, Vec<&RepositoryCodeSymbolRecord>>::new();
    for symbol in symbols {
        by_name.entry(&symbol.name).or_default().push(symbol);
    }

    for import in imports {
        import.target_hint = Some(import.module.clone());
        let Some(request) = PythonImportRequest::parse(&import.path, &import.module) else {
            continue;
        };
        match resolve_python_import(&request, &local_modules, &by_name) {
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
}

enum Resolution<'a> {
    Resolved(&'a RepositoryCodeSymbolRecord),
    Ambiguous,
    Unresolved,
}

enum ImportResolution {
    Resolved,
    Ambiguous,
    Unresolved,
}

struct PythonImportRequest {
    module_paths: Vec<String>,
    imported_names: Vec<String>,
}

impl PythonImportRequest {
    fn parse(import_path: &str, statement: &str) -> Option<Self> {
        if !(import_path.ends_with(".py") || import_path.ends_with(".pyw")) {
            return None;
        }
        let statement = statement.trim().trim_end_matches(';').trim();
        if let Some(body) = statement.strip_prefix("from ") {
            let (module, names) = body.split_once(" import ")?;
            let module_paths = python_module_path_candidates(import_path, module.trim());
            let imported_names = parse_python_imported_names(names);
            if module_paths.is_empty() && imported_names.is_empty() {
                return None;
            }

            return Some(Self {
                module_paths,
                imported_names,
            });
        }
        if let Some(body) = statement.strip_prefix("import ") {
            let module_paths = body
                .split(',')
                .filter_map(|part| {
                    let module = part
                        .trim()
                        .split_once(" as ")
                        .map_or(part.trim(), |(module, _)| module.trim());
                    python_module_path_candidates(import_path, module)
                        .into_iter()
                        .next()
                })
                .collect::<Vec<_>>();
            if module_paths.is_empty() {
                return None;
            }

            return Some(Self {
                module_paths,
                imported_names: Vec::new(),
            });
        }

        None
    }
}

fn resolve_reference_target<'a>(
    reference: &RepositoryCodeReferenceRecord,
    candidates: Option<&[&'a RepositoryCodeSymbolRecord]>,
) -> Resolution<'a> {
    let Some(candidates) = candidates else {
        return Resolution::Unresolved;
    };
    if candidates.len() == 1 {
        return Resolution::Resolved(candidates[0]);
    }

    let same_path = candidates
        .iter()
        .copied()
        .filter(|symbol| symbol.path == reference.path)
        .collect::<Vec<_>>();
    if same_path.len() == 1 {
        return Resolution::Resolved(same_path[0]);
    }

    Resolution::Ambiguous
}

fn resolve_python_import(
    request: &PythonImportRequest,
    local_modules: &BTreeSet<String>,
    by_name: &BTreeMap<&str, Vec<&RepositoryCodeSymbolRecord>>,
) -> ImportResolution {
    if request.imported_names.is_empty() {
        return if request
            .module_paths
            .iter()
            .any(|module_path| local_modules.contains(module_path))
        {
            ImportResolution::Resolved
        } else {
            ImportResolution::Unresolved
        };
    }

    let mut resolved = 0usize;
    let mut ambiguous = false;
    for name in &request.imported_names {
        match resolve_imported_symbol(name, &request.module_paths, by_name) {
            ImportResolution::Resolved => resolved += 1,
            ImportResolution::Ambiguous => ambiguous = true,
            ImportResolution::Unresolved => {}
        }
    }
    if ambiguous || (resolved > 0 && resolved < request.imported_names.len()) {
        return ImportResolution::Ambiguous;
    }
    if resolved == request.imported_names.len() {
        return ImportResolution::Resolved;
    }

    ImportResolution::Unresolved
}

fn resolve_imported_symbol(
    name: &str,
    module_paths: &[String],
    by_name: &BTreeMap<&str, Vec<&RepositoryCodeSymbolRecord>>,
) -> ImportResolution {
    let Some(candidates) = by_name.get(name) else {
        return ImportResolution::Unresolved;
    };
    let module_matches = candidates
        .iter()
        .copied()
        .filter(|symbol| {
            module_paths
                .iter()
                .any(|module_path| symbol_path_matches_module(&symbol.path, module_path))
        })
        .collect::<Vec<_>>();
    if module_matches.is_empty() {
        return ImportResolution::Unresolved;
    }

    match module_matches.len() {
        0 => ImportResolution::Unresolved,
        1 => ImportResolution::Resolved,
        _ => ImportResolution::Ambiguous,
    }
}

fn local_python_modules(files: &[RepositoryCodeFileRecord]) -> BTreeSet<String> {
    files
        .iter()
        .filter(|file| file.language_id == "python")
        .filter_map(|file| python_file_module_path(&file.path))
        .collect()
}

fn python_file_module_path(path: &str) -> Option<String> {
    let path = path
        .strip_suffix(".py")
        .or_else(|| path.strip_suffix(".pyw"))?;
    let module_path = strip_source_root(path);
    if module_path == "__init__" {
        return None;
    }

    Some(
        module_path
            .strip_suffix("/__init__")
            .unwrap_or(module_path)
            .to_owned(),
    )
}

fn python_module_path_candidates(import_path: &str, module: &str) -> Vec<String> {
    let module = module.trim();
    if module.is_empty() {
        return Vec::new();
    }
    let mut candidates = Vec::new();
    if module.starts_with('.') {
        if let Some(relative) = relative_python_module_path(import_path, module) {
            candidates.push(relative);
        }
    } else {
        candidates.push(module.replace('.', "/"));
    }
    candidates.sort();
    candidates.dedup();

    candidates
}

fn relative_python_module_path(import_path: &str, module: &str) -> Option<String> {
    let dot_count = module
        .chars()
        .take_while(|character| *character == '.')
        .count();
    let remainder = module[dot_count..].replace('.', "/");
    let mut package = import_path
        .rsplit_once('/')
        .map_or(Vec::new(), |(parent, _)| {
            parent.split('/').collect::<Vec<_>>()
        });
    let drop_count = dot_count.saturating_sub(1);
    if drop_count > package.len() {
        return None;
    }
    for _ in 0..drop_count {
        package.pop();
    }
    if !remainder.is_empty() {
        package.extend(remainder.split('/'));
    }
    if package.is_empty() {
        return None;
    }

    Some(strip_source_root(&package.join("/")).to_owned())
}

fn parse_python_imported_names(names: &str) -> Vec<String> {
    names
        .replace(['(', ')', '\\'], " ")
        .split(',')
        .filter_map(|part| {
            let name = part
                .trim()
                .split_once(" as ")
                .map_or(part.trim(), |(name, _)| name.trim());
            let name = name.trim_start_matches('.');
            (!name.is_empty() && name != "*").then(|| name.to_owned())
        })
        .collect()
}

fn symbol_path_matches_module(path: &str, module_path: &str) -> bool {
    let path = strip_source_root(path);
    path == format!("{module_path}.py")
        || path == format!("{module_path}.pyw")
        || path == format!("{module_path}/__init__.py")
        || path.starts_with(&format!("{module_path}/"))
}

fn strip_source_root(path: &str) -> &str {
    path.strip_prefix("src/").unwrap_or(path)
}

fn path_prefix(qualified_name: &str) -> &str {
    qualified_name
        .rsplit_once("::")
        .map_or(qualified_name, |(prefix, _)| prefix)
}

fn container_kind(kind: &str) -> bool {
    matches!(
        kind,
        "class" | "constructor" | "function" | "interface" | "method" | "module" | "type"
    )
}

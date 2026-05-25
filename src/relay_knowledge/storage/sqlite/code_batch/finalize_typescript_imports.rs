use std::collections::{BTreeMap, BTreeSet};

use super::{
    ImportResolution, SymbolKey, normalize_join, parent_dir, parse_quoted_specifier,
    path_matches_candidate, resolve_first_module_file,
};

const MODULE_EXTENSIONS: [&str; 8] = ["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"];

pub(super) fn needs_symbol_index(import_path: &str, statement: &str) -> bool {
    TypeScriptImportRequest::parse(import_path, statement)
        .is_some_and(|request| !request.imported_names.is_empty())
}

pub(super) struct NamedImportBinding {
    pub(super) imported_name: String,
    pub(super) local_name: String,
}

pub(super) fn named_import_bindings(statement: &str) -> Vec<NamedImportBinding> {
    let statement = statement.trim().trim_end_matches(';').trim();
    let Some(body) = statement.strip_prefix("import ") else {
        return Vec::new();
    };
    let Some((imports, _)) = body.rsplit_once(" from ") else {
        return Vec::new();
    };

    parse_named_import_bindings(imports)
}

pub(super) fn resolve_import(
    import_path: &str,
    statement: &str,
    indexed_module_paths: &BTreeMap<String, Vec<String>>,
    symbols_by_name: &BTreeMap<String, Vec<SymbolKey>>,
) -> ImportResolution {
    let Some(request) = TypeScriptImportRequest::parse(import_path, statement) else {
        return ImportResolution::Unresolved;
    };
    if request.module_paths.is_empty() {
        return ImportResolution::Unresolved;
    }
    if request.imported_names.is_empty() {
        return resolve_first_module_file(&request.module_paths, true, indexed_module_paths);
    }

    combined_named_import_resolution(
        request
            .imported_names
            .iter()
            .map(|name| resolve_named_import_path(name, &request.module_paths, symbols_by_name)),
    )
}

struct TypeScriptImportRequest {
    module_paths: Vec<String>,
    imported_names: Vec<String>,
}

impl TypeScriptImportRequest {
    fn parse(import_path: &str, statement: &str) -> Option<Self> {
        let statement = statement.trim().trim_end_matches(';').trim();
        if let Some(specifier) = dynamic_import_specifier(statement) {
            return Some(Self::for_specifier(import_path, specifier, Vec::new()));
        }
        if let Some(body) = statement.strip_prefix("import ") {
            return Self::parse_import_body(import_path, body);
        }
        if let Some(body) = statement.strip_prefix("export ") {
            return Self::parse_export_body(import_path, body);
        }

        None
    }

    fn parse_import_body(import_path: &str, body: &str) -> Option<Self> {
        if !body.contains(" from ") {
            let specifier = parse_quoted_specifier(body)?;
            return Some(Self::for_specifier(import_path, specifier, Vec::new()));
        }

        let (imports, module) = body.rsplit_once(" from ")?;
        let specifier = parse_quoted_specifier(module)?;

        Some(Self::for_specifier(
            import_path,
            specifier,
            parse_named_imports(imports),
        ))
    }

    fn parse_export_body(import_path: &str, body: &str) -> Option<Self> {
        let body = body.trim().strip_prefix("type ").unwrap_or(body.trim());
        let (exports, module) = body.rsplit_once(" from ")?;
        let specifier = parse_quoted_specifier(module)?;

        Some(Self::for_specifier(
            import_path,
            specifier,
            parse_named_imports(exports),
        ))
    }

    fn for_specifier(import_path: &str, specifier: &str, imported_names: Vec<String>) -> Self {
        Self {
            module_paths: relative_module_candidates(import_path, specifier),
            imported_names,
        }
    }
}

fn dynamic_import_specifier(statement: &str) -> Option<&str> {
    let expression = statement
        .trim()
        .strip_prefix("await ")
        .unwrap_or(statement.trim())
        .trim();
    let arguments = expression.strip_prefix("import")?.trim_start();
    arguments
        .starts_with('(')
        .then(|| parse_quoted_specifier(arguments))
        .flatten()
}

fn parse_named_imports(imports: &str) -> Vec<String> {
    parse_named_import_bindings(imports)
        .into_iter()
        .map(|binding| binding.imported_name)
        .collect()
}

fn parse_named_import_bindings(imports: &str) -> Vec<NamedImportBinding> {
    let imports = imports
        .trim()
        .strip_prefix("type ")
        .unwrap_or(imports.trim());
    let Some(start) = imports.find('{') else {
        return Vec::new();
    };
    let Some(end) = imports[start + 1..]
        .find('}')
        .map(|offset| start + 1 + offset)
    else {
        return Vec::new();
    };

    imports[start + 1..end]
        .split(',')
        .filter_map(|part| {
            let part = part
                .trim()
                .strip_prefix("type ")
                .unwrap_or(part.trim())
                .trim();
            let (imported_name, local_name) = part
                .split_once(" as ")
                .map_or((part, part), |(name, alias)| (name.trim(), alias.trim()));
            (!imported_name.is_empty() && !local_name.is_empty()).then(|| NamedImportBinding {
                imported_name: imported_name.to_owned(),
                local_name: local_name.to_owned(),
            })
        })
        .collect()
}

fn relative_module_candidates(import_path: &str, specifier: &str) -> Vec<String> {
    let base_path = if specifier.starts_with("./") || specifier.starts_with("../") {
        let Some(base_path) = normalize_join(parent_dir(import_path), specifier) else {
            return Vec::new();
        };
        base_path
    } else if specifier.contains('/') {
        specifier.to_owned()
    } else {
        return Vec::new();
    };
    let mut candidates = Vec::new();
    push_module_file_candidates(&mut candidates, &base_path);
    for extension in MODULE_EXTENSIONS {
        candidates.push(format!("{base_path}/index.{extension}"));
    }
    candidates.sort();
    candidates.dedup();

    candidates
}

fn push_module_file_candidates(candidates: &mut Vec<String>, base_path: &str) {
    if let Some((stem, extension)) = base_path.rsplit_once('.')
        && MODULE_EXTENSIONS.contains(&extension)
    {
        candidates.push(base_path.to_owned());
        for replacement in MODULE_EXTENSIONS {
            candidates.push(format!("{stem}.{replacement}"));
        }
        return;
    }
    for extension in MODULE_EXTENSIONS {
        candidates.push(format!("{base_path}.{extension}"));
    }
}

fn resolve_named_import_path(
    name: &str,
    module_paths: &[String],
    symbols_by_name: &BTreeMap<String, Vec<SymbolKey>>,
) -> ImportResolution {
    let mut matched_path = None::<String>;
    let mut match_count = 0usize;
    let Some(symbols) = symbols_by_name.get(name) else {
        return ImportResolution::Unresolved;
    };
    for symbol in symbols {
        if module_paths
            .iter()
            .any(|module_path| path_matches_candidate(&symbol.path, module_path))
        {
            match_count += 1;
            if match_count > 1 {
                return ImportResolution::Ambiguous;
            }
            matched_path = Some(symbol.path.clone());
        }
    }

    matched_path.map_or(ImportResolution::Unresolved, ImportResolution::Resolved)
}

fn combined_named_import_resolution(
    results: impl IntoIterator<Item = ImportResolution>,
) -> ImportResolution {
    let mut total = 0usize;
    let mut resolved = 0usize;
    let mut target_paths = BTreeSet::<String>::new();
    for result in results {
        total += 1;
        match result {
            ImportResolution::Resolved(path) => {
                resolved += 1;
                target_paths.insert(path);
            }
            ImportResolution::Ambiguous => return ImportResolution::Ambiguous,
            ImportResolution::Unresolved => {}
        }
    }
    if total == 0 {
        return ImportResolution::Unresolved;
    }
    if resolved == total
        && target_paths.len() == 1
        && let Some(path) = target_paths.into_iter().next()
    {
        return ImportResolution::Resolved(path);
    }
    if resolved > 0 {
        return ImportResolution::Ambiguous;
    }

    ImportResolution::Unresolved
}

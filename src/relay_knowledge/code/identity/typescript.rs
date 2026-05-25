use crate::domain::CodeImportRecord;

use super::imports::{
    ImportContext, ImportResolution, combined_resolution, normalize_join, parent_dir,
    parse_quoted_specifier,
};

const MODULE_EXTENSIONS: [&str; 8] = ["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"];

pub(super) fn resolve_import(
    import: &CodeImportRecord,
    context: &ImportContext<'_>,
) -> Option<ImportResolution> {
    let request = TypeScriptImportRequest::parse(&import.path, &import.module)?;
    if request.module_paths.is_empty() {
        return Some(ImportResolution::Unresolved);
    }
    if request.imported_names.is_empty() {
        return Some(if context.any_module_file_exists(&request.module_paths) {
            ImportResolution::Resolved
        } else {
            ImportResolution::Unresolved
        });
    }

    Some(combined_resolution(request.imported_names.iter().map(
        |name| context.resolve_name_in_paths(name, &request.module_paths),
    )))
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
            let name = part
                .trim()
                .strip_prefix("type ")
                .unwrap_or(part.trim())
                .split_once(" as ")
                .map_or(part.trim(), |(name, _)| name.trim());
            (!name.is_empty()).then(|| name.to_owned())
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
    if let Some((stem, extension)) = base_path.rsplit_once('.') {
        if MODULE_EXTENSIONS.contains(&extension) {
            candidates.push(base_path.to_owned());
            for replacement in MODULE_EXTENSIONS {
                candidates.push(format!("{stem}.{replacement}"));
            }
            return;
        }
    }
    for extension in MODULE_EXTENSIONS {
        candidates.push(format!("{base_path}.{extension}"));
    }
}

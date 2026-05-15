use crate::domain::CodeImportRecord;

use super::imports::{
    ImportContext, ImportResolution, normalize_join, parent_dir, parse_quoted_specifier,
};

pub(super) fn resolve_import(
    import: &CodeImportRecord,
    context: &ImportContext<'_>,
) -> Option<ImportResolution> {
    let request = CppImportRequest::parse(&import.path, &import.module)?;

    Some(match request {
        CppImportRequest::Include { candidates } => {
            if context.any_module_file_exists(&candidates) {
                ImportResolution::Resolved
            } else {
                ImportResolution::Unresolved
            }
        }
        CppImportRequest::UsingSymbol { namespace, name } => {
            if namespace.is_empty() {
                context.resolve_name(&name)
            } else {
                context.resolve_name_in_namespace(&namespace, &name)
            }
        }
        CppImportRequest::UsingNamespace { namespace } => {
            if context.namespace_exists(&namespace) {
                ImportResolution::Resolved
            } else {
                ImportResolution::Unresolved
            }
        }
    })
}

enum CppImportRequest {
    Include { candidates: Vec<String> },
    UsingSymbol { namespace: String, name: String },
    UsingNamespace { namespace: String },
}

impl CppImportRequest {
    fn parse(import_path: &str, statement: &str) -> Option<Self> {
        let statement = statement.trim().trim_end_matches(';').trim();
        if statement.starts_with("#include") {
            return parse_include(import_path, statement);
        }
        if let Some(namespace) = statement.strip_prefix("using namespace ") {
            let namespace = namespace.trim().replace("::", ".");
            if namespace.is_empty() {
                return None;
            }
            return Some(Self::UsingNamespace { namespace });
        }
        if let Some(name) = statement.strip_prefix("using ") {
            if name.contains('=') {
                return None;
            }
            let (namespace, name) = name.rsplit_once("::").unwrap_or(("", name));
            let name = name.trim();
            if name.is_empty() {
                return None;
            }
            return Some(Self::UsingSymbol {
                namespace: namespace.trim().to_owned(),
                name: name.to_owned(),
            });
        }

        None
    }
}

fn parse_include(import_path: &str, statement: &str) -> Option<CppImportRequest> {
    let target = parse_quoted_specifier(statement).or_else(|| parse_angle_specifier(statement))?;
    let mut candidates = Vec::new();
    if let Some(relative) = normalize_join(parent_dir(import_path), target) {
        candidates.push(relative);
    }
    candidates.push(target.to_owned());
    candidates.sort();
    candidates.dedup();

    Some(CppImportRequest::Include { candidates })
}

fn parse_angle_specifier(statement: &str) -> Option<&str> {
    let start = statement.find('<')?;
    let rest = &statement[start + 1..];
    let end = rest.find('>')?;

    Some(&rest[..end])
}

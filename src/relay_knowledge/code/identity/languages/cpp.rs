use crate::domain::CodeImportRecord;

use super::super::import_resolution::{
    ImportContext, ImportResolution, ModuleFileResolution, normalize_join, parent_dir,
    parse_quoted_specifier,
};

pub(in crate::code::identity) fn resolve_import(
    import: &CodeImportRecord,
    context: &ImportContext<'_>,
) -> Option<(ImportResolution, Option<String>)> {
    let request = CppImportRequest::parse(&import.path, &import.module)?;

    Some(match request {
        CppImportRequest::Include {
            candidates,
            allow_source_root_match,
        } => match context.resolve_first_module_file(&candidates, allow_source_root_match) {
            ModuleFileResolution::Resolved(target_hint) => {
                (ImportResolution::Resolved, Some(target_hint))
            }
            ModuleFileResolution::Ambiguous => (ImportResolution::Ambiguous, None),
            ModuleFileResolution::Unresolved => (ImportResolution::Unresolved, None),
        },
        CppImportRequest::UsingSymbol { namespace, name } => {
            let resolution = if namespace.is_empty() {
                context.resolve_name(&name)
            } else {
                context.resolve_name_in_namespace(&namespace, &name)
            };
            (resolution, None)
        }
        CppImportRequest::UsingNamespace { namespace } => {
            let resolution = if context.namespace_exists(&namespace) {
                ImportResolution::Resolved
            } else {
                ImportResolution::Unresolved
            };
            (resolution, None)
        }
    })
}

enum CppImportRequest {
    Include {
        candidates: Vec<String>,
        allow_source_root_match: bool,
    },
    UsingSymbol {
        namespace: String,
        name: String,
    },
    UsingNamespace {
        namespace: String,
    },
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
    let (target, quoted) = if let Some(target) = parse_quoted_specifier(statement) {
        (target, true)
    } else {
        (parse_angle_specifier(statement)?, false)
    };
    let mut candidates = Vec::new();
    if quoted {
        if let Some(relative) = normalize_join(parent_dir(import_path), target) {
            candidates.push(relative);
        }
    }
    push_candidate(&mut candidates, target.to_owned());
    if !target.starts_with("include/") {
        push_candidate(&mut candidates, format!("include/{target}"));
    }

    Some(CppImportRequest::Include {
        candidates,
        allow_source_root_match: quoted,
    })
}

fn push_candidate(candidates: &mut Vec<String>, candidate: String) {
    if !candidates.contains(&candidate) {
        candidates.push(candidate);
    }
}

fn parse_angle_specifier(statement: &str) -> Option<&str> {
    let start = statement.find('<')?;
    let rest = &statement[start + 1..];
    let end = rest.find('>')?;

    Some(&rest[..end])
}

use crate::domain::CodeImportRecord;

use super::super::import_resolution::{
    ImportContext, ImportResolution, ModuleFileResolution, normalize_join, parent_dir,
    parse_quoted_specifier,
};

pub(in crate::code::identity) fn resolve_import(
    import: &CodeImportRecord,
    context: &ImportContext<'_>,
) -> Option<(ImportResolution, Option<String>)> {
    let statement = import.module.trim().trim_end_matches(';').trim();
    let IncludeRequest {
        candidates,
        allow_source_root_match,
    } = parse_include(&import.path, statement)?;

    Some(
        match context.resolve_first_module_file(&candidates, allow_source_root_match) {
            ModuleFileResolution::Resolved(target_hint) => {
                (ImportResolution::Resolved, Some(target_hint))
            }
            ModuleFileResolution::Ambiguous => (ImportResolution::Ambiguous, None),
            ModuleFileResolution::Unresolved => (ImportResolution::Unresolved, None),
        },
    )
}

struct IncludeRequest {
    candidates: Vec<String>,
    allow_source_root_match: bool,
}

fn parse_include(import_path: &str, statement: &str) -> Option<IncludeRequest> {
    if !statement.starts_with("#include") {
        return None;
    }
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

    Some(IncludeRequest {
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

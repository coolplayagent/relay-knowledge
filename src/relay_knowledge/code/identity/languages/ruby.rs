use crate::domain::CodeImportRecord;

use super::super::import_resolution::{
    ImportContext, ImportResolution, ModuleFileResolution, normalize_join, parent_dir,
    parse_quoted_specifier,
};

pub(in crate::code::identity) fn resolve_import(
    import: &CodeImportRecord,
    context: &ImportContext<'_>,
) -> Option<(ImportResolution, Option<String>)> {
    let statement = import.module.lines().find_map(ruby_import_statement)?;
    let specifier = parse_quoted_specifier(statement)?;
    let relative = ruby_keyword_statement(statement, "require_relative")
        || ruby_keyword_statement(statement, "load")
        || specifier.starts_with("./")
        || specifier.starts_with("../");
    let candidates = ruby_module_candidates(&import.path, specifier, relative);
    Some(resolve_module_candidates(context, &candidates))
}

fn ruby_import_statement(statement: &str) -> Option<&str> {
    let statement = statement.trim();
    let import_like = ["require", "require_relative", "load"]
        .iter()
        .any(|keyword| ruby_keyword_statement(statement, keyword));

    import_like.then_some(statement)
}

fn ruby_module_candidates(import_path: &str, specifier: &str, relative: bool) -> Vec<String> {
    let mut candidates = Vec::new();
    if relative {
        if let Some(candidate) = normalize_join(parent_dir(import_path), specifier) {
            push_module_file_candidates(&mut candidates, &candidate);
        }
    } else {
        push_module_file_candidates(&mut candidates, specifier);
        push_module_file_candidates(&mut candidates, &format!("lib/{specifier}"));
    }

    candidates
}

fn push_module_file_candidates(candidates: &mut Vec<String>, base_path: &str) {
    push_unique(candidates, base_path.to_owned());
    if base_path.rsplit_once('.').is_some() {
        return;
    }
    push_unique(candidates, format!("{base_path}.rb"));
}

fn resolve_module_candidates(
    context: &ImportContext<'_>,
    candidates: &[String],
) -> (ImportResolution, Option<String>) {
    match context.resolve_first_module_file(candidates, true) {
        ModuleFileResolution::Resolved(target_hint) => {
            (ImportResolution::Resolved, Some(target_hint))
        }
        ModuleFileResolution::Ambiguous => (ImportResolution::Ambiguous, None),
        ModuleFileResolution::Unresolved => (ImportResolution::Unresolved, None),
    }
}

fn ruby_keyword_statement(statement: &str, keyword: &str) -> bool {
    statement
        .strip_prefix(keyword)
        .is_some_and(|rest| rest.starts_with(char::is_whitespace) || rest.starts_with('('))
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.contains(&value) {
        values.push(value);
    }
}

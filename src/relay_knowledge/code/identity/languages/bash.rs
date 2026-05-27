use crate::domain::CodeImportRecord;

use super::super::import_resolution::{
    ImportContext, ImportResolution, ModuleFileResolution, normalize_join, parent_dir,
};

pub(in crate::code::identity) fn resolve_import(
    import: &CodeImportRecord,
    context: &ImportContext<'_>,
) -> Option<(ImportResolution, Option<String>)> {
    let target = import
        .module
        .lines()
        .find_map(shellcheck_source_target)
        .or_else(|| import.module.lines().find_map(bash_source_target))?;
    let candidates = shell_source_candidates(&import.path, &target);
    Some(resolve_module_candidates(context, &candidates))
}

fn shellcheck_source_target(line: &str) -> Option<String> {
    let statement = line.trim();
    if !statement.starts_with('#') {
        return None;
    }
    let target = statement.split_whitespace().find_map(|part| {
        part.strip_prefix("source=")
            .map(|value| trim_shell_quotes(value).to_owned())
    })?;

    (!target.is_empty()).then_some(target)
}

fn bash_source_target(line: &str) -> Option<String> {
    let statement = line.trim();
    let rest = statement
        .strip_prefix("source")
        .filter(|rest| rest.starts_with(char::is_whitespace))
        .or_else(|| {
            statement
                .strip_prefix('.')
                .filter(|rest| rest.starts_with(char::is_whitespace))
        })?;
    let target = first_shell_word(rest.trim())?;

    (!target.contains('$')).then(|| target.to_owned())
}

fn shell_source_candidates(import_path: &str, target: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    if let Some(relative) = normalize_join(parent_dir(import_path), target) {
        push_module_file_candidates(&mut candidates, &relative, &["sh", "bash"]);
    }
    push_module_file_candidates(&mut candidates, target, &["sh", "bash"]);

    candidates
}

fn first_shell_word(value: &str) -> Option<&str> {
    let first = value.chars().next()?;
    if matches!(first, '"' | '\'') {
        let rest = value.get(first.len_utf8()..)?;
        let quoted_end = rest.find(first)?;
        return Some(&rest[..quoted_end]);
    }

    value.split_whitespace().next()
}

fn trim_shell_quotes(value: &str) -> &str {
    let Some(first) = value
        .chars()
        .next()
        .filter(|quote| matches!(quote, '"' | '\''))
    else {
        return value;
    };
    value
        .get(first.len_utf8()..)
        .and_then(|rest| rest.strip_suffix(first))
        .unwrap_or(value)
}

fn push_module_file_candidates(candidates: &mut Vec<String>, base_path: &str, extensions: &[&str]) {
    push_unique(candidates, base_path.to_owned());
    if base_path.rsplit_once('.').is_some() {
        return;
    }
    for extension in extensions {
        push_unique(candidates, format!("{base_path}.{extension}"));
    }
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

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.contains(&value) {
        values.push(value);
    }
}

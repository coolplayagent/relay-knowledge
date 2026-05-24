use crate::domain::CodeImportRecord;

use super::imports::{
    ImportContext, ImportResolution, ModuleFileResolution, normalize_join, parent_dir,
    parse_quoted_specifier,
};

pub(super) fn resolve_import(
    language_id: &str,
    import: &CodeImportRecord,
    context: &ImportContext<'_>,
) -> Option<(ImportResolution, Option<String>)> {
    match language_id {
        "bash" => resolve_bash_import(import, context),
        "ruby" => resolve_ruby_import(import, context),
        _ => None,
    }
}

fn resolve_ruby_import(
    import: &CodeImportRecord,
    context: &ImportContext<'_>,
) -> Option<(ImportResolution, Option<String>)> {
    let statement = import.module.lines().find_map(ruby_import_statement)?;
    let specifier = parse_quoted_specifier(statement)?;
    let relative = script_keyword_statement(statement, "require_relative")
        || script_keyword_statement(statement, "load")
        || specifier.starts_with("./")
        || specifier.starts_with("../");
    let candidates = ruby_module_candidates(&import.path, specifier, relative);
    Some(resolve_module_candidates(context, &candidates))
}

fn resolve_bash_import(
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

fn ruby_import_statement(statement: &str) -> Option<&str> {
    let statement = statement.trim();
    let import_like = ["require", "require_relative", "load"]
        .iter()
        .any(|keyword| script_keyword_statement(statement, keyword));

    import_like.then_some(statement)
}

fn ruby_module_candidates(import_path: &str, specifier: &str, relative: bool) -> Vec<String> {
    let mut candidates = Vec::new();
    if relative {
        if let Some(candidate) = normalize_join(parent_dir(import_path), specifier) {
            push_module_file_candidates(&mut candidates, &candidate, &["rb"]);
        }
    } else {
        push_module_file_candidates(&mut candidates, specifier, &["rb"]);
        push_module_file_candidates(&mut candidates, &format!("lib/{specifier}"), &["rb"]);
    }

    candidates
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

fn script_keyword_statement(statement: &str, keyword: &str) -> bool {
    statement
        .strip_prefix(keyword)
        .is_some_and(|rest| rest.starts_with(char::is_whitespace) || rest.starts_with('('))
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.contains(&value) {
        values.push(value);
    }
}

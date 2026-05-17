use crate::domain::CodeImportRecord;

use super::imports::{ImportContext, ImportResolution, ModuleFileResolution};

pub(super) fn resolve_import(
    import: &CodeImportRecord,
    context: &ImportContext<'_>,
) -> Option<(ImportResolution, Option<String>)> {
    let import_path = go_import_path(&import.module)?;
    match context.resolve_go_directory_with_language_files(&import_path) {
        ModuleFileResolution::Resolved(target_hint) => {
            Some((ImportResolution::Resolved, Some(target_hint)))
        }
        ModuleFileResolution::Ambiguous => Some((ImportResolution::Ambiguous, None)),
        ModuleFileResolution::Unresolved => Some((ImportResolution::Unresolved, None)),
    }
}

fn go_import_path(module: &str) -> Option<String> {
    let path = module
        .trim()
        .trim_end_matches(';')
        .split_whitespace()
        .last()?;
    let path = path
        .trim_matches('"')
        .trim_matches('`')
        .trim_matches('\'')
        .trim();
    (!path.is_empty()).then(|| path.to_owned())
}

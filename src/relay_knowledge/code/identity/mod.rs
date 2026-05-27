mod import_resolution;
mod languages;
mod references;
mod symbols;

use crate::domain::{CodeImportRecord, RepositoryCodeFileRecord, RepositoryCodeSymbolRecord};

pub(super) use references::resolve_reference_targets;
pub(super) use symbols::enrich_symbol_identities;

pub(super) fn resolve_import_targets(
    files: &[RepositoryCodeFileRecord],
    symbols: &[RepositoryCodeSymbolRecord],
    imports: &mut [CodeImportRecord],
) {
    let context = import_resolution::ImportContext::new(files, symbols);
    for import in imports {
        import.target_hint = Some(import.module.clone());
        let Some(language_id) = context.language_for_path(&import.path) else {
            continue;
        };
        let resolution = match language_id {
            "python" => languages::python::resolve_import(import, &context),
            "go" => match languages::go::resolve_import(import, &context) {
                Some((resolution, target_hint)) => {
                    if let Some(target_hint) = target_hint {
                        import.target_hint = Some(target_hint);
                    }
                    Some(resolution)
                }
                None => None,
            },
            "java" => match languages::java::resolve_import(import, &context) {
                Some((resolution, target_hint)) => {
                    if let Some(target_hint) = target_hint {
                        import.target_hint = Some(target_hint);
                    }
                    Some(resolution)
                }
                None => None,
            },
            "typescript" | "tsx" => languages::typescript::resolve_import(import, &context),
            "bash" => match languages::bash::resolve_import(import, &context) {
                Some((resolution, target_hint)) => {
                    if let Some(target_hint) = target_hint {
                        import.target_hint = Some(target_hint);
                    }
                    Some(resolution)
                }
                None => None,
            },
            "ruby" => match languages::ruby::resolve_import(import, &context) {
                Some((resolution, target_hint)) => {
                    if let Some(target_hint) = target_hint {
                        import.target_hint = Some(target_hint);
                    }
                    Some(resolution)
                }
                None => None,
            },
            "c" => match languages::c::resolve_import(import, &context) {
                Some((resolution, target_hint)) => {
                    if let Some(target_hint) = target_hint {
                        import.target_hint = Some(target_hint);
                    }
                    Some(resolution)
                }
                None => None,
            },
            "cpp" => match languages::cpp::resolve_import(import, &context) {
                Some((resolution, target_hint)) => {
                    if let Some(target_hint) = target_hint {
                        import.target_hint = Some(target_hint);
                    }
                    Some(resolution)
                }
                None => None,
            },
            _ => None,
        };
        if let Some(resolution) = resolution {
            import_resolution::apply_resolution(import, resolution);
        }
    }
}

#[cfg(test)]
#[path = "import_resolution_tests.rs"]
mod import_resolution_tests;

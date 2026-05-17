mod cpp;
mod go;
mod imports;
mod java;
mod python;
mod references;
mod symbols;
mod typescript;

use crate::domain::{CodeImportRecord, RepositoryCodeFileRecord, RepositoryCodeSymbolRecord};

pub(super) use references::resolve_reference_targets;
pub(super) use symbols::enrich_symbol_identities;

pub(super) fn resolve_import_targets(
    files: &[RepositoryCodeFileRecord],
    symbols: &[RepositoryCodeSymbolRecord],
    imports: &mut [CodeImportRecord],
) {
    let context = imports::ImportContext::new(files, symbols);
    for import in imports {
        import.target_hint = Some(import.module.clone());
        let Some(language_id) = context.language_for_path(&import.path) else {
            continue;
        };
        let resolution = match language_id {
            "python" => python::resolve_import(import, &context),
            "go" => match go::resolve_import(import, &context) {
                Some((resolution, target_hint)) => {
                    if let Some(target_hint) = target_hint {
                        import.target_hint = Some(target_hint);
                    }
                    Some(resolution)
                }
                None => None,
            },
            "java" => java::resolve_import(import, &context),
            "typescript" | "tsx" => typescript::resolve_import(import, &context),
            "c" | "cpp" => match cpp::resolve_import(import, &context) {
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
            imports::apply_resolution(import, resolution);
        }
    }
}

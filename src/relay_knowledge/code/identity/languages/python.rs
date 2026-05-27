use crate::domain::CodeImportRecord;

use super::super::import_resolution::{
    ImportContext, ImportResolution, combined_resolution, normalize_join, parent_dir,
    strip_source_root,
};

pub(in crate::code::identity) fn resolve_import(
    import: &CodeImportRecord,
    context: &ImportContext<'_>,
) -> Option<ImportResolution> {
    let request = PythonImportRequest::parse(&import.path, &import.module)?;

    Some(resolve_python_import(&request, context))
}

struct PythonImportRequest {
    module_paths: Vec<String>,
    imported_names: Vec<String>,
}

impl PythonImportRequest {
    fn parse(import_path: &str, statement: &str) -> Option<Self> {
        if !(import_path.ends_with(".py") || import_path.ends_with(".pyw")) {
            return None;
        }
        let statement = statement.trim().trim_end_matches(';').trim();
        if let Some(body) = statement.strip_prefix("from ") {
            let (module, names) = body.split_once(" import ")?;
            let module_paths = python_module_path_candidates(import_path, module.trim());
            let imported_names = parse_python_imported_names(names);
            if module_paths.is_empty() && imported_names.is_empty() {
                return None;
            }

            return Some(Self {
                module_paths,
                imported_names,
            });
        }
        if let Some(body) = statement.strip_prefix("import ") {
            let module_paths = body
                .split(',')
                .filter_map(|part| {
                    let module = part
                        .trim()
                        .split_once(" as ")
                        .map_or(part.trim(), |(module, _)| module.trim());
                    python_module_path_candidates(import_path, module)
                        .into_iter()
                        .next()
                })
                .collect::<Vec<_>>();
            if module_paths.is_empty() {
                return None;
            }

            return Some(Self {
                module_paths,
                imported_names: Vec::new(),
            });
        }

        None
    }
}

fn resolve_python_import(
    request: &PythonImportRequest,
    context: &ImportContext<'_>,
) -> ImportResolution {
    if request.imported_names.is_empty() {
        return if request
            .module_paths
            .iter()
            .any(|module_path| python_module_exists(context, module_path))
        {
            ImportResolution::Resolved
        } else {
            ImportResolution::Unresolved
        };
    }

    combined_resolution(
        request.imported_names.iter().map(|name| {
            resolve_python_imported_name(context, name, request.module_paths.as_slice())
        }),
    )
}

fn resolve_python_imported_name(
    context: &ImportContext<'_>,
    name: &str,
    module_paths: &[String],
) -> ImportResolution {
    let symbol_paths = module_paths
        .iter()
        .flat_map(|module_path| python_module_files(module_path))
        .collect::<Vec<_>>();
    match context.resolve_name_in_paths(name, &symbol_paths) {
        ImportResolution::Unresolved => {
            if module_paths
                .iter()
                .any(|module_path| python_module_exists(context, &format!("{module_path}/{name}")))
            {
                ImportResolution::Resolved
            } else {
                ImportResolution::Unresolved
            }
        }
        resolution => resolution,
    }
}

fn python_module_exists(context: &ImportContext<'_>, module_path: &str) -> bool {
    python_module_files(module_path)
        .iter()
        .any(|file_path| context.module_file_exists(file_path))
}

fn python_module_files(module_path: &str) -> Vec<String> {
    vec![
        format!("{module_path}.py"),
        format!("{module_path}.pyw"),
        format!("{module_path}/__init__.py"),
    ]
}

fn python_module_path_candidates(import_path: &str, module: &str) -> Vec<String> {
    let module = module.trim();
    if module.is_empty() {
        return Vec::new();
    }
    let mut candidates = Vec::new();
    if module.starts_with('.') {
        if let Some(relative) = relative_python_module_path(import_path, module) {
            candidates.push(relative);
        }
    } else {
        candidates.push(module.replace('.', "/"));
    }
    candidates.sort();
    candidates.dedup();

    candidates
}

fn relative_python_module_path(import_path: &str, module: &str) -> Option<String> {
    let dot_count = module
        .chars()
        .take_while(|character| *character == '.')
        .count();
    let remainder = module[dot_count..].replace('.', "/");
    let import_path = strip_source_root(import_path);
    let mut package = parent_dir(&import_path)
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let drop_count = dot_count.saturating_sub(1);
    if drop_count > package.len() {
        return None;
    }
    for _ in 0..drop_count {
        package.pop();
    }
    let base = package.join("/");
    if remainder.is_empty() {
        return (!base.is_empty()).then_some(base);
    }

    normalize_join(&base, &remainder)
}

fn parse_python_imported_names(names: &str) -> Vec<String> {
    names
        .replace(['(', ')', '\\'], " ")
        .split(',')
        .filter_map(|part| {
            let name = part
                .trim()
                .split_once(" as ")
                .map_or(part.trim(), |(name, _)| name.trim());
            let name = name.trim_start_matches('.');
            (!name.is_empty() && name != "*").then(|| name.to_owned())
        })
        .collect()
}

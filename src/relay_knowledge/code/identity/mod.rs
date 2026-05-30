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
            "javascript" | "jsx" => languages::javascript::resolve_import(import, &context),
            "go" => match languages::go::resolve_import(import, &context) {
                Some(output) => resolved_with_hint(import, output),
                None => None,
            },
            "java" => match languages::java::resolve_import(import, &context) {
                Some(output) => resolved_with_hint(import, output),
                None => None,
            },
            "typescript" | "tsx" => languages::typescript::resolve_import(import, &context),
            "kotlin" => match languages::kotlin::resolve_import(import, &context) {
                Some(output) => resolved_with_hint(import, output),
                None => None,
            },
            "scala" => match languages::scala::resolve_import(import, &context) {
                Some(output) => resolved_with_hint(import, output),
                None => None,
            },
            "csharp" => languages::csharp::resolve_import(import, &context),
            "php" => languages::php::resolve_import(import, &context),
            "rust" => match languages::rust::resolve_import(import, &context) {
                Some(output) => resolved_with_hint(import, output),
                None => None,
            },
            "swift" => match languages::swift::resolve_import(import, &context) {
                Some(output) => resolved_with_hint(import, output),
                None => None,
            },
            "bash" => match languages::bash::resolve_import(import, &context) {
                Some(output) => resolved_with_hint(import, output),
                None => None,
            },
            "ruby" => match languages::ruby::resolve_import(import, &context) {
                Some(output) => resolved_with_hint(import, output),
                None => None,
            },
            "c" => match languages::c::resolve_import(import, &context) {
                Some(output) => resolved_with_hint(import, output),
                None => None,
            },
            "cpp" => match languages::cpp::resolve_import(import, &context) {
                Some(output) => resolved_with_hint(import, output),
                None => None,
            },
            "cmake" | "jinja2" | "make" | "markdown" | "ninja" | "starlark" | "xml" => {
                let candidates =
                    config_import_candidates(language_id, &import.path, &import.module);
                resolved_with_hint(
                    import,
                    import_resolution::module_file_resolution(
                        context.resolve_first_exact_module_file(&candidates),
                    ),
                )
            }
            "gotemplate" => {
                let candidates =
                    config_import_candidates(language_id, &import.path, &import.module);
                let file_resolution = context.resolve_first_exact_module_file(&candidates);
                match import_resolution::module_file_resolution(file_resolution) {
                    (import_resolution::ImportResolution::Unresolved, _) => {
                        resolved_with_hint(import, gotemplate_symbol_resolution(import, &context))
                    }
                    output => resolved_with_hint(import, output),
                }
            }
            _ => None,
        };
        if let Some(resolution) = resolution {
            import_resolution::apply_resolution(import, resolution);
        }
    }
}

fn config_import_candidates(language_id: &str, path: &str, module: &str) -> Vec<String> {
    if language_id == "markdown" {
        return markdown_import_candidates(path, module);
    }

    let mut candidates = Vec::new();
    if let Some(label_path) = starlark_label_path(path, module) {
        candidates.push(label_path);
    }
    if !module.starts_with('/')
        && !module.contains("://")
        && !already_joined_with_parent(path, module)
    {
        if let Some((parent, _)) = path.rsplit_once('/') {
            if let Some(candidate) = import_resolution::normalize_join(parent, module) {
                candidates.push(candidate);
            }
        }
    }
    if language_id == "jinja2"
        && let Some(root) = nearest_template_root(path)
        && let Some(candidate) = import_resolution::normalize_join(root, module)
    {
        push_unique_candidate(&mut candidates, candidate);
    }
    if should_add_root_config_candidate(language_id, path, module) {
        if let Some(candidate) = import_resolution::normalize_join("", module) {
            push_unique_candidate(&mut candidates, candidate);
        } else {
            push_unique_candidate(&mut candidates, module.to_owned());
        }
    }

    candidates
}

fn markdown_import_candidates(path: &str, module: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    let Some(candidate) = module.strip_prefix('/').map_or_else(
        || normalize_markdown_path(import_resolution::parent_dir(path), module),
        |root_relative| normalize_markdown_path("", root_relative),
    ) else {
        return candidates;
    };

    if !candidate.is_empty() {
        push_unique_candidate(&mut candidates, candidate.clone());
    }
    if should_add_markdown_index_candidates(module, &candidate) {
        let directory = (!candidate.is_empty()).then(|| format!("{candidate}/"));
        let directory = directory.as_deref().unwrap_or_default();
        push_unique_candidate(&mut candidates, format!("{directory}README.md"));
        push_unique_candidate(&mut candidates, format!("{directory}README.markdown"));
        push_unique_candidate(&mut candidates, format!("{directory}index.md"));
        push_unique_candidate(&mut candidates, format!("{directory}index.markdown"));
    }
    candidates
}

fn normalize_markdown_path(parent: &str, child: &str) -> Option<String> {
    let mut parts = Vec::<&str>::new();
    for part in parent
        .split('/')
        .chain(child.split('/'))
        .filter(|part| !part.is_empty() && *part != ".")
    {
        if part == ".." {
            parts.pop()?;
        } else {
            parts.push(part);
        }
    }

    Some(parts.join("/"))
}

fn should_add_markdown_index_candidates(module: &str, candidate: &str) -> bool {
    module.ends_with('/')
        || candidate
            .rsplit('/')
            .next()
            .is_some_and(|segment| !segment.contains('.'))
}

fn should_add_root_config_candidate(language_id: &str, path: &str, module: &str) -> bool {
    language_id != "cmake" || !path.contains('/') || already_joined_with_parent(path, module)
}

fn push_unique_candidate(candidates: &mut Vec<String>, candidate: String) {
    if !candidates.iter().any(|existing| existing == &candidate) {
        candidates.push(candidate);
    }
}

fn already_joined_with_parent(path: &str, module: &str) -> bool {
    path.rsplit_once('/').is_some_and(|(parent, _)| {
        module
            .strip_prefix(parent)
            .is_some_and(|rest| rest.starts_with('/'))
    })
}

fn gotemplate_symbol_resolution(
    import: &CodeImportRecord,
    context: &import_resolution::ImportContext<'_>,
) -> (import_resolution::ImportResolution, Option<String>) {
    let parent = import_resolution::parent_dir(&import.path);
    let local = context.resolve_name_in_directory_tree_for_language_and_kinds_with_hint(
        &import.module,
        parent,
        "gotemplate",
        &["template"],
    );
    if local.0 != import_resolution::ImportResolution::Unresolved {
        return local;
    }

    let Some(template_root) = nearest_template_root(&import.path).filter(|root| *root != parent)
    else {
        return local;
    };
    context.resolve_name_in_directory_tree_for_language_and_kinds_with_hint(
        &import.module,
        template_root,
        "gotemplate",
        &["template"],
    )
}

fn nearest_template_root(path: &str) -> Option<&str> {
    let mut root_end = None;
    let mut offset = 0usize;
    for segment in path.split('/') {
        let end = offset + segment.len();
        if segment == "templates" {
            root_end = Some(end);
        }
        offset = end + 1;
    }

    root_end.map(|end| &path[..end])
}

fn starlark_label_path(path: &str, module: &str) -> Option<String> {
    if module.starts_with('@') {
        return None;
    }
    if let Some(rest) = module.strip_prefix("//") {
        let (package, file) = rest.split_once(':')?;
        return import_resolution::normalize_join(package, file);
    }
    if let Some(file) = module.strip_prefix(':') {
        return import_resolution::normalize_join(import_resolution::parent_dir(path), file);
    }

    None
}

fn resolved_with_hint(
    import: &mut CodeImportRecord,
    output: (import_resolution::ImportResolution, Option<String>),
) -> Option<import_resolution::ImportResolution> {
    let (resolution, target_hint) = output;
    if let Some(target_hint) = target_hint {
        import.target_hint = Some(target_hint);
    }

    Some(resolution)
}

#[cfg(test)]
#[path = "import_resolution_tests.rs"]
mod import_resolution_tests;

#[cfg(test)]
#[path = "import_resolution_language_tests.rs"]
mod import_resolution_language_tests;
#[cfg(test)]
#[path = "import_resolution_review_tests.rs"]
mod import_resolution_review_tests;

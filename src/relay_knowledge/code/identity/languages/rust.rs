use crate::domain::CodeImportRecord;

use super::super::import_resolution::{
    ImportContext, ImportResolution, ModuleFileResolution, combined_resolution,
    module_file_resolution, normalize_join, parent_dir,
};

pub(in crate::code::identity) fn resolve_import(
    import: &CodeImportRecord,
    context: &ImportContext<'_>,
) -> Option<(ImportResolution, Option<String>)> {
    let requests = RustImportRequest::parse(&import.path, &import.module)?;
    let mut target_hint = None;
    let resolution = combined_resolution(requests.iter().map(|request| match request {
        RustImportRequest::Module { candidates } => {
            let output =
                module_file_resolution(context.resolve_first_module_file(candidates, true));
            if requests.len() == 1 {
                target_hint = output.1.clone();
            }
            output.0
        }
        RustImportRequest::Symbol {
            name,
            module_candidates,
        } => {
            let file_resolution = context.resolve_first_module_file(module_candidates, true);
            match file_resolution {
                ModuleFileResolution::Resolved(path) => {
                    let resolution =
                        context.resolve_name_in_paths(name, std::slice::from_ref(&path));
                    if requests.len() == 1 {
                        target_hint = Some(path);
                    }
                    resolution
                }
                ModuleFileResolution::Ambiguous => ImportResolution::Ambiguous,
                ModuleFileResolution::Unresolved => {
                    context.resolve_name_in_paths(name, module_candidates)
                }
            }
        }
    }));

    Some((resolution, target_hint))
}

enum RustImportRequest {
    Module {
        candidates: Vec<String>,
    },
    Symbol {
        name: String,
        module_candidates: Vec<String>,
    },
}

impl RustImportRequest {
    fn parse(import_path: &str, statement: &str) -> Option<Vec<Self>> {
        let statement = statement.trim().trim_end_matches(';').trim();
        if let Some(body) = statement.strip_prefix("mod ") {
            let name = body.split_whitespace().next()?.trim();
            return Some(vec![Self::Module {
                candidates: rust_module_candidates(parent_dir(import_path), name),
            }]);
        }
        let path = rust_use_path(statement)?;
        let requests = expand_use_tree(path)
            .into_iter()
            .filter_map(|path| Self::parse_use_path(import_path, &path))
            .collect::<Vec<_>>();

        (!requests.is_empty()).then_some(requests)
    }

    fn parse_use_path(import_path: &str, path: &str) -> Option<Self> {
        let normalized = normalize_use_path(import_path, path)?;
        let Some((module_path, name)) = normalized.rsplit_once("::") else {
            return Some(Self::Module {
                candidates: rust_module_candidates("", &normalized),
            });
        };
        if name == "*" {
            return Some(Self::Module {
                candidates: rust_module_candidates("", &module_path.replace("::", "/")),
            });
        }

        Some(Self::Symbol {
            name: name.to_owned(),
            module_candidates: rust_module_candidates("", &module_path.replace("::", "/")),
        })
    }
}

fn rust_use_path(statement: &str) -> Option<&str> {
    statement
        .strip_prefix("use ")
        .or_else(|| statement.strip_prefix("pub use "))
        .or_else(|| {
            statement
                .strip_prefix("pub(")
                .and_then(|rest| rest.split_once(") use ").map(|(_, body)| body))
        })
        .map(str::trim)
}

fn expand_use_tree(path: &str) -> Vec<String> {
    expand_rust_use_tree(path.trim())
}

fn expand_rust_use_tree(path: &str) -> Vec<String> {
    let Some((prefix, selectors)) = path.split_once("::{") else {
        return vec![strip_rust_alias(path).to_owned()];
    };
    let Some(selectors) = selectors.strip_suffix('}') else {
        return vec![strip_rust_alias(path).to_owned()];
    };

    split_rust_selectors(selectors)
        .into_iter()
        .filter_map(|selector| {
            let selector = selector.trim();
            let selector_without_alias = strip_rust_alias(selector);
            if selector_without_alias == "self" {
                return Some(prefix.to_owned());
            }
            if selector.contains("::{") {
                return Some(format!("{prefix}::{selector}"));
            }
            (!selector_without_alias.is_empty())
                .then(|| format!("{prefix}::{selector_without_alias}"))
        })
        .flat_map(|path| expand_rust_use_tree(&path))
        .collect()
}

fn strip_rust_alias(path: &str) -> &str {
    path.split_once(" as ")
        .map_or(path, |(path, _)| path)
        .trim()
}

fn split_rust_selectors(selectors: &str) -> Vec<&str> {
    let mut items = Vec::new();
    let mut brace_depth = 0usize;
    let mut start = 0usize;
    for (index, character) in selectors.char_indices() {
        match character {
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            ',' if brace_depth == 0 => {
                items.push(selectors[start..index].trim());
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    items.push(selectors[start..].trim());

    items
}

fn normalize_use_path(import_path: &str, path: &str) -> Option<String> {
    if path.starts_with("crate::") {
        return Some(path.trim_start_matches("crate::").to_owned());
    }
    if let Some(stripped) = path.strip_prefix("self::") {
        let joined = normalize_join(parent_dir(import_path), &stripped.replace("::", "/"))?;
        return Some(joined.replace('/', "::"));
    }
    if let Some(stripped) = path.strip_prefix("super::") {
        let joined = normalize_join(
            rust_super_base_dir(import_path),
            &stripped.replace("::", "/"),
        )?;
        return Some(joined.replace('/', "::"));
    }
    if rust_crate_root_file(import_path) {
        return Some(path.to_owned());
    }

    None
}

fn rust_crate_root_file(path: &str) -> bool {
    matches!(path, "src/lib.rs" | "src/main.rs" | "lib.rs" | "main.rs")
}

fn rust_super_base_dir(import_path: &str) -> &str {
    if import_path.ends_with("/mod.rs") {
        parent_dir(parent_dir(import_path))
    } else {
        parent_dir(import_path)
    }
}

fn rust_module_candidates(parent: &str, module_path: &str) -> Vec<String> {
    let base = if parent.is_empty() {
        module_path.to_owned()
    } else {
        match normalize_join(parent, module_path) {
            Some(value) => value,
            None => return Vec::new(),
        }
    };
    vec![format!("{base}.rs"), format!("{base}/mod.rs")]
}

use std::collections::BTreeMap;

use super::super::{super::symbols::SymbolKey, ImportResolution, module_paths, symbol_targets};

#[cfg(test)]
#[path = "java_tests.rs"]
mod tests;

pub(super) fn resolve(
    statement: &str,
    indexed_module_paths: &BTreeMap<String, Vec<String>>,
    symbols_by_name: &BTreeMap<String, Vec<SymbolKey>>,
) -> ImportResolution {
    match JavaImportRequest::parse(statement) {
        Some(JavaImportRequest::Class { class_path }) => {
            module_paths::resolve_file(&java_source_path(&class_path), true, indexed_module_paths)
        }
        Some(JavaImportRequest::PackageWildcard { package_path }) => {
            if directory_has_java_files(&package_path, indexed_module_paths) {
                ImportResolution::Resolved(package_path)
            } else {
                ImportResolution::Unresolved
            }
        }
        Some(JavaImportRequest::StaticMember { class_path, member }) => {
            symbol_targets::resolve_name_in_paths(
                &member,
                &[java_source_path(&class_path)],
                symbols_by_name,
            )
        }
        Some(JavaImportRequest::StaticWildcard { class_path }) => {
            module_paths::resolve_file(&java_source_path(&class_path), true, indexed_module_paths)
        }
        None => ImportResolution::Unresolved,
    }
}

#[derive(Debug, Eq, PartialEq)]
enum JavaImportRequest {
    Class { class_path: String },
    PackageWildcard { package_path: String },
    StaticMember { class_path: String, member: String },
    StaticWildcard { class_path: String },
}

impl JavaImportRequest {
    fn parse(statement: &str) -> Option<Self> {
        let body = statement
            .trim()
            .trim_end_matches(';')
            .trim()
            .strip_prefix("import ")?;
        let (is_static, body) = body
            .strip_prefix("static ")
            .map_or((false, body), |body| (true, body.trim()));
        if body.is_empty() {
            return None;
        }
        if let Some(prefix) = body.strip_suffix(".*") {
            let path = prefix.replace('.', "/");
            return if is_static {
                Some(Self::StaticWildcard { class_path: path })
            } else {
                Some(Self::PackageWildcard { package_path: path })
            };
        }

        let (parent, name) = body.rsplit_once('.')?;
        let parent_path = parent.replace('.', "/");
        let name = name.trim();
        if name.is_empty() {
            return None;
        }
        if is_static {
            Some(Self::StaticMember {
                class_path: parent_path,
                member: name.to_owned(),
            })
        } else {
            Some(Self::Class {
                class_path: format!("{parent_path}/{name}"),
            })
        }
    }
}

fn java_source_path(class_path: &str) -> String {
    format!("{class_path}.java")
}

fn directory_has_java_files(
    directory_path: &str,
    indexed_module_paths: &BTreeMap<String, Vec<String>>,
) -> bool {
    let directory = module_paths::normalize(directory_path);
    let prefix = if directory.is_empty() {
        String::new()
    } else {
        format!("{directory}/")
    };
    indexed_module_paths
        .range(prefix.clone()..)
        .take_while(|(path, _)| prefix.is_empty() || path.starts_with(&prefix))
        .any(|(path, _)| path.ends_with(".java"))
}

pub(super) fn imported_symbol_names(statement: &str) -> Vec<String> {
    match JavaImportRequest::parse(statement) {
        Some(JavaImportRequest::StaticMember { member, .. }) => vec![member],
        _ => Vec::new(),
    }
}

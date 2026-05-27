use crate::domain::CodeImportRecord;

use super::super::import_resolution::{ImportContext, ImportResolution, ModuleFileResolution};

pub(in crate::code::identity) fn resolve_import(
    import: &CodeImportRecord,
    context: &ImportContext<'_>,
) -> Option<(ImportResolution, Option<String>)> {
    let request = JavaImportRequest::parse(&import.module)?;

    Some(match request {
        JavaImportRequest::Class { class_path, name } => {
            let file_path = java_source_path(&class_path);
            match context.resolve_first_module_file(std::slice::from_ref(&file_path), true) {
                ModuleFileResolution::Resolved(target_hint) => {
                    (ImportResolution::Resolved, Some(target_hint))
                }
                ModuleFileResolution::Ambiguous => (ImportResolution::Ambiguous, None),
                ModuleFileResolution::Unresolved => {
                    (context.resolve_name_in_paths(&name, &[file_path]), None)
                }
            }
        }
        JavaImportRequest::PackageWildcard { package_path } => {
            if context.directory_has_language_files(&package_path, "java") {
                (ImportResolution::Resolved, Some(package_path))
            } else {
                (ImportResolution::Unresolved, None)
            }
        }
        JavaImportRequest::StaticMember { class_path, member } => {
            let file_path = java_source_path(&class_path);
            (context.resolve_name_in_paths(&member, &[file_path]), None)
        }
        JavaImportRequest::StaticWildcard { class_path } => {
            let file_path = java_source_path(&class_path);
            match context.resolve_first_module_file(&[file_path], true) {
                ModuleFileResolution::Resolved(target_hint) => {
                    (ImportResolution::Resolved, Some(target_hint))
                }
                ModuleFileResolution::Ambiguous => (ImportResolution::Ambiguous, None),
                ModuleFileResolution::Unresolved => (ImportResolution::Unresolved, None),
            }
        }
    })
}

enum JavaImportRequest {
    Class { class_path: String, name: String },
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
                name: name.to_owned(),
            })
        }
    }
}

fn java_source_path(class_path: &str) -> String {
    format!("{class_path}.java")
}

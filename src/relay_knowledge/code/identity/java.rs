use crate::domain::CodeImportRecord;

use super::imports::{ImportContext, ImportResolution};

pub(super) fn resolve_import(
    import: &CodeImportRecord,
    context: &ImportContext<'_>,
) -> Option<ImportResolution> {
    let request = JavaImportRequest::parse(&import.module)?;

    Some(match request {
        JavaImportRequest::Class { class_path, name } => {
            let file_path = java_source_path(&class_path);
            if context.module_file_exists_with_language(&file_path, "java") {
                ImportResolution::Resolved
            } else {
                context.resolve_name_in_paths(&name, &[file_path])
            }
        }
        JavaImportRequest::PackageWildcard { package_path } => {
            if context.directory_has_language_files(&package_path, "java") {
                ImportResolution::Resolved
            } else {
                ImportResolution::Unresolved
            }
        }
        JavaImportRequest::StaticMember { class_path, member } => {
            let file_path = java_source_path(&class_path);
            context.resolve_name_in_paths(&member, &[file_path])
        }
        JavaImportRequest::StaticWildcard { class_path } => {
            let file_path = java_source_path(&class_path);
            if context.module_file_exists_with_language(&file_path, "java") {
                ImportResolution::Resolved
            } else {
                ImportResolution::Unresolved
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

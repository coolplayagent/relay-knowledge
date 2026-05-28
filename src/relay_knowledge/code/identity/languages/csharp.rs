use crate::domain::CodeImportRecord;

use super::super::import_resolution::{ImportContext, ImportResolution};

const TYPE_LIKE_KINDS: &[&str] = &["class", "interface", "struct"];

pub(in crate::code::identity) fn resolve_import(
    import: &CodeImportRecord,
    context: &ImportContext<'_>,
) -> Option<ImportResolution> {
    let request = CSharpImportRequest::parse(&import.module)?;

    Some(match request {
        CSharpImportRequest::Namespace { namespace } => {
            if context.namespace_exists_for_language(&namespace, "csharp") {
                ImportResolution::Resolved
            } else {
                ImportResolution::Unresolved
            }
        }
        CSharpImportRequest::Type {
            namespace,
            name,
            is_alias,
            is_static,
        } => {
            let namespace_path = namespace.replace('.', "/");
            let full_namespace_path = csharp_join_path(&namespace_path, &name);
            if !is_alias
                && !is_static
                && context.directory_has_language_files(&full_namespace_path, "csharp")
            {
                return Some(ImportResolution::Resolved);
            }
            let source_paths = [format!("{}.cs", csharp_join_path(&namespace_path, &name))];
            let path_resolution = if is_alias || is_static {
                context.resolve_name_in_paths_for_language_and_kinds(
                    &name,
                    &source_paths,
                    "csharp",
                    TYPE_LIKE_KINDS,
                )
            } else {
                context.resolve_name_in_paths(&name, &source_paths)
            };
            match path_resolution {
                ImportResolution::Unresolved if is_static => context
                    .resolve_name_in_namespace_for_language_and_kinds(
                        &namespace,
                        &name,
                        "csharp",
                        TYPE_LIKE_KINDS,
                    ),
                ImportResolution::Unresolved if is_alias => {
                    let aliased_namespace = format!("{namespace}.{name}");
                    if context.namespace_exists_for_language(&aliased_namespace, "csharp") {
                        ImportResolution::Resolved
                    } else {
                        context.resolve_name_in_namespace_for_language_and_kinds(
                            &namespace,
                            &name,
                            "csharp",
                            TYPE_LIKE_KINDS,
                        )
                    }
                }
                ImportResolution::Unresolved => {
                    context.resolve_name_in_namespace_for_language(&namespace, &name, "csharp")
                }
                resolution => resolution,
            }
        }
    })
}

enum CSharpImportRequest {
    Namespace {
        namespace: String,
    },
    Type {
        namespace: String,
        name: String,
        is_alias: bool,
        is_static: bool,
    },
}

impl CSharpImportRequest {
    fn parse(statement: &str) -> Option<Self> {
        let body = statement
            .trim()
            .trim_end_matches(';')
            .trim()
            .strip_prefix("global using ")
            .or_else(|| {
                statement
                    .trim()
                    .trim_end_matches(';')
                    .trim()
                    .strip_prefix("using ")
            })?;
        let (is_alias, body) = body
            .split_once('=')
            .map_or((false, body), |(_, target)| (true, target.trim()));
        let (is_static, body) = body
            .strip_prefix("static ")
            .map_or((false, body), |body| (true, body.trim()));
        let body = body.strip_prefix("global::").unwrap_or(body).trim();
        if body.is_empty() {
            return None;
        }
        if is_alias || is_static {
            let (namespace, name) = body.rsplit_once('.').unwrap_or(("", body));
            return Some(Self::Type {
                namespace: namespace.to_owned(),
                name: name.to_owned(),
                is_alias,
                is_static,
            });
        }

        Some(Self::Namespace {
            namespace: body.to_owned(),
        })
    }
}

fn csharp_join_path(namespace_path: &str, name: &str) -> String {
    if namespace_path.is_empty() {
        name.to_owned()
    } else {
        format!("{namespace_path}/{name}")
    }
}

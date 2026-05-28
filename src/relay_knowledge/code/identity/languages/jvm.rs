use crate::domain::CodeImportRecord;

use super::super::import_resolution::{
    ImportContext, ImportResolution, ModuleFileResolution, combined_resolution,
};

pub(in crate::code::identity) fn resolve_import(
    import: &CodeImportRecord,
    context: &ImportContext<'_>,
    language_id: &str,
    extensions: &[&str],
) -> Option<(ImportResolution, Option<String>)> {
    let request = JvmImportRequest::parse(&import.module)?;

    Some(match request {
        JvmImportRequest::Classes { classes } => {
            combined_jvm_class_resolution(context, extensions, &classes)
        }
        JvmImportRequest::PackageWildcard { package_path } => {
            if context.package_declaration_conflicts_for_language(&package_path, language_id) {
                return Some((ImportResolution::Unresolved, None));
            }
            match context.resolve_directory_with_language_files(&package_path, language_id) {
                ModuleFileResolution::Resolved(target_hint) => {
                    (ImportResolution::Resolved, Some(target_hint))
                }
                ModuleFileResolution::Ambiguous => (ImportResolution::Ambiguous, None),
                ModuleFileResolution::Unresolved => (ImportResolution::Unresolved, None),
            }
        }
    })
}

enum JvmImportRequest {
    Classes { classes: Vec<JvmClassImport> },
    PackageWildcard { package_path: String },
}

struct JvmClassImport {
    class_path: String,
    name: String,
}

impl JvmImportRequest {
    fn parse(statement: &str) -> Option<Self> {
        let body = statement
            .trim()
            .trim_end_matches(';')
            .trim()
            .strip_prefix("import ")?;
        if body.is_empty() || body.starts_with("static ") {
            return None;
        }
        let body = strip_kotlin_alias(body);
        if let Some(prefix) = body.strip_suffix(".*").or_else(|| body.strip_suffix("._")) {
            return Some(Self::PackageWildcard {
                package_path: prefix.replace('.', "/"),
            });
        }
        let expressions = split_jvm_import_expressions(body);
        if expressions.len() > 1 {
            let classes = expressions
                .into_iter()
                .filter_map(parse_jvm_class_import)
                .collect::<Vec<_>>();
            return (!classes.is_empty()).then_some(Self::Classes { classes });
        }
        if let Some((parent, selectors)) = body.split_once(".{") {
            let selectors = selectors.strip_suffix('}')?;
            let classes = selectors
                .split(',')
                .filter_map(|selector| {
                    let name = scala_selector_name(selector)?;
                    Some(JvmClassImport {
                        class_path: format!("{}/{}", parent.replace('.', "/"), name),
                        name: name.to_owned(),
                    })
                })
                .collect::<Vec<_>>();
            return (!classes.is_empty()).then_some(Self::Classes { classes });
        }

        parse_jvm_class_import(body).map(|class_import| Self::Classes {
            classes: vec![class_import],
        })
    }
}

fn parse_jvm_class_import(expression: &str) -> Option<JvmClassImport> {
    let (parent, name) = expression.trim().rsplit_once('.')?;
    let name = name.trim();
    (!name.is_empty()).then(|| JvmClassImport {
        class_path: format!("{}/{}", parent.replace('.', "/"), name),
        name: name.to_owned(),
    })
}

fn combined_jvm_class_resolution(
    context: &ImportContext<'_>,
    extensions: &[&str],
    classes: &[JvmClassImport],
) -> (ImportResolution, Option<String>) {
    let mut target_hint = None;
    let resolution = combined_resolution(classes.iter().map(|class_import| {
        let file_paths = source_paths(&class_import.class_path, extensions);
        match context.resolve_first_module_file(&file_paths, true) {
            ModuleFileResolution::Resolved(path) => {
                let resolution =
                    context.resolve_name_in_paths(&class_import.name, std::slice::from_ref(&path));
                if classes.len() == 1 {
                    target_hint = Some(path);
                }
                resolution
            }
            ModuleFileResolution::Ambiguous => ImportResolution::Ambiguous,
            ModuleFileResolution::Unresolved => {
                let package = class_import
                    .class_path
                    .rsplit_once('/')
                    .map_or("", |(package, _)| package);
                match context.resolve_name_in_paths(&class_import.name, &file_paths) {
                    ImportResolution::Unresolved => {
                        let owner_resolution =
                            resolve_owner_member_import(context, extensions, class_import);
                        if owner_resolution != ImportResolution::Unresolved {
                            owner_resolution
                        } else {
                            context.resolve_name_in_directory(
                                &class_import.name,
                                package,
                                language_id_for_extensions(extensions),
                            )
                        }
                    }
                    resolution => resolution,
                }
            }
        }
    }));

    (resolution, target_hint)
}

fn resolve_owner_member_import(
    context: &ImportContext<'_>,
    extensions: &[&str],
    class_import: &JvmClassImport,
) -> ImportResolution {
    let Some((owner_path, _)) = class_import.class_path.rsplit_once('/') else {
        return ImportResolution::Unresolved;
    };
    context.resolve_name_in_paths(&class_import.name, &source_paths(owner_path, extensions))
}

fn language_id_for_extensions(extensions: &[&str]) -> &'static str {
    if extensions.contains(&"kt") {
        "kotlin"
    } else {
        "scala"
    }
}

fn strip_kotlin_alias(body: &str) -> &str {
    body.split_once(" as ")
        .map_or(body, |(import_path, _)| import_path.trim())
}

fn scala_selector_name(selector: &str) -> Option<&str> {
    let name = match selector.trim().split_once("=>") {
        Some((_, target)) if target.trim() == "_" => return None,
        Some((name, _)) => name.trim(),
        None => selector.trim(),
    };
    (!name.is_empty() && name != "_").then_some(name)
}

fn split_jvm_import_expressions(body: &str) -> Vec<&str> {
    let mut expressions = Vec::new();
    let mut brace_depth = 0usize;
    let mut start = 0usize;
    for (index, character) in body.char_indices() {
        match character {
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            ',' if brace_depth == 0 => {
                expressions.push(body[start..index].trim());
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    expressions.push(body[start..].trim());

    expressions
}

fn source_paths(class_path: &str, extensions: &[&str]) -> Vec<String> {
    extensions
        .iter()
        .map(|extension| format!("{class_path}.{extension}"))
        .collect()
}

use crate::domain::CodeImportRecord;

use super::super::import_resolution::{ImportContext, ImportResolution, combined_resolution};

pub(in crate::code::identity) fn resolve_import(
    import: &CodeImportRecord,
    context: &ImportContext<'_>,
) -> Option<ImportResolution> {
    let requests = PhpImportRequest::parse(&import.module)?;

    Some(combined_resolution(
        requests
            .iter()
            .map(|request| resolve_php_import(request, context)),
    ))
}

struct PhpImportRequest {
    qualified_name: String,
    namespace: String,
    name: String,
    import_kind: PhpImportKind,
}

impl PhpImportRequest {
    fn parse(statement: &str) -> Option<Vec<Self>> {
        let body = statement
            .trim()
            .trim_end_matches(';')
            .trim()
            .strip_prefix("use ")?;
        let (import_kind, body) = php_import_kind(body);
        let requests = split_php_use_clauses(body)
            .into_iter()
            .flat_map(|clause| parse_php_use_clause(clause, import_kind))
            .collect::<Vec<_>>();

        (!requests.is_empty()).then_some(requests)
    }
}

#[derive(Clone, Copy)]
enum PhpImportKind {
    Type,
    Function,
    Const,
}

impl PhpImportKind {
    fn allowed_symbol_kinds(self) -> &'static [&'static str] {
        match self {
            Self::Type => &["class", "interface"],
            Self::Function => &["function"],
            Self::Const => &["constant"],
        }
    }
}

fn php_import_kind(body: &str) -> (PhpImportKind, &str) {
    let body = body.trim();
    php_import_kind_with_default(body, PhpImportKind::Type)
}

fn php_import_kind_with_default(body: &str, default: PhpImportKind) -> (PhpImportKind, &str) {
    let body = body.trim();
    if let Some(body) = strip_php_keyword_prefix(body, "function") {
        return (PhpImportKind::Function, body.trim());
    }
    if let Some(body) = strip_php_keyword_prefix(body, "const") {
        return (PhpImportKind::Const, body.trim());
    }

    (default, body)
}

fn strip_php_keyword_prefix<'a>(body: &'a str, keyword: &str) -> Option<&'a str> {
    let (head, rest) = body.split_once(char::is_whitespace)?;
    head.eq_ignore_ascii_case(keyword).then_some(rest)
}

fn split_php_use_clauses(body: &str) -> Vec<&str> {
    let mut clauses = Vec::new();
    let mut brace_depth = 0usize;
    let mut start = 0usize;
    for (index, character) in body.char_indices() {
        match character {
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            ',' if brace_depth == 0 => {
                clauses.push(body[start..index].trim());
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    clauses.push(body[start..].trim());

    clauses
}

fn parse_php_use_clause(clause: &str, import_kind: PhpImportKind) -> Vec<PhpImportRequest> {
    let clause = clause.trim().trim_start_matches('\\');
    if let Some((namespace, names)) = grouped_use_parts(clause) {
        return names
            .split(',')
            .filter_map(|name| {
                let (item_kind, name) = php_import_kind_with_default(name, import_kind);
                php_request_from_qualified_name(
                    &qualified_php_name(namespace, strip_php_alias(name)),
                    item_kind,
                )
            })
            .collect();
    }

    let clause = strip_php_alias(clause);
    php_request_from_qualified_name(clause, import_kind)
        .into_iter()
        .collect()
}

fn php_request_from_qualified_name(
    qualified_name: &str,
    import_kind: PhpImportKind,
) -> Option<PhpImportRequest> {
    let qualified_name = qualified_name.trim().trim_start_matches('\\');
    let (namespace, name) = qualified_name
        .rsplit_once('\\')
        .unwrap_or(("", qualified_name));
    php_request(namespace, name, import_kind)
}

fn php_request(
    namespace: &str,
    name: &str,
    import_kind: PhpImportKind,
) -> Option<PhpImportRequest> {
    let name = strip_php_alias(name).trim();
    (!name.is_empty()).then(|| PhpImportRequest {
        qualified_name: qualified_php_name(namespace, name),
        namespace: namespace.to_owned(),
        name: name.to_owned(),
        import_kind,
    })
}

fn qualified_php_name(namespace: &str, name: &str) -> String {
    if namespace.is_empty() {
        name.to_owned()
    } else {
        format!("{namespace}\\{name}")
    }
}

fn strip_php_alias(value: &str) -> &str {
    value
        .to_ascii_lowercase()
        .find(" as ")
        .map_or(value, |index| &value[..index])
        .trim()
}

fn resolve_php_import(request: &PhpImportRequest, context: &ImportContext<'_>) -> ImportResolution {
    let module_paths = php_source_paths(&request.qualified_name);
    let allowed_kinds = request.import_kind.allowed_symbol_kinds();
    let path_resolution = context.resolve_name_in_paths_for_language_and_kinds(
        &request.name,
        &module_paths,
        "php",
        allowed_kinds,
    );
    if path_resolution != ImportResolution::Unresolved {
        return path_resolution;
    }

    let namespace_resolution = context.resolve_name_in_namespace_for_language_and_kinds(
        &request.namespace.replace('\\', "."),
        &request.name,
        "php",
        allowed_kinds,
    );
    if namespace_resolution != ImportResolution::Unresolved {
        return namespace_resolution;
    }

    context.resolve_name_in_directory_for_language_and_kinds(
        &request.name,
        &request.namespace.replace('\\', "/"),
        "php",
        allowed_kinds,
    )
}

fn grouped_use_parts(body: &str) -> Option<(&str, &str)> {
    let (namespace, names) = body.split_once("\\{")?;
    Some((namespace.trim_start_matches('\\'), names.strip_suffix('}')?))
}

fn php_source_paths(qualified_name: &str) -> Vec<String> {
    let path = qualified_name.replace('\\', "/");
    vec![format!("{path}.php"), format!("{path}.phtml")]
}

use crate::domain::CodeImportRecord;

use super::super::import_resolution::{ImportContext, ImportResolution, ModuleFileResolution};

pub(in crate::code::identity) fn resolve_import(
    import: &CodeImportRecord,
    context: &ImportContext<'_>,
) -> Option<(ImportResolution, Option<String>)> {
    let request = SwiftImportRequest::parse(&import.module)?;
    if let Some(name) = request.name {
        let resolution = if let Some(import_kind) = request.import_kind {
            context.resolve_name_in_directory_tree_for_language_and_kinds(
                name,
                request.module,
                "swift",
                import_kind.allowed_symbol_kinds(),
            )
        } else {
            context.resolve_name_in_directory_tree(name, request.module, "swift")
        };
        return Some((resolution, Some(request.module.to_owned())));
    }
    match context.resolve_directory_tree_with_language_files(request.module, "swift") {
        ModuleFileResolution::Resolved(target_hint) => {
            Some((ImportResolution::Resolved, Some(target_hint)))
        }
        ModuleFileResolution::Ambiguous => Some((ImportResolution::Ambiguous, None)),
        ModuleFileResolution::Unresolved => Some((ImportResolution::Unresolved, None)),
    }
}

struct SwiftImportRequest<'a> {
    module: &'a str,
    name: Option<&'a str>,
    import_kind: Option<SwiftImportKind>,
}

impl<'a> SwiftImportRequest<'a> {
    fn parse(statement: &'a str) -> Option<Self> {
        let statement = statement.trim().trim_end_matches(';').trim();
        let statement = strip_swift_import_attributes(statement);
        let statement = statement.strip_prefix("import ")?;
        let mut parts = statement.split_whitespace();
        let first = parts.next()?;
        let (import_kind, target) = if let Some(import_kind) = SwiftImportKind::parse(first) {
            (Some(import_kind), parts.next()?)
        } else {
            (None, first)
        };
        let (module, name) = target
            .split_once('.')
            .map_or((target, None), |(module, name)| (module, Some(name)));

        Some(Self {
            module,
            name,
            import_kind,
        })
    }
}

#[derive(Clone, Copy)]
enum SwiftImportKind {
    Type,
    Interface,
    Function,
    Value,
}

impl SwiftImportKind {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "class" | "enum" | "struct" => Some(Self::Type),
            "protocol" => Some(Self::Interface),
            "func" => Some(Self::Function),
            "typealias" => Some(Self::Type),
            "var" | "let" => Some(Self::Value),
            "extension" => Some(Self::Type),
            _ => None,
        }
    }

    fn allowed_symbol_kinds(self) -> &'static [&'static str] {
        match self {
            Self::Type => &["class", "type"],
            Self::Interface => &["interface"],
            Self::Function => &["function"],
            Self::Value => &["constant", "variable"],
        }
    }
}

fn strip_swift_import_attributes(mut statement: &str) -> &str {
    while let Some(stripped) = statement.strip_prefix('@') {
        let Some((_, rest)) = stripped.split_once(char::is_whitespace) else {
            return statement;
        };
        statement = rest.trim_start();
    }

    statement
}

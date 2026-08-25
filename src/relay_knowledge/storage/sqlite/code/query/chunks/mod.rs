use crate::domain::{CodeQueryKind, CodeRetrievalHit, CodeRetrievalRequest};

use super::{
    filtered_hits_for_gate, references::reference_usage_context_bonus,
    relevance::SymbolIdentityQuery,
};

mod search;
pub(super) use search::search_chunks;

const REFERENCE_NON_CODE_LANGUAGE_IDS: &[&str] = &[
    "gomod",
    "ini",
    "json",
    "markdown",
    "properties",
    "toml",
    "xml",
    "yaml",
];

pub(super) fn definition_query_needs_chunk_fallback(
    request: &CodeRetrievalRequest,
    hits: &[CodeRetrievalHit],
) -> bool {
    if request.code_query_kind != CodeQueryKind::Definition {
        return false;
    }
    let Some(identity) = SymbolIdentityQuery::from_query(&request.query) else {
        return hits.is_empty();
    };

    !hits.iter().any(|hit| {
        hit.canonical_symbol_id
            .as_deref()
            .is_some_and(|symbol_id| canonical_symbol_leaf_matches(symbol_id, identity.leaf_name()))
    })
}

pub(super) fn references_query_needs_chunk_fallback(
    request: &CodeRetrievalRequest,
    hits: &[CodeRetrievalHit],
) -> bool {
    request.code_query_kind == CodeQueryKind::References
        && filtered_hits_for_gate(hits, request).len() < request.limit.max(1)
        && SymbolIdentityQuery::from_query(&request.query).is_some()
}

pub(super) fn canonical_symbol_leaf_matches(canonical_symbol_id: &str, leaf_name: &str) -> bool {
    canonical_symbol_id
        .rsplit(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .find(|part| !part.is_empty())
        .is_some_and(|part| part == leaf_name)
}

pub(super) fn exact_reference_chunk_bonus(
    request: &CodeRetrievalRequest,
    base_score: f64,
    content: &str,
) -> f64 {
    if request.code_query_kind != CodeQueryKind::References {
        return 0.0;
    }
    let Some(identity) = SymbolIdentityQuery::from_query(&request.query) else {
        return 0.0;
    };

    reference_usage_context_bonus(
        base_score,
        "value",
        identity.leaf_name(),
        Some(content),
        request,
    )
}

pub(super) fn exact_reference_chunk_contains_usage(
    request: &CodeRetrievalRequest,
    language_id: &str,
    content: &str,
) -> bool {
    if request.code_query_kind != CodeQueryKind::References {
        return true;
    }
    let Some(identity) = SymbolIdentityQuery::from_query(&request.query) else {
        return true;
    };
    if REFERENCE_NON_CODE_LANGUAGE_IDS.contains(&language_id) {
        return false;
    }

    let code = super::identifiers::code_outside_comments_and_literals(language_id, content);
    if matches!(language_id, "c" | "cpp") {
        c_family_chunk_contains_usage(language_id, &code, identity.leaf_name())
    } else {
        code.lines().any(|line| {
            line_contains_identifier(line, identity.leaf_name())
                && !line_declares_identity(language_id, line, identity.leaf_name())
        })
    }
}

fn reference_usage_language_filter_sql(request: &CodeRetrievalRequest) -> String {
    if request.code_query_kind != CodeQueryKind::References {
        return String::new();
    }
    let languages = REFERENCE_NON_CODE_LANGUAGE_IDS
        .iter()
        .map(|language_id| format!("'{language_id}'"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("code_repository_search.language_id NOT IN ({languages})")
}

pub(super) fn exact_definition_chunk_bonus(request: &CodeRetrievalRequest, content: &str) -> f64 {
    if request.code_query_kind != CodeQueryKind::Definition {
        return 0.0;
    }
    let Some(identity) = SymbolIdentityQuery::from_query(&request.query) else {
        return 0.0;
    };

    if content
        .lines()
        .map(str::trim)
        .any(|line| declaration_line_defines_identity(line, identity.leaf_name()))
    {
        3.0
    } else {
        0.0
    }
}

fn declaration_line_defines_identity(line: &str, leaf_name: &str) -> bool {
    if !line_contains_identifier(line, leaf_name) {
        return false;
    }
    if line.starts_with("typedef ") || line.contains(" typedef ") {
        return true;
    }
    if line
        .strip_prefix("using ")
        .is_some_and(|remainder| line_starts_with_identifier(remainder, leaf_name))
    {
        return true;
    }

    ["struct ", "class ", "enum ", "union "]
        .into_iter()
        .filter_map(|prefix| line.strip_prefix(prefix))
        .any(|remainder| line_starts_with_identifier(remainder, leaf_name))
}

fn c_family_chunk_contains_usage(language_id: &str, code: &str, leaf_name: &str) -> bool {
    let mut logical_directive = String::new();
    for line in code.lines() {
        if !logical_directive.is_empty() || line.trim_start().starts_with('#') {
            append_preprocessor_line(&mut logical_directive, line);
            if line.trim_end().ends_with('\\') {
                continue;
            }
            if preprocessor_directive_contains_usage(&logical_directive, leaf_name) {
                return true;
            }
            logical_directive.clear();
            continue;
        }
        if line_contains_identifier(line, leaf_name)
            && !line_declares_identity(language_id, line, leaf_name)
        {
            return true;
        }
    }

    !logical_directive.is_empty()
        && preprocessor_directive_contains_usage(&logical_directive, leaf_name)
}

fn append_preprocessor_line(logical_directive: &mut String, line: &str) {
    let line = line.trim_end();
    let line = line.strip_suffix('\\').unwrap_or(line);
    if !logical_directive.is_empty() {
        logical_directive.push(' ');
    }
    logical_directive.push_str(line);
}

fn preprocessor_directive_contains_usage(directive: &str, leaf_name: &str) -> bool {
    let Some(directive) = directive.trim_start().strip_prefix('#') else {
        return false;
    };
    let directive = directive.trim_start();
    let Some(remainder) = directive.strip_prefix("define") else {
        return line_contains_identifier(directive, leaf_name);
    };
    if remainder.chars().next().is_some_and(is_identifier_char) {
        return line_contains_identifier(directive, leaf_name);
    }

    let remainder = remainder.trim_start();
    let macro_name_len = remainder
        .char_indices()
        .find_map(|(index, character)| (!is_identifier_char(character)).then_some(index))
        .unwrap_or(remainder.len());
    let (macro_name, body) = remainder.split_at(macro_name_len);
    if macro_name.is_empty() || macro_name == leaf_name {
        return false;
    }
    if macro_parameter_defines_identity(body, leaf_name) {
        return false;
    }

    line_contains_identifier(body, leaf_name)
}

fn macro_parameter_defines_identity(body: &str, leaf_name: &str) -> bool {
    let Some(parameters) = body.strip_prefix('(') else {
        return false;
    };
    let Some(end) = parameters.find(')') else {
        return false;
    };
    parameters[..end]
        .split(',')
        .map(str::trim)
        .any(|parameter| parameter == leaf_name)
}

fn line_declares_identity(language_id: &str, line: &str, leaf_name: &str) -> bool {
    if matches!(language_id, "c" | "cpp") && c_family_typedef_declares_identity(line, leaf_name) {
        return true;
    }
    let occurrence_count = identifier_occurrence_count(line, leaf_name);
    if occurrence_count != 1 {
        return false;
    }
    let Some((start, end)) = identifier_range(line, leaf_name) else {
        return false;
    };
    let prefix = line[..start].trim_end();
    let suffix = line[end..].trim_start();
    let preceding_keyword = prefix
        .rsplit(|character: char| !is_identifier_char(character))
        .find(|token| !token.is_empty());

    if preceding_keyword.is_some_and(|keyword| {
        matches!(
            keyword,
            "class"
                | "enum"
                | "interface"
                | "module"
                | "namespace"
                | "struct"
                | "trait"
                | "type"
                | "union"
        ) && named_declaration_suffix(suffix)
    }) {
        return true;
    }
    if prefix
        .split(|character: char| !is_identifier_char(character))
        .any(|token| token == "typedef")
    {
        return true;
    }

    match language_id {
        "c" | "cpp" => c_family_line_declares_identity(prefix, suffix),
        "javascript" | "jsx" | "typescript" | "tsx" => {
            matches!(
                preceding_keyword,
                Some("const" | "function" | "let" | "var")
            )
        }
        "python" | "ruby" => matches!(preceding_keyword, Some("class" | "def" | "module")),
        "rust" => matches!(
            preceding_keyword,
            Some("const" | "fn" | "macro_rules" | "mod" | "static")
        ),
        "bash" => {
            preceding_keyword == Some("function")
                || (prefix.is_empty() && suffix.trim_start().starts_with("()"))
        }
        _ => false,
    }
}

fn named_declaration_suffix(suffix: &str) -> bool {
    suffix.is_empty()
        || suffix == ";"
        || suffix.starts_with('{')
        || suffix.starts_with(':')
        || suffix.starts_with("extends ")
        || suffix.starts_with("final ")
        || suffix.starts_with("implements ")
        || suffix.starts_with("where ")
}

fn c_family_line_declares_identity(prefix: &str, suffix: &str) -> bool {
    if prefix.is_empty()
        || prefix
            .rsplit(|character: char| !is_identifier_char(character))
            .find(|token| !token.is_empty())
            .is_some_and(|token| {
                matches!(
                    token,
                    "case" | "co_return" | "goto" | "return" | "sizeof" | "throw"
                )
            })
    {
        return false;
    }
    let declarator_suffix = suffix.starts_with(';')
        || suffix.starts_with('=')
        || suffix.starts_with('[')
        || suffix.starts_with(',')
        || c_family_function_declarator_suffix(suffix);
    declarator_suffix
        && !prefix.chars().any(|character| {
            matches!(
                character,
                '=' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | '.' | '?' | '+' | '/' | '%'
            )
        })
        && !prefix.contains("->")
        && !prefix.ends_with("::")
}

fn c_family_function_declarator_suffix(suffix: &str) -> bool {
    let Some(parameters) = suffix.strip_prefix('(') else {
        return false;
    };
    let mut depth = 1usize;
    let mut close = None;
    for (index, character) in parameters.char_indices() {
        match character {
            '(' => depth = depth.saturating_add(1),
            ')' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(index + character.len_utf8());
                    break;
                }
            }
            _ => {}
        }
    }
    let Some(close) = close else {
        return false;
    };
    let remainder = parameters[close..].trim_start();

    remainder.starts_with(';') || remainder.contains('{')
}

fn c_family_typedef_declares_identity(line: &str, leaf_name: &str) -> bool {
    let trimmed = line.trim();
    if !trimmed.ends_with(';')
        || !trimmed
            .split(|character: char| !is_identifier_char(character))
            .any(|token| token == "typedef")
    {
        return false;
    }
    trimmed
        .trim_end_matches(';')
        .split(|character: char| !is_identifier_char(character))
        .rfind(|token| !token.is_empty())
        == Some(leaf_name)
}

fn identifier_occurrence_count(line: &str, identifier: &str) -> usize {
    line.match_indices(identifier)
        .filter(|(start, _)| identifier_at(line, *start, identifier))
        .count()
}

fn identifier_range(line: &str, identifier: &str) -> Option<(usize, usize)> {
    line.match_indices(identifier)
        .find(|(start, _)| identifier_at(line, *start, identifier))
        .map(|(start, _)| (start, start + identifier.len()))
}

fn identifier_at(line: &str, start: usize, identifier: &str) -> bool {
    let end = start + identifier.len();
    line.get(..start).is_some_and(|prefix| {
        prefix
            .chars()
            .next_back()
            .is_none_or(|character| !is_identifier_char(character))
    }) && line
        .get(end..)
        .is_some_and(|suffix| suffix.chars().next().is_none_or(|c| !is_identifier_char(c)))
}

fn line_starts_with_identifier(line: &str, identifier: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with(identifier)
        && trimmed
            .get(identifier.len()..)
            .is_some_and(|suffix| suffix.chars().next().is_none_or(|c| !is_identifier_char(c)))
}

fn line_contains_identifier(line: &str, identifier: &str) -> bool {
    line.match_indices(identifier).any(|(start, _)| {
        let end = start + identifier.len();
        line.get(..start).is_some_and(|prefix| {
            prefix
                .chars()
                .next_back()
                .is_none_or(|c| !is_identifier_char(c))
        }) && line
            .get(end..)
            .is_some_and(|suffix| suffix.chars().next().is_none_or(|c| !is_identifier_char(c)))
    })
}

fn is_identifier_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;

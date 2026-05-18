use crate::domain::CodeQueryKind;

use super::code_query_identifiers::identifier_terms_equivalent;

pub(super) fn import_line_priority(base_score: f64, line_start: u32, query: &str) -> f64 {
    if base_score <= 0.0 || !query_looks_like_import_path(query) {
        return 0.0;
    }

    1.0 / f64::from(line_start.clamp(1, 1_000))
}

pub(super) fn import_surface_bonus(base_score: f64, path: &str) -> f64 {
    if base_score <= 0.0 {
        return 0.0;
    }
    if path
        .split('/')
        .any(|segment| matches!(segment, "test" | "tests" | "__tests__"))
    {
        return 0.0;
    }
    match path.rsplit('/').next().unwrap_or(path) {
        "__init__.py" | "mod.rs" | "lib.rs" | "index.js" | "index.jsx" | "index.ts"
        | "index.tsx" => 0.2,
        _ => 0.0,
    }
}

pub(super) fn import_target_symbol_bonus(query: &str, matched_symbol_name: Option<&str>) -> f64 {
    let Some(matched_symbol_name) = matched_symbol_name else {
        return 0.0;
    };
    let terms = query_terms(query);
    let Some(term) = terms.last() else {
        return 0.0;
    };
    if term.len() >= 3
        && matched_symbol_name
            .split_whitespace()
            .any(|name| name.eq_ignore_ascii_case(term))
    {
        2.0
    } else {
        0.0
    }
}

pub(super) fn hybrid_import_sparse_query_penalty(
    base_score: f64,
    query: &str,
    path: &str,
    module: &str,
    target_hint: Option<&str>,
    _matched_symbol_name: Option<&str>,
    kind: CodeQueryKind,
) -> f64 {
    if base_score <= 0.0 || kind != CodeQueryKind::Hybrid || query_looks_like_import_path(query) {
        return 0.0;
    }
    let terms = normalized_query_terms(query);
    if terms.len() < MIN_HYBRID_SPARSE_IMPORT_QUERY_TERMS {
        return 0.0;
    }

    let fields = [path, module, target_hint.unwrap_or_default()];
    let matched_terms = terms
        .iter()
        .filter(|term| {
            fields
                .iter()
                .any(|field| import_field_matches_query_term(field, term))
        })
        .count();
    let required_terms = terms.len().div_ceil(2);
    if matched_terms >= required_terms {
        return 0.0;
    }

    let missing_required_terms = required_terms - matched_terms;
    let penalty = (missing_required_terms as f64 * HYBRID_SPARSE_IMPORT_PENALTY_PER_TERM)
        .min(MAX_HYBRID_SPARSE_IMPORT_PENALTY)
        .min((base_score - MIN_SPARSE_IMPORT_BASE_SCORE).max(0.0));
    -penalty
}

const MIN_HYBRID_SPARSE_IMPORT_QUERY_TERMS: usize = 6;
const HYBRID_SPARSE_IMPORT_PENALTY_PER_TERM: f64 = 4.0;
const MAX_HYBRID_SPARSE_IMPORT_PENALTY: f64 = 16.0;
const MIN_SPARSE_IMPORT_BASE_SCORE: f64 = 0.5;
const MIN_IMPORT_COVERAGE_TERM_LEN: usize = 3;

fn normalized_query_terms(query: &str) -> Vec<String> {
    let mut terms = query_terms(query)
        .into_iter()
        .filter(|term| term.len() >= MIN_IMPORT_COVERAGE_TERM_LEN)
        .map(|term| term.to_ascii_lowercase())
        .collect::<Vec<_>>();
    terms.sort();
    terms.dedup();

    terms
}

fn import_field_matches_query_term(field: &str, term: &str) -> bool {
    let lower = field.to_ascii_lowercase();
    if lower.contains(term) {
        return true;
    }

    identifier_tokens(field).any(|candidate| {
        identifier_terms_equivalent(candidate, term)
            || candidate
                .split('_')
                .filter(|part| !part.is_empty())
                .any(|part| identifier_terms_equivalent(part, term))
            || camel_case_terms(candidate)
                .iter()
                .any(|part| identifier_terms_equivalent(part, term))
    })
}

fn query_looks_like_import_path(query: &str) -> bool {
    let trimmed = query.trim();
    trimmed.contains('/') || trimmed.contains('\\') || query_contains_file_extension(trimmed)
}

fn query_contains_file_extension(query: &str) -> bool {
    query.split_whitespace().any(|term| {
        let term = term.trim_matches(|character: char| {
            !(character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.'))
        });
        let Some((stem, extension)) = term.rsplit_once('.') else {
            return false;
        };
        !stem.is_empty() && file_extension_is_path_like(extension)
    })
}

fn file_extension_is_path_like(extension: &str) -> bool {
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "c" | "cc"
            | "cpp"
            | "cs"
            | "go"
            | "gradle"
            | "h"
            | "hh"
            | "hpp"
            | "hxx"
            | "java"
            | "js"
            | "json"
            | "jsx"
            | "kt"
            | "md"
            | "php"
            | "py"
            | "rb"
            | "rs"
            | "scala"
            | "sh"
            | "swift"
            | "ts"
            | "tsx"
            | "txt"
            | "xml"
            | "yaml"
            | "yml"
    )
}

fn query_terms(query: &str) -> Vec<String> {
    query
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|term| !term.is_empty())
        .map(str::to_owned)
        .collect()
}

fn identifier_tokens(value: &str) -> impl Iterator<Item = &str> {
    value
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|token| !token.is_empty())
}

fn camel_case_terms(token: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut start = 0;
    let mut previous: Option<char> = None;
    let chars = token.char_indices().collect::<Vec<_>>();
    for (index, (byte_index, character)) in chars.iter().enumerate() {
        let next = chars.get(index + 1).map(|(_, next)| *next);
        let starts_upper_word = character.is_ascii_uppercase()
            && previous.is_some_and(|previous| {
                previous.is_ascii_lowercase()
                    || previous.is_ascii_digit()
                    || next.is_some_and(|next| next.is_ascii_lowercase())
            });
        if *byte_index > start && starts_upper_word {
            terms.push(token[start..*byte_index].to_ascii_lowercase());
            start = *byte_index;
        }
        previous = Some(*character);
    }
    if start < token.len() {
        terms.push(token[start..].to_ascii_lowercase());
    }

    terms
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_line_priority_only_applies_to_path_like_queries() {
        assert_eq!(import_line_priority(3.0, 1, "ProviderShared"), 0.0);
        assert_eq!(
            import_line_priority(3.0, 1, "org.springframework.util.ObjectUtils"),
            0.0
        );
        assert!(import_line_priority(3.0, 10, "linux/debugfs.h") > 0.0);
        assert!(import_line_priority(3.0, 10, "./redaction") > 0.0);
        assert!(import_line_priority(3.0, 10, "shared.ts") > 0.0);
        assert_eq!(import_line_priority(0.0, 1, "linux/debugfs.h"), 0.0);
    }

    #[test]
    fn hybrid_sparse_import_penalty_only_applies_to_long_concept_queries() {
        assert_eq!(
            hybrid_import_sparse_query_penalty(
                12.0,
                "linux/debugfs.h",
                "mm/cma_debug.c",
                "#include <linux/debugfs.h>",
                Some("include/linux/debugfs.h"),
                None,
                CodeQueryKind::Hybrid,
            ),
            0.0
        );
        assert_eq!(
            hybrid_import_sparse_query_penalty(
                12.0,
                "OpenAI Chat protocol SSE tool calls",
                "packages/llm/src/providers/openai.ts",
                "import * as OpenAIChat from \"../protocols/openai-chat\"",
                Some("packages/llm/src/protocols/openai-chat.ts"),
                None,
                CodeQueryKind::Imports,
            ),
            0.0
        );
        assert_eq!(
            hybrid_import_sparse_query_penalty(
                12.0,
                "OpenAI Chat protocol SSE tool",
                "packages/llm/src/providers/openai.ts",
                "import * as OpenAIChat from \"../protocols/openai-chat\"",
                Some("packages/llm/src/protocols/openai-chat.ts"),
                None,
                CodeQueryKind::Hybrid,
            ),
            0.0
        );
    }

    #[test]
    fn hybrid_sparse_import_penalty_demotes_low_coverage_import_edges() {
        let penalty = hybrid_import_sparse_query_penalty(
            18.0,
            "OpenAI Chat protocol SSE tool calls lifecycle finish events route transport",
            "packages/llm/src/providers/openai.ts",
            "import * as OpenAIChat from \"../protocols/openai-chat\"",
            Some("packages/llm/src/protocols/openai-chat.ts"),
            None,
            CodeQueryKind::Hybrid,
        );

        assert!(penalty <= -11.0, "penalty was {penalty}");
    }

    #[test]
    fn hybrid_sparse_import_penalty_preserves_imports_covering_query_terms() {
        let penalty = hybrid_import_sparse_query_penalty(
            18.0,
            "OpenAI Chat protocol SSE tool calls lifecycle finish events route transport",
            "packages/llm/src/route/transport/openai-chat.ts",
            "import { ToolStream, Lifecycle, finishEvents, route, transport } from \"../protocols/openai-chat\"",
            Some("packages/llm/src/protocols/openai-chat.ts"),
            Some("ToolStream Lifecycle finishEvents OpenAIChatProtocol"),
            CodeQueryKind::Hybrid,
        );

        assert_eq!(penalty, 0.0);
    }
}

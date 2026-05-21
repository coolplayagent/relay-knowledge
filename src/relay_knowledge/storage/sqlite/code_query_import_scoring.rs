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

pub(super) fn import_public_dependency_surface_bonus(
    base_score: f64,
    query: &str,
    path: &str,
    target_hint: Option<&str>,
    kind: CodeQueryKind,
) -> f64 {
    if base_score <= 0.0 || kind != CodeQueryKind::Imports || !query_looks_like_import_path(query) {
        return 0.0;
    }
    let target_is_header =
        target_hint.is_some_and(path_has_header_extension) || path_has_header_extension(query);
    if !target_is_header
        || !path_has_header_extension(path)
        || path_looks_like_test_or_benchmark(path)
    {
        return 0.0;
    }

    0.85
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

pub(super) fn import_same_file_usage_bonus(
    base_score: f64,
    usage_count: usize,
    kind: CodeQueryKind,
) -> f64 {
    if base_score <= 0.0 || kind != CodeQueryKind::Imports || usage_count <= 1 {
        return 0.0;
    }

    ((usage_count - 1) as f64 * IMPORT_USAGE_BONUS_PER_REFERENCE).min(MAX_IMPORT_USAGE_BONUS)
}

pub(super) fn import_target_directory_bonus(
    base_score: f64,
    query: &str,
    path: &str,
    target_hint: Option<&str>,
    kind: CodeQueryKind,
) -> f64 {
    if base_score <= 0.0 || kind != CodeQueryKind::Imports || query_looks_like_import_path(query) {
        return 0.0;
    }
    let Some(target_parent) = target_hint.and_then(parent_dir) else {
        return 0.0;
    };
    if parent_dir(path).is_some_and(|parent| parent == target_parent)
        && path != target_hint.unwrap_or_default()
    {
        0.4
    } else {
        0.0
    }
}

pub(super) fn import_binding_context_bonus(
    base_score: f64,
    query: &str,
    module: &str,
    kind: CodeQueryKind,
) -> f64 {
    if base_score <= 0.0 || kind != CodeQueryKind::Imports || query_looks_like_import_path(query) {
        return 0.0;
    }
    let Some(binding_count) = named_import_binding_count_for_query(module, query) else {
        return 0.0;
    };
    if binding_count <= 1 {
        return 0.0;
    }

    ((binding_count - 1) as f64 * IMPORT_BINDING_CONTEXT_BONUS_PER_BINDING)
        .min(MAX_IMPORT_BINDING_CONTEXT_BONUS)
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
const IMPORT_USAGE_BONUS_PER_REFERENCE: f64 = 0.05;
const MAX_IMPORT_USAGE_BONUS: f64 = 0.4;
const IMPORT_BINDING_CONTEXT_BONUS_PER_BINDING: f64 = 0.25;
const MAX_IMPORT_BINDING_CONTEXT_BONUS: f64 = 1.0;

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

pub(super) fn query_looks_like_import_path(query: &str) -> bool {
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

fn parent_dir(path: &str) -> Option<&str> {
    path.rsplit_once('/')
        .map(|(parent, _)| parent)
        .filter(|parent| !parent.is_empty())
}

fn path_has_header_extension(path: &str) -> bool {
    path.rsplit('/')
        .next()
        .and_then(|file_name| file_name.rsplit_once('.').map(|(_, extension)| extension))
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "h" | "hh" | "hpp" | "hxx" | "inc" | "ipp"
            )
        })
}

fn path_looks_like_test_or_benchmark(path: &str) -> bool {
    path.to_ascii_lowercase().split('/').any(|segment| {
        matches!(
            segment,
            "test" | "tests" | "__tests__" | "testing" | "bench" | "benchmark" | "benchmarks"
        ) || segment.ends_with("_test")
            || segment.ends_with(".test.ts")
            || segment.ends_with(".test.tsx")
            || segment.ends_with(".spec.ts")
            || segment.ends_with(".spec.tsx")
    })
}

fn named_import_binding_count_for_query(module: &str, query: &str) -> Option<usize> {
    let (start, end) = named_import_bounds(module)?;
    let query_terms = query_terms(query);
    let mut binding_count = 0;
    let mut query_is_bound = false;
    for binding in module[start + 1..end].split(',') {
        let binding = binding.trim().trim_start_matches("type ").trim();
        if binding.is_empty() {
            continue;
        }
        binding_count += 1;
        if import_binding_matches_query(binding, &query_terms) {
            query_is_bound = true;
        }
    }

    query_is_bound.then_some(binding_count)
}

fn named_import_bounds(module: &str) -> Option<(usize, usize)> {
    let start = module.find('{')?;
    let end = module[start + 1..].find('}')? + start + 1;
    (end > start).then_some((start, end))
}

fn import_binding_matches_query(binding: &str, query_terms: &[String]) -> bool {
    let binding_name = binding
        .split_whitespace()
        .last()
        .unwrap_or(binding)
        .trim_matches(|character: char| !(character.is_ascii_alphanumeric() || character == '_'));
    query_terms
        .iter()
        .any(|term| identifier_terms_equivalent(binding_name, term))
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

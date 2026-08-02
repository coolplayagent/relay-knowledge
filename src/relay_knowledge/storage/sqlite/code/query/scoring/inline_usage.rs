use crate::domain::{CodeQueryKind, CodeRetrievalRequest};

use super::super::hybrid::planning::workflow_language_scope_matches;
use super::path_ranking::{path_looks_like_test_or_benchmark, query_mentions_test_or_benchmark};

pub(in super::super) fn language_scoped_inline_usage_chunk_bonus(
    base_score: f64,
    query: &str,
    content: &str,
    path: &str,
    language_id: &str,
    request: &CodeRetrievalRequest,
) -> f64 {
    if base_score <= 0.0
        || request.code_query_kind != CodeQueryKind::Hybrid
        || !query_has_language_scoped_inline_intent(query)
        || !query_language_scope_matches_hit(query, language_id)
        || (path_looks_like_test_or_benchmark(path) && !query_mentions_test_or_benchmark(query))
        || !content_has_inline_invocation(content)
    {
        return 0.0;
    }

    4.5
}

fn query_has_language_scoped_inline_intent(query: &str) -> bool {
    let terms = query_terms(query);
    terms.iter().any(|term| {
        matches!(
            term.as_str(),
            "callback" | "callbacks" | "closure" | "closures" | "lambda" | "lambdas"
        )
    }) && terms.iter().any(|term| {
        matches!(
            term.as_str(),
            "csharp"
                | "go"
                | "java"
                | "javascript"
                | "kotlin"
                | "python"
                | "ruby"
                | "rust"
                | "scala"
                | "swift"
                | "typescript"
        )
    })
}

fn query_language_scope_matches_hit(query: &str, language_id: &str) -> bool {
    query_terms(query)
        .iter()
        .any(|term| workflow_language_scope_matches(language_id, term))
}

fn content_has_inline_invocation(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    (content.contains("->") || content.contains("=>") || content.contains('|'))
        && (lower.contains(".map")
            || lower.contains(".filter")
            || lower.contains(".any")
            || lower.contains(".for_each")
            || content.contains(").")
            || content.contains("()"))
}

fn query_terms(query: &str) -> Vec<String> {
    query
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|term| !term.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

#[cfg(test)]
#[path = "inline_usage_tests.rs"]
mod tests;

use crate::domain::{CodeQueryKind, CodeRetrievalRequest};

const EXECUTION_FLOW_TERMS: &[&str] = &[
    "background",
    "connect",
    "connection",
    "event",
    "events",
    "finish",
    "flow",
    "lifecycle",
    "protocol",
    "reconnect",
    "route",
    "sse",
    "stream",
    "transport",
    "worker",
];

pub(super) fn caller_context_density_bonus(
    base_score: f64,
    query: &str,
    caller_name: Option<&str>,
    callee_name: &str,
    path: &str,
    caller_excerpt: Option<&str>,
    request: &CodeRetrievalRequest,
) -> f64 {
    if base_score <= 0.0 || request.code_query_kind != CodeQueryKind::Callers {
        return 0.0;
    }

    let target_terms = identifier_terms(&format!("{query} {callee_name}"));
    if target_terms.is_empty() {
        return 0.0;
    }

    caller_name_bonus(caller_name, &target_terms)
        + target_path_surface_bonus(path, &target_terms)
        + repeated_target_mention_bonus(caller_excerpt, callee_name)
}

fn caller_name_bonus(caller_name: Option<&str>, target_terms: &[String]) -> f64 {
    let Some(caller_name) = caller_name else {
        return 0.0;
    };
    let caller_terms = identifier_terms(caller_name);
    if caller_terms
        .iter()
        .any(|caller_term| target_terms.iter().any(|target| caller_term == target))
    {
        0.35
    } else {
        0.0
    }
}

fn target_path_surface_bonus(path: &str, target_terms: &[String]) -> f64 {
    let path_terms = identifier_terms(path);
    let has_surface_match = target_terms.iter().any(|target| {
        target.len() >= 4
            && path_terms
                .iter()
                .any(|path_term| related_identifier_terms(path_term, target))
    });
    if has_surface_match { 0.85 } else { 0.0 }
}

fn related_identifier_terms(left: &str, right: &str) -> bool {
    left == right
        || (left.len() >= 4
            && right.len() >= 4
            && (left.starts_with(right) || right.starts_with(left)))
}

fn repeated_target_mention_bonus(caller_excerpt: Option<&str>, callee_name: &str) -> f64 {
    let Some(caller_excerpt) = caller_excerpt else {
        return 0.0;
    };
    let callee = callee_name.trim();
    if callee.is_empty() {
        return 0.0;
    }
    let mentions = caller_excerpt.match_indices(callee).count();
    if mentions <= 1 {
        0.0
    } else {
        (mentions.saturating_sub(1).min(3) as f64) * 0.2
    }
}

pub(super) fn execution_flow_chunk_bonus(
    base_score: f64,
    query: &str,
    content: &str,
    path: &str,
    request: &CodeRetrievalRequest,
) -> f64 {
    if base_score <= 0.0
        || request.code_query_kind != CodeQueryKind::Hybrid
        || !query_execution_flow_intent(query)
        || (path_looks_like_test_or_benchmark(path) && !query_mentions_test_or_benchmark(query))
    {
        return 0.0;
    }

    let query_terms = meaningful_terms(query);
    if query_terms.len() < 4 {
        return 0.0;
    }
    let content_terms = meaningful_terms(content);
    let matched_terms = matched_query_terms(&query_terms, &content_terms);
    if matched_terms.len() < 3 {
        return 0.0;
    }

    let coverage = matched_terms.len() as f64 / query_terms.len() as f64;
    let ordered = ordered_query_term_coverage(&query_terms, &content_terms);
    let action_density = flow_action_density(content);
    ((coverage * 1.8) + (ordered * 0.9) + action_density).min(3.0)
}

fn query_execution_flow_intent(query: &str) -> bool {
    let terms = meaningful_terms(query);
    let intent_terms = terms
        .iter()
        .filter(|term| EXECUTION_FLOW_TERMS.contains(&term.as_str()))
        .count();
    intent_terms >= 2 || (intent_terms >= 1 && terms.len() >= 6)
}

fn matched_query_terms(query_terms: &[String], content_terms: &[String]) -> Vec<String> {
    let mut matched = Vec::new();
    for query_term in query_terms {
        if matched.contains(query_term) {
            continue;
        }
        if content_terms
            .iter()
            .any(|content_term| related_identifier_terms(content_term, query_term))
        {
            matched.push(query_term.clone());
        }
    }
    matched
}

fn ordered_query_term_coverage(query_terms: &[String], content_terms: &[String]) -> f64 {
    let ranks = query_terms
        .iter()
        .filter_map(|query_term| {
            content_terms
                .iter()
                .position(|content_term| related_identifier_terms(content_term, query_term))
        })
        .collect::<Vec<_>>();
    if ranks.len() <= 1 {
        return 0.0;
    }
    let ordered_pairs = ranks.windows(2).filter(|pair| pair[0] <= pair[1]).count();
    ordered_pairs as f64 / (ranks.len() - 1) as f64
}

fn flow_action_density(content: &str) -> f64 {
    let action_lines = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| {
            line.contains("=>")
                || line.contains("function")
                || line.contains("return ")
                || line.contains("yield")
                || line.contains("await")
                || line.contains(".on")
                || line.contains(".pipe")
                || line.contains(".make")
                || line.contains(".finish")
                || line.contains(".initial")
        })
        .take(8)
        .count();

    (action_lines as f64 * 0.12).min(0.75)
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

fn query_mentions_test_or_benchmark(query: &str) -> bool {
    meaningful_terms(query).iter().any(|term| {
        matches!(
            term.as_str(),
            "test" | "tests" | "testing" | "bench" | "benchmark" | "benchmarks"
        )
    })
}

fn meaningful_terms(value: &str) -> Vec<String> {
    let mut terms = identifier_terms(value)
        .into_iter()
        .filter(|term| term.len() >= 3)
        .filter(|term| !matches!(term.as_str(), "the" | "and" | "for" | "with" | "from"))
        .collect::<Vec<_>>();
    terms.sort();
    terms.dedup();
    terms
}

fn identifier_terms(value: &str) -> Vec<String> {
    let mut terms = Vec::new();
    for raw in value
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|term| !term.is_empty())
    {
        terms.push(raw.to_ascii_lowercase());
        terms.extend(
            raw.split('_')
                .filter(|part| !part.is_empty())
                .map(str::to_ascii_lowercase),
        );
        push_camel_terms(raw, &mut terms);
    }
    terms.sort();
    terms.dedup();
    terms
}

fn push_camel_terms(token: &str, terms: &mut Vec<String>) {
    let chars = token.char_indices().collect::<Vec<_>>();
    if chars.is_empty() {
        return;
    }

    let mut start = 0;
    for index in 1..chars.len() {
        let previous = chars[index - 1].1;
        let current = chars[index].1;
        let next = chars.get(index + 1).map(|(_, character)| *character);
        let starts_word = previous.is_ascii_lowercase() && current.is_ascii_uppercase();
        let ends_acronym = previous.is_ascii_uppercase()
            && current.is_ascii_uppercase()
            && next.is_some_and(|character| character.is_ascii_lowercase());
        let changes_kind = previous.is_ascii_alphabetic() != current.is_ascii_alphabetic();
        if starts_word || ends_acronym || changes_kind {
            terms.push(token[start..chars[index].0].to_ascii_lowercase());
            start = chars[index].0;
        }
    }
    terms.push(token[start..].to_ascii_lowercase());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{CodeRepositorySelector, FreshnessPolicy};

    #[test]
    fn caller_context_bonus_prefers_target_named_surfaces() {
        let request = request("redactUrl", CodeQueryKind::Callers);

        let redactor = caller_context_density_bonus(
            4.0,
            "redactUrl",
            Some("url"),
            "redactUrl",
            "packages/http-recorder/src/redactor.ts",
            Some("request: (snapshot) => ({ url: redactUrl(snapshot.url) })"),
            &request,
        );
        let executor = caller_context_density_bonus(
            4.0,
            "redactUrl",
            Some("requestDetails"),
            "redactUrl",
            "packages/llm/src/route/executor.ts",
            Some("url: redactUrl(request.url),"),
            &request,
        );

        assert!(redactor > executor);
    }

    #[test]
    fn execution_flow_bonus_requires_flow_intent_and_content_coverage() {
        let hybrid = request(
            "OpenAI Chat protocol SSE tool calls lifecycle finish events route transport",
            CodeQueryKind::Hybrid,
        );
        let focused = execution_flow_chunk_bonus(
            8.0,
            &hybrid.query,
            "OpenAI Chat protocol route uses SSE transport events.\nconst step = () => ToolStream.empty()\nLifecycle.finish(lifecycle, events)",
            "src/openai-chat.ts",
            &hybrid,
        );
        let narrow = execution_flow_chunk_bonus(
            8.0,
            &hybrid.query,
            "const lowerToolCall = () => ({ type: \"function\" })",
            "src/openai-chat.ts",
            &hybrid,
        );
        let symbol = request("OpenAI Chat", CodeQueryKind::Definition);

        assert!(focused > narrow);
        assert_eq!(
            execution_flow_chunk_bonus(
                8.0,
                &symbol.query,
                "OpenAI Chat protocol route",
                "src/openai-chat.ts",
                &symbol
            ),
            0.0
        );
    }

    #[test]
    fn execution_flow_bonus_ignores_tests_without_test_intent() {
        let hybrid = request(
            "OpenAI Chat protocol SSE tool calls lifecycle finish events route transport",
            CodeQueryKind::Hybrid,
        );

        assert_eq!(
            execution_flow_chunk_bonus(
                8.0,
                &hybrid.query,
                "OpenAI Chat protocol route uses SSE transport events.\nLifecycle.finish(lifecycle, events)",
                "packages/llm/test/openai-chat.test.ts",
                &hybrid,
            ),
            0.0
        );
    }

    fn request(query: &str, kind: CodeQueryKind) -> CodeRetrievalRequest {
        let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
            .expect("selector should validate");
        CodeRetrievalRequest::new(query, selector, kind, 10, FreshnessPolicy::AllowStale)
            .expect("request should validate")
    }
}

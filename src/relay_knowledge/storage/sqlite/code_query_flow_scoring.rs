use std::collections::BTreeSet;

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

const INLINE_CONSTRUCT_TERMS: &[&str] = &[
    "arrow", "callback", "closure", "handler", "inline", "lambda", "nested",
];
const COMPACT_HIGH_COVERAGE_MAX_NONBLANK_LINES: usize = 20;
const COMPACT_HIGH_COVERAGE_MIN_BASE_SCORE: f64 = 6.0;
const COMPACT_HIGH_COVERAGE_MIN_MATCHED_TERMS: usize = 4;
const COMPACT_API_SEQUENCE_MAX_NONBLANK_LINES: usize = 18;
const COMPACT_API_SEQUENCE_MAX_SPAN_LINES: usize = 14;
const COMPACT_API_SEQUENCE_MIN_MATCHED_IDENTITIES: usize = 3;
const SOURCE_DEFINITION_BODY_MAX_NONBLANK_LINES: usize = 36;
const SOURCE_DEFINITION_BODY_MIN_BASE_SCORE: f64 = 6.0;
const SOURCE_DEFINITION_BODY_MIN_MATCHED_TERMS: usize = 4;

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

pub(super) fn inline_construct_chunk_bonus(
    base_score: f64,
    query: &str,
    content: &str,
    path: &str,
    request: &CodeRetrievalRequest,
) -> f64 {
    if base_score <= 0.0
        || request.code_query_kind != CodeQueryKind::Hybrid
        || !query_inline_construct_intent(query)
        || (path_looks_like_test(path) && !query_mentions_test_or_benchmark(query))
        || !content_has_inline_construct(content)
    {
        return 0.0;
    }

    let query_terms = meaningful_terms(query);
    if query_terms.len() < 3 {
        return 0.0;
    }
    let content_terms = meaningful_terms(content);
    let matched_terms = matched_query_terms(&query_terms, &content_terms);
    if matched_terms.len() < 3 {
        return 0.0;
    }

    let coverage = matched_terms.len() as f64 / query_terms.len() as f64;
    ((coverage * 1.25) + inline_construct_density(content)).min(2.2)
}

pub(super) fn compact_high_coverage_chunk_bonus(
    base_score: f64,
    query: &str,
    content: &str,
    path: &str,
    request: &CodeRetrievalRequest,
) -> f64 {
    if base_score < COMPACT_HIGH_COVERAGE_MIN_BASE_SCORE
        || request.code_query_kind != CodeQueryKind::Hybrid
        || (path_looks_like_test_or_benchmark(path) && !query_mentions_test_or_benchmark(query))
    {
        return 0.0;
    }

    let query_terms = meaningful_terms(query);
    if query_terms.len() < COMPACT_HIGH_COVERAGE_MIN_MATCHED_TERMS {
        return 0.0;
    }
    let content_terms = meaningful_terms(content);
    let matched_terms = matched_query_terms(&query_terms, &content_terms);
    let required_matches = COMPACT_HIGH_COVERAGE_MIN_MATCHED_TERMS
        .max(query_terms.len().saturating_mul(3).div_ceil(4));
    if matched_terms.len() < required_matches {
        return 0.0;
    }

    let nonblank_lines = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(COMPACT_HIGH_COVERAGE_MAX_NONBLANK_LINES + 1)
        .count();
    if nonblank_lines == 0 || nonblank_lines > COMPACT_HIGH_COVERAGE_MAX_NONBLANK_LINES {
        return 0.0;
    }

    let coverage = matched_terms.len() as f64 / query_terms.len() as f64;
    (0.35 + (coverage * 0.4)).min(0.75)
}

pub(super) fn compact_api_sequence_chunk_bonus(
    base_score: f64,
    query: &str,
    content: &str,
    path: &str,
    request: &CodeRetrievalRequest,
) -> f64 {
    if base_score < COMPACT_HIGH_COVERAGE_MIN_BASE_SCORE
        || request.code_query_kind != CodeQueryKind::Hybrid
        || (path_looks_like_test_or_benchmark(path) && !query_mentions_test_or_benchmark(query))
    {
        return 0.0;
    }

    let identities = query_api_sequence_identities(query);
    if identities.len() < COMPACT_API_SEQUENCE_MIN_MATCHED_IDENTITIES {
        return 0.0;
    }
    let Some(sequence) = matched_api_sequence(content, &identities) else {
        return 0.0;
    };

    let nonblank_lines = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(COMPACT_API_SEQUENCE_MAX_NONBLANK_LINES + 1)
        .count();
    if nonblank_lines == 0 || nonblank_lines > COMPACT_API_SEQUENCE_MAX_NONBLANK_LINES {
        return 0.0;
    }
    let span = sequence
        .last_line
        .saturating_sub(sequence.first_line)
        .saturating_add(1);
    if span > COMPACT_API_SEQUENCE_MAX_SPAN_LINES {
        return 0.0;
    }

    let coverage = sequence.matched_identities as f64 / identities.len() as f64;
    let density = sequence.matched_lines as f64 / nonblank_lines as f64;
    let complete_sequence_bonus = if sequence.matched_identities == identities.len() {
        2.0
    } else {
        0.0
    };
    (0.45 + (coverage * 1.15) + (density * 0.45) + complete_sequence_bonus).min(3.8)
}

pub(super) fn source_definition_body_chunk_bonus(
    base_score: f64,
    query: &str,
    content: &str,
    path: &str,
    request: &CodeRetrievalRequest,
) -> f64 {
    if base_score < SOURCE_DEFINITION_BODY_MIN_BASE_SCORE
        || request.code_query_kind != CodeQueryKind::Hybrid
        || !path_looks_like_source_implementation(path)
        || path_looks_like_test_or_benchmark(path)
        || !content_looks_like_definition_body(content)
    {
        return 0.0;
    }

    let query_terms = meaningful_terms(query);
    if query_terms.len() < SOURCE_DEFINITION_BODY_MIN_MATCHED_TERMS {
        return 0.0;
    }
    let content_terms = meaningful_terms(content);
    let matched_terms = matched_query_terms(&query_terms, &content_terms);
    let required_matches =
        SOURCE_DEFINITION_BODY_MIN_MATCHED_TERMS.max(query_terms.len().div_ceil(2));
    if matched_terms.len() < required_matches {
        return 0.0;
    }

    let nonblank_lines = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(SOURCE_DEFINITION_BODY_MAX_NONBLANK_LINES + 1)
        .count();
    if nonblank_lines == 0 || nonblank_lines > SOURCE_DEFINITION_BODY_MAX_NONBLANK_LINES {
        return 0.0;
    }

    let coverage = matched_terms.len() as f64 / query_terms.len() as f64;
    let action_density = source_body_action_density(content);
    (0.65 + (coverage * 1.7) + action_density).min(2.75)
}

struct ApiSequenceMatch {
    matched_identities: usize,
    matched_lines: usize,
    first_line: usize,
    last_line: usize,
}

fn matched_api_sequence(content: &str, identities: &[Vec<String>]) -> Option<ApiSequenceMatch> {
    let mut matched_identity_indexes = BTreeSet::new();
    let mut matched_lines = BTreeSet::new();
    let mut first_line = None;
    let mut last_line = 0usize;

    for (line_index, line) in content.lines().enumerate() {
        let line = line.trim();
        if !line_looks_like_api_call(line) {
            continue;
        }
        let line_terms = identifier_terms(line);
        let matched_before = matched_identity_indexes.len();
        for (identity_index, identity_terms) in identities.iter().enumerate() {
            if identity_terms_match_line(identity_terms, &line_terms) {
                matched_identity_indexes.insert(identity_index);
            }
        }
        if matched_identity_indexes.len() > matched_before {
            matched_lines.insert(line_index);
            first_line.get_or_insert(line_index);
            last_line = line_index;
        }
    }

    (matched_identity_indexes.len() >= COMPACT_API_SEQUENCE_MIN_MATCHED_IDENTITIES).then(|| {
        ApiSequenceMatch {
            matched_identities: matched_identity_indexes.len(),
            matched_lines: matched_lines.len(),
            first_line: first_line.unwrap_or(0),
            last_line,
        }
    })
}

fn query_api_sequence_identities(query: &str) -> Vec<Vec<String>> {
    let mut identities = Vec::new();
    for raw in query.split_whitespace().map(str::trim) {
        if !token_looks_like_api_identity(raw) {
            continue;
        }
        let terms = identifier_terms(raw)
            .into_iter()
            .filter(|term| term.len() >= 3)
            .filter(|term| !matches!(term.as_str(), "the" | "and" | "for" | "with" | "from"))
            .collect::<Vec<_>>();
        if !terms.is_empty() && !identities.contains(&terms) {
            identities.push(terms);
        }
    }

    identities
}

fn token_looks_like_api_identity(token: &str) -> bool {
    let token = token.trim_matches(|character: char| {
        !(character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | ':'))
    });
    if token.contains('.') || token.contains("::") {
        return true;
    }

    let mut previous = None;
    token.chars().any(|character| {
        let boundary = character.is_ascii_uppercase()
            && previous.is_some_and(|prev: char| prev.is_ascii_lowercase());
        previous = Some(character);
        boundary
    })
}

fn line_looks_like_api_call(line: &str) -> bool {
    if line.is_empty()
        || line.starts_with("//")
        || line.starts_with('#')
        || line.starts_with('*')
        || !line.contains('(')
    {
        return false;
    }

    line.contains('.')
        || line.contains("::")
        || line.contains("->")
        || line.contains(":=")
        || line.contains(" = ")
}

fn identity_terms_match_line(identity_terms: &[String], line_terms: &[String]) -> bool {
    identity_terms.iter().all(|identity_term| {
        line_terms
            .iter()
            .any(|line_term| related_identifier_terms(line_term, identity_term))
    })
}

fn query_execution_flow_intent(query: &str) -> bool {
    let terms = meaningful_terms(query);
    let intent_terms = terms
        .iter()
        .filter(|term| EXECUTION_FLOW_TERMS.contains(&term.as_str()))
        .count();
    intent_terms >= 2 || (intent_terms >= 1 && terms.len() >= 6)
}

fn query_inline_construct_intent(query: &str) -> bool {
    meaningful_terms(query)
        .iter()
        .any(|term| INLINE_CONSTRUCT_TERMS.contains(&term.as_str()))
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

fn content_has_inline_construct(content: &str) -> bool {
    content.lines().any(line_contains_inline_construct)
}

fn content_looks_like_definition_body(content: &str) -> bool {
    if !(content.contains('{') && content.contains('}')) {
        return false;
    }

    let mut saw_signature = false;
    for line in content.lines().map(str::trim) {
        if line.is_empty() || line.starts_with("//") || line.starts_with('*') {
            continue;
        }
        if line == "{" && saw_signature {
            return true;
        }
        if line_looks_like_definition_signature(line) {
            if line.contains('{') {
                return true;
            }
            saw_signature = true;
        } else if line.ends_with(';') || line.ends_with('}') {
            saw_signature = false;
        }
    }

    false
}

fn line_looks_like_definition_signature(line: &str) -> bool {
    line.contains('(')
        && !line.ends_with(';')
        && !line.starts_with('#')
        && !line.starts_with("if ")
        && !line.starts_with("for ")
        && !line.starts_with("while ")
        && !line.starts_with("switch ")
        && !line.starts_with("catch ")
        && !line.starts_with("return ")
}

fn inline_construct_density(content: &str) -> f64 {
    let construct_lines = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| line_contains_inline_construct(line))
        .take(6)
        .count();

    (construct_lines as f64 * 0.3).min(0.9)
}

fn line_contains_inline_construct(line: &str) -> bool {
    let line = line.trim();
    line.contains("=>")
        || line.contains(" -> ")
        || line.contains("[](")
        || line.contains("](")
        || line.contains(": func(")
        || line.contains(": func (")
        || line.contains("function(")
        || line.contains("function (")
        || line.contains("lambda ")
        || line.contains("lambda:")
        || line.contains("static inline")
        || line.starts_with("async def ")
        || line.starts_with("def ")
        || line.contains(".addEventListener(")
        || line_contains_pipe_closure(line)
}

fn line_contains_pipe_closure(line: &str) -> bool {
    if !(line.contains("(|")
        || line.contains("= |")
        || line.contains(": |")
        || line.contains(", |")
        || line.contains("return |"))
    {
        return false;
    }
    line.matches('|').take(3).count() >= 2
}

fn source_body_action_density(content: &str) -> f64 {
    let action_lines = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| {
            line.contains("->")
                || line.contains('.')
                || line.contains("::")
                || line.contains(":=")
                || line.contains(" = ")
                || line.contains("return ")
                || line.contains("await ")
        })
        .take(6)
        .count();

    (action_lines as f64 * 0.18).min(0.9)
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

fn path_looks_like_source_implementation(path: &str) -> bool {
    let extension = path
        .rsplit('/')
        .next()
        .and_then(|file_name| file_name.rsplit_once('.').map(|(_, extension)| extension))
        .map(str::to_ascii_lowercase);
    matches!(
        extension.as_deref(),
        Some(
            "c" | "cc"
                | "cpp"
                | "cxx"
                | "cs"
                | "go"
                | "java"
                | "js"
                | "jsx"
                | "kt"
                | "kts"
                | "php"
                | "py"
                | "rb"
                | "rs"
                | "scala"
                | "swift"
                | "ts"
                | "tsx"
        )
    )
}

fn path_looks_like_test(path: &str) -> bool {
    path.to_ascii_lowercase().split('/').any(|segment| {
        matches!(segment, "test" | "tests" | "__tests__" | "testing")
            || segment.ends_with("_test")
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

    #[test]
    fn inline_construct_bonus_prefers_callback_shapes() {
        let hybrid = request(
            "Project db helper Database.use inline callback Effect.sync",
            CodeQueryKind::Hybrid,
        );
        let inline = inline_construct_chunk_bonus(
            8.0,
            &hybrid.query,
            "const db = <T>(fn: Fn<T>) => Effect.sync(() => Database.use(fn))",
            "packages/opencode/src/project/project.ts",
            &hybrid,
        );
        let named = inline_construct_chunk_bonus(
            8.0,
            &hybrid.query,
            "function db(fn) { return Effect.sync(Database.use(fn)) }",
            "packages/opencode/src/project/project.ts",
            &hybrid,
        );

        assert!(inline > named, "inline={inline} named={named}");
        assert_eq!(named, 0.0);
    }

    #[test]
    fn inline_construct_bonus_detects_cross_language_forms() {
        for (query, content) in [
            (
                "ReflectionUtils USER_DECLARED_METHODS MethodFilter lambda isBridge isSynthetic",
                "MethodFilter USER_DECLARED_METHODS = method -> !method.isBridge() && !method.isSynthetic()",
            ),
            (
                "ResourceEventHandlerFuncs AddFunc inline addNode UpdateFunc",
                "ResourceEventHandlerFuncs{AddFunc: func(obj interface{}) { ttlc.addNode(obj) }}",
            ),
            (
                "keystone_roles roles.iter any admin reseller_admin closure",
                "keystone_roles.iter().any(|role| role == \"admin\" || role == \"reseller_admin\")",
            ),
            (
                "_handle_connection nested async recv_json send_event websocket",
                "async def recv_json():\n    await websocket.recv()\nasync def send_event(): pass",
            ),
            (
                "round_hint_to_min static inline do_mmap mmap_min_addr",
                "static inline unsigned long round_hint_to_min(unsigned long addr) { return mmap_min_addr; }",
            ),
            (
                "setupEventBindings promptInput addEventListener input arrow callback handlePromptComposerInput",
                "promptInput.addEventListener(\"input\", () => handlePromptComposerInput())",
            ),
        ] {
            let request = request(query, CodeQueryKind::Hybrid);
            let bonus = inline_construct_chunk_bonus(
                8.0,
                &request.query,
                content,
                "src/inline-source.rs",
                &request,
            );
            assert!(bonus > 0.0, "missing inline bonus for {content}");
        }
    }

    #[test]
    fn inline_construct_bonus_ignores_tests_without_test_intent() {
        let hybrid = request(
            "Project db helper Database.use inline callback Effect.sync",
            CodeQueryKind::Hybrid,
        );

        assert_eq!(
            inline_construct_chunk_bonus(
                8.0,
                &hybrid.query,
                "const db = <T>(fn: Fn<T>) => Effect.sync(() => Database.use(fn))",
                "packages/opencode/src/project/project.test.ts",
                &hybrid,
            ),
            0.0
        );
    }

    #[test]
    fn inline_construct_bonus_allows_benchmark_implementation_sources() {
        let hybrid = request(
            "ZstdCompress lambda port::Zstd_Compress FLAGS_zstd_compression_level",
            CodeQueryKind::Hybrid,
        );
        let bonus = inline_construct_chunk_bonus(
            8.0,
            &hybrid.query,
            "auto ZstdCompress = [](const char* input, size_t length) { return port::Zstd_Compress(input, length, FLAGS_zstd_compression_level); };",
            "benchmarks/db_bench.cc",
            &hybrid,
        );

        assert!(bonus > 0.0, "benchmark lambda should be eligible");
    }

    #[test]
    fn compact_high_coverage_bonus_prefers_concise_usage_chunks() {
        let hybrid = request(
            "client.Dial envconfig MustLoadDefaultClientOptions workflow client",
            CodeQueryKind::Hybrid,
        );
        let compact = compact_high_coverage_chunk_bonus(
            8.0,
            &hybrid.query,
            "func main() {\n\
            c, err := client.Dial(envconfig.MustLoadDefaultClientOptions())\n\
            if err != nil { panic(err) }\n\
            w := worker.New(c, \"hello-world\", worker.Options{})\n\
            w.RegisterWorkflow(helloworld.Workflow)\n\
            err = w.Run(worker.InterruptCh())\n\
            if err != nil { panic(err) }\n\
            }",
            "helloworld/worker/main.go",
            &hybrid,
        );
        let long = compact_high_coverage_chunk_bonus(
            8.0,
            &hybrid.query,
            &(0..25)
                .map(|_| "client.Dial envconfig MustLoadDefaultClientOptions workflow client")
                .collect::<Vec<_>>()
                .join("\n"),
            "starter/main.go",
            &hybrid,
        );
        let definition = request("client.Dial workflow client", CodeQueryKind::Definition);

        assert!(
            compact > 0.0,
            "compact chunk should receive a bounded bonus"
        );
        assert_eq!(long, 0.0);
        assert_eq!(
            compact_high_coverage_chunk_bonus(
                8.0,
                &definition.query,
                "client.Dial workflow client",
                "src/client.go",
                &definition,
            ),
            0.0
        );
    }

    #[test]
    fn compact_api_sequence_bonus_prefers_complete_short_lifecycle_flows() {
        let hybrid = request(
            "worker.New RegisterWorkflow RegisterActivity InterruptCh task queue",
            CodeQueryKind::Hybrid,
        );
        let complete = compact_api_sequence_chunk_bonus(
            8.0,
            &hybrid.query,
            "func main() {\n\
            w := worker.New(c, \"hello-world\", worker.Options{})\n\
            w.RegisterWorkflow(helloworld.Workflow)\n\
            w.RegisterActivity(helloworld.Activity)\n\
            err = w.Run(worker.InterruptCh())\n\
            }",
            "helloworld/worker/main.go",
            &hybrid,
        );
        let partial = compact_api_sequence_chunk_bonus(
            8.0,
            &hybrid.query,
            "func main() {\n\
            w := worker.New(c, caller.TaskQueue, worker.Options{})\n\
            w.RegisterWorkflow(caller.EchoCallerWorkflow)\n\
            err = w.Run(worker.InterruptCh())\n\
            }",
            "nexus/caller/worker/main.go",
            &hybrid,
        );
        let verbose = compact_api_sequence_chunk_bonus(
            8.0,
            &hybrid.query,
            &(0..24)
                .map(|_| "w.RegisterWorkflow(flow.Workflow); w.RegisterActivity(flow.Activity)")
                .collect::<Vec<_>>()
                .join("\n"),
            "worker-specific-task-queues/worker/main.go",
            &hybrid,
        );

        assert!(complete > partial, "complete={complete} partial={partial}");
        assert!(
            partial > 0.0,
            "partial compact API sequence should still score"
        );
        assert_eq!(verbose, 0.0);
    }

    fn request(query: &str, kind: CodeQueryKind) -> CodeRetrievalRequest {
        let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
            .expect("selector should validate");
        CodeRetrievalRequest::new(query, selector, kind, 10, FreshnessPolicy::AllowStale)
            .expect("request should validate")
    }
}

use std::collections::BTreeSet;

use crate::domain::{CodeQueryKind, CodeRetrievalRequest};

#[derive(Clone, Copy)]
pub(super) struct CallSiteQueryIntent {
    pub(super) test_or_benchmark: bool,
    pub(super) example_or_sample: bool,
}

const CALLER_RESULT_ASSIGNMENT_BONUS: f64 = 1.15;
const CALLER_RESULT_NAMED_SLOT_BONUS: f64 = 0.25;

pub(super) fn call_site_source_path_bonus(
    base_score: f64,
    path: &str,
    request: &CodeRetrievalRequest,
    query: &str,
    query_has_test_intent: bool,
) -> f64 {
    if base_score <= 0.0
        || query_has_test_intent
        || !matches!(
            request.code_query_kind,
            CodeQueryKind::Callers | CodeQueryKind::Callees
        )
        || path_looks_like_test_or_benchmark(path)
        || (!query_mentions_example_or_sample(query) && path_looks_like_example_or_sample(path))
    {
        return 0.0;
    }

    if request.code_query_kind == CodeQueryKind::Callers
        && !query_mentions_adapter_surface(query)
        && path_looks_like_adapter_surface(path)
    {
        return 0.0;
    }

    0.2
}

pub(super) fn call_site_test_path_penalty(
    base_score: f64,
    path: &str,
    request: &CodeRetrievalRequest,
    query_has_test_intent: bool,
) -> f64 {
    if base_score <= 0.0
        || query_has_test_intent
        || !matches!(
            request.code_query_kind,
            CodeQueryKind::Callers | CodeQueryKind::Callees
        )
        || !path_looks_like_test_or_benchmark(path)
    {
        return 0.0;
    }

    -0.35
}

pub(super) fn call_site_example_path_penalty(
    base_score: f64,
    path: &str,
    request: &CodeRetrievalRequest,
    query_has_example_intent: bool,
) -> f64 {
    if base_score <= 0.0
        || query_has_example_intent
        || !matches!(
            request.code_query_kind,
            CodeQueryKind::Callers | CodeQueryKind::Callees
        )
        || !path_looks_like_example_or_sample(path)
    {
        return 0.0;
    }

    -0.6
}

pub(super) fn caller_result_assignment_bonus(
    base_score: f64,
    path: &str,
    query: &str,
    caller_excerpt: Option<&str>,
    callee_name: &str,
    request: &CodeRetrievalRequest,
    intent: CallSiteQueryIntent,
) -> f64 {
    if base_score <= 0.0
        || request.code_query_kind != CodeQueryKind::Callers
        || (path_looks_like_test_or_benchmark(path) && !intent.test_or_benchmark)
        || (path_looks_like_example_or_sample(path) && !intent.example_or_sample)
        || (path_looks_like_adapter_surface(path) && !query_mentions_adapter_surface(query))
    {
        return 0.0;
    }
    let Some(caller_excerpt) = caller_excerpt else {
        return 0.0;
    };
    if callee_name.trim().is_empty() {
        return 0.0;
    }

    caller_excerpt
        .lines()
        .filter_map(|line| assigned_call_result_bonus(line, callee_name))
        .fold(0.0, f64::max)
}

fn assigned_call_result_bonus(line: &str, callee_name: &str) -> Option<f64> {
    let mut search_start = 0;
    while let Some(relative_index) = line[search_start..].find(callee_name) {
        let start = search_start + relative_index;
        let end = start + callee_name.len();
        let prefix = &line[..start];
        if identifier_boundary_before(prefix) && call_suffix(&line[end..]) {
            let Some(left_side) = assigned_call_left_side(prefix) else {
                search_start = end;
                continue;
            };
            return Some(
                CALLER_RESULT_ASSIGNMENT_BONUS
                    + named_assignment_slot_bonus(left_side, callee_name),
            );
        }
        search_start = end;
    }

    None
}

fn identifier_boundary_before(prefix: &str) -> bool {
    prefix
        .chars()
        .next_back()
        .is_none_or(|character| !(character.is_ascii_alphanumeric() || character == '_'))
}

fn assigned_call_left_side(prefix: &str) -> Option<&str> {
    let Some(index) = prefix.rfind('=') else {
        return None;
    };
    let previous = prefix[..index]
        .chars()
        .rev()
        .find(|character| !character.is_whitespace());
    if previous.is_some_and(|character| matches!(character, '=' | '!' | '<' | '>')) {
        return None;
    }
    let next = prefix[index + 1..]
        .chars()
        .find(|character| !character.is_whitespace());
    if matches!(next, Some('>')) {
        return None;
    }

    Some(prefix[..index].trim().trim_end_matches(':').trim())
}

fn named_assignment_slot_bonus(left_side: &str, callee_name: &str) -> f64 {
    let left_terms = identifier_terms(left_side);
    if left_terms.is_empty() {
        return 0.0;
    }
    identifier_terms(callee_name)
        .into_iter()
        .any(|term| term.len() >= 4 && left_terms.contains(&term))
        .then_some(CALLER_RESULT_NAMED_SLOT_BONUS)
        .unwrap_or(0.0)
}

fn identifier_terms(value: &str) -> BTreeSet<String> {
    let mut terms = BTreeSet::new();
    for token in value.split(|character: char| !character.is_ascii_alphanumeric()) {
        push_identifier_terms(&mut terms, token);
    }

    terms
}

fn push_identifier_terms(terms: &mut BTreeSet<String>, token: &str) {
    let mut start = 0;
    let characters = token.char_indices().collect::<Vec<_>>();
    for index in 1..characters.len() {
        let (byte_index, character) = characters[index];
        let previous = characters[index - 1].1;
        let next = characters.get(index + 1).map(|(_, next)| *next);
        let acronym_tail = previous.is_ascii_uppercase()
            && character.is_ascii_uppercase()
            && next.is_some_and(|next| next.is_ascii_lowercase());
        if (character.is_ascii_uppercase()
            && (previous.is_ascii_lowercase() || previous.is_ascii_digit()))
            || acronym_tail
        {
            push_identifier_term(terms, &token[start..byte_index]);
            start = byte_index;
        }
    }
    push_identifier_term(terms, &token[start..]);
}

fn push_identifier_term(terms: &mut BTreeSet<String>, term: &str) {
    let term = term.trim().to_ascii_lowercase();
    if !term.is_empty() {
        terms.insert(term);
    }
}

pub(super) fn callee_member_context_bonus(
    base_score: f64,
    caller_excerpt: Option<&str>,
    callee_name: &str,
    request: &CodeRetrievalRequest,
) -> f64 {
    if base_score <= 0.0 || request.code_query_kind != CodeQueryKind::Callees {
        return 0.0;
    }
    let Some(caller_excerpt) = caller_excerpt else {
        return 0.0;
    };
    if callee_name.trim().is_empty() {
        return 0.0;
    }

    if caller_excerpt
        .lines()
        .any(|line| line_contains_member_call_to(line, callee_name))
    {
        0.45
    } else {
        0.0
    }
}

fn line_contains_member_call_to(line: &str, callee_name: &str) -> bool {
    let mut search_start = 0;
    while let Some(relative_index) = line[search_start..].find(callee_name) {
        let start = search_start + relative_index;
        let end = start + callee_name.len();
        if member_call_prefix(&line[..start]) && call_suffix(&line[end..]) {
            return true;
        }
        search_start = end;
    }

    false
}

fn member_call_prefix(prefix: &str) -> bool {
    let prefix = prefix.trim_end();
    prefix.ends_with('.') || prefix.ends_with("::") || prefix.ends_with("->")
}

fn call_suffix(suffix: &str) -> bool {
    let suffix = suffix.trim_start();
    suffix.starts_with('(') || (suffix.starts_with('<') && suffix.contains('('))
}

pub(super) fn declaration_surface_path_bonus(
    declaration_bonus: f64,
    path: &str,
    request: &CodeRetrievalRequest,
) -> f64 {
    if declaration_bonus <= 0.0
        || request.code_query_kind != CodeQueryKind::Hybrid
        || path_looks_like_test_or_benchmark(path)
    {
        return 0.0;
    }
    let file_name = path.rsplit('/').next().unwrap_or(path);
    if file_name_has_header_extension(file_name) {
        0.35
    } else {
        0.0
    }
}

pub(super) fn symbol_declaration_surface_path_bonus(
    base_score: f64,
    kind: &str,
    path: &str,
    request: &CodeRetrievalRequest,
) -> f64 {
    if base_score <= 0.0
        || request.code_query_kind != CodeQueryKind::Hybrid
        || kind != "function_declaration"
        || path_looks_like_test_or_benchmark(path)
    {
        return 0.0;
    }
    let file_name = path.rsplit('/').next().unwrap_or(path);
    if file_name_has_header_extension(file_name) {
        0.55
    } else {
        0.0
    }
}

pub(super) fn import_test_path_penalty(
    base_score: f64,
    path: &str,
    request: &CodeRetrievalRequest,
    query_has_test_intent: bool,
) -> f64 {
    if base_score <= 0.0
        || query_has_test_intent
        || !matches!(
            request.code_query_kind,
            CodeQueryKind::Hybrid | CodeQueryKind::Imports
        )
        || !path_looks_like_test_or_benchmark(path)
    {
        return 0.0;
    }

    -0.35
}

pub(super) fn symbol_test_path_penalty(
    base_score: f64,
    path: &str,
    request: &CodeRetrievalRequest,
    query_has_test_intent: bool,
) -> f64 {
    if base_score <= 0.0
        || query_has_test_intent
        || !matches!(
            request.code_query_kind,
            CodeQueryKind::Definition | CodeQueryKind::Symbol | CodeQueryKind::Hybrid
        )
        || !path_looks_like_test_or_benchmark(path)
    {
        return 0.0;
    }

    -0.75
}

pub(super) fn query_mentions_test_or_benchmark(query: &str) -> bool {
    query
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|term| !term.is_empty())
        .any(term_mentions_test_or_benchmark)
}

pub(super) fn query_mentions_example_or_sample(query: &str) -> bool {
    query
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|term| !term.is_empty())
        .any(term_mentions_example_or_sample)
}

fn term_mentions_test_or_benchmark(term: &str) -> bool {
    identifier_intent_parts(term)
        .iter()
        .any(|part| term_is_test_or_benchmark(part))
}

fn term_mentions_example_or_sample(term: &str) -> bool {
    identifier_intent_parts(term)
        .iter()
        .any(|part| term_is_example_or_sample(part))
}

fn path_looks_like_test_or_benchmark(path: &str) -> bool {
    let lower_path = path.to_ascii_lowercase();
    lower_path
        .split('/')
        .any(|segment| term_mentions_test_or_benchmark(segment) || segment == "__tests__")
        || lower_path
            .rsplit('/')
            .next()
            .is_some_and(file_name_looks_like_test_or_benchmark)
}

fn path_looks_like_example_or_sample(path: &str) -> bool {
    let lower_path = path.to_ascii_lowercase();
    lower_path
        .split('/')
        .any(segment_mentions_example_or_sample)
        || lower_path
            .rsplit('/')
            .next()
            .is_some_and(file_name_looks_like_example_or_sample)
}

fn path_looks_like_adapter_surface(path: &str) -> bool {
    let lower_path = path.to_ascii_lowercase();
    lower_path.split('/').any(segment_mentions_adapter_surface)
        || lower_path
            .rsplit('/')
            .next()
            .is_some_and(file_name_looks_like_adapter_surface)
}

fn query_mentions_adapter_surface(query: &str) -> bool {
    query
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|term| !term.is_empty())
        .any(term_mentions_adapter_surface)
}

fn file_name_looks_like_test_or_benchmark(file_name: &str) -> bool {
    let stem = file_name
        .rsplit_once('.')
        .map_or(file_name, |(stem, _)| stem);
    stem == "test"
        || stem == "tests"
        || stem == "testing"
        || stem == "bench"
        || stem == "benchmark"
        || stem.starts_with("test_")
        || stem.ends_with("_test")
        || stem.ends_with("_tests")
        || stem.ends_with("_bench")
        || stem.ends_with("_benchmark")
}

fn term_is_test_or_benchmark(term: &str) -> bool {
    matches!(
        term,
        "test" | "tests" | "testing" | "bench" | "benchmark" | "benchmarks"
    )
}

fn file_name_looks_like_example_or_sample(file_name: &str) -> bool {
    let stem = file_name
        .rsplit_once('.')
        .map_or(file_name, |(stem, _)| stem);
    segment_mentions_example_or_sample(stem)
}

fn term_is_example_or_sample(term: &str) -> bool {
    matches!(
        term,
        "demo"
            | "demos"
            | "example"
            | "examples"
            | "quickstart"
            | "quickstarts"
            | "sample"
            | "samples"
            | "tutorial"
            | "tutorials"
    )
}

fn file_name_looks_like_adapter_surface(file_name: &str) -> bool {
    let stem = file_name
        .rsplit_once('.')
        .map_or(file_name, |(stem, _)| stem);
    stem == "c" || segment_mentions_adapter_surface(stem)
}

fn segment_mentions_example_or_sample(segment: &str) -> bool {
    term_mentions_example_or_sample(segment)
}

fn segment_mentions_adapter_surface(segment: &str) -> bool {
    term_mentions_adapter_surface(segment)
}

fn term_mentions_adapter_surface(term: &str) -> bool {
    identifier_intent_parts(term)
        .iter()
        .any(|part| term_is_adapter_surface(part))
}

fn identifier_intent_parts(term: &str) -> Vec<String> {
    let mut parts = Vec::new();
    for chunk in term.split(['_', '-', '.']).filter(|part| !part.is_empty()) {
        push_camel_case_intent_parts(chunk, &mut parts);
    }
    parts
}

fn push_camel_case_intent_parts(chunk: &str, parts: &mut Vec<String>) {
    let characters = chunk.char_indices().collect::<Vec<_>>();
    if characters.is_empty() {
        return;
    }

    let mut start = 0;
    for index in 1..characters.len() {
        let previous = characters[index - 1].1;
        let current = characters[index].1;
        let next = characters.get(index + 1).map(|(_, character)| *character);
        let starts_camel_word = previous.is_ascii_lowercase() && current.is_ascii_uppercase();
        let ends_acronym = previous.is_ascii_uppercase()
            && current.is_ascii_uppercase()
            && next.is_some_and(|c| c.is_ascii_lowercase());
        let changes_alnum_kind = previous.is_ascii_alphabetic() != current.is_ascii_alphabetic();
        if starts_camel_word || ends_acronym || changes_alnum_kind {
            push_intent_part(&chunk[start..characters[index].0], parts);
            start = characters[index].0;
        }
    }
    push_intent_part(&chunk[start..], parts);

    let uppercase_or_digit = chunk
        .chars()
        .all(|character| character.is_ascii_uppercase() || character.is_ascii_digit());
    if uppercase_or_digit {
        let lower_chunk = chunk.to_ascii_lowercase();
        for suffix in ["api", "ffi", "jni"] {
            if lower_chunk.len() > suffix.len() && lower_chunk.ends_with(suffix) {
                push_intent_part(suffix, parts);
            }
        }
    }
}

fn push_intent_part(part: &str, parts: &mut Vec<String>) {
    if !part.is_empty() {
        parts.push(part.to_ascii_lowercase());
    }
}

fn term_is_adapter_surface(term: &str) -> bool {
    matches!(
        term,
        "adapter"
            | "adapters"
            | "api"
            | "binding"
            | "bindings"
            | "bridge"
            | "ffi"
            | "interop"
            | "jni"
            | "wrapper"
            | "wrappers"
    )
}

fn file_name_has_header_extension(file_name: &str) -> bool {
    let Some((_, extension)) = file_name.rsplit_once('.') else {
        return false;
    };

    matches!(
        extension.to_ascii_lowercase().as_str(),
        "h" | "hh" | "hpp" | "hxx" | "inc" | "ipp"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_site_source_path_bonus_prefers_application_edges_over_noise() {
        let callers = retrieval_request(CodeQueryKind::Callers);
        let hybrid = retrieval_request(CodeQueryKind::Hybrid);

        assert_eq!(
            call_site_source_path_bonus(4.0, "db/db_impl.cc", &callers, "NewLRUCache", false),
            0.2
        );
        assert_eq!(
            call_site_source_path_bonus(4.0, "db/db_test.cc", &callers, "NewLRUCache", false),
            0.0
        );
        assert_eq!(
            call_site_source_path_bonus(
                4.0,
                "benchmarks/db_bench.cc",
                &callers,
                "NewLRUCache",
                false,
            ),
            0.0
        );
        assert_eq!(
            call_site_source_path_bonus(
                4.0,
                "src/pkg/__tests__/caller.ts",
                &callers,
                "NewLRUCache",
                false,
            ),
            0.0
        );
        assert_eq!(
            call_site_source_path_bonus(
                4.0,
                "packages/llm/example/tutorial.ts",
                &callers,
                "generateObject",
                false,
            ),
            0.0
        );
        assert_eq!(
            call_site_source_path_bonus(
                4.0,
                "packages/llm/example/tutorial.ts",
                &callers,
                "generateObject tutorial",
                false,
            ),
            0.2
        );
        assert_eq!(
            call_site_source_path_bonus(0.0, "db/db_impl.cc", &callers, "NewLRUCache", false),
            0.0
        );
        assert_eq!(
            call_site_source_path_bonus(4.0, "db/db_impl.cc", &hybrid, "NewLRUCache", false),
            0.0
        );
        assert_eq!(
            call_site_source_path_bonus(4.0, "db/db_impl.cc", &callers, "NewLRUCache", true),
            0.0
        );
    }

    #[test]
    fn call_site_test_path_penalty_demotes_tests_without_test_intent() {
        let callers = retrieval_request(CodeQueryKind::Callers);
        let callees = retrieval_request(CodeQueryKind::Callees);
        let hybrid = retrieval_request(CodeQueryKind::Hybrid);

        assert_eq!(
            call_site_test_path_penalty(4.0, "table/filter_block_test.cc", &callers, false),
            -0.35
        );
        assert_eq!(
            call_site_test_path_penalty(4.0, "util/bloom_test.cc", &callees, false),
            -0.35
        );
        assert_eq!(
            call_site_test_path_penalty(4.0, "table/table.cc", &callers, false),
            0.0
        );
        assert_eq!(
            call_site_test_path_penalty(4.0, "table/filter_block_test.cc", &callers, true),
            0.0
        );
        assert_eq!(
            call_site_test_path_penalty(4.0, "table/filter_block_test.cc", &hybrid, false),
            0.0
        );
        assert_eq!(
            call_site_test_path_penalty(0.0, "table/filter_block_test.cc", &callers, false),
            0.0
        );
    }

    #[test]
    fn call_site_example_path_penalty_demotes_examples_without_example_intent() {
        let callers = retrieval_request(CodeQueryKind::Callers);
        let callees = retrieval_request(CodeQueryKind::Callees);
        let hybrid = retrieval_request(CodeQueryKind::Hybrid);

        assert_eq!(
            call_site_example_path_penalty(
                4.0,
                "packages/llm/example/tutorial.ts",
                &callers,
                false,
            ),
            -0.6
        );
        assert_eq!(
            call_site_example_path_penalty(4.0, "examples/cache_demo.cc", &callees, false),
            -0.6
        );
        assert_eq!(
            call_site_example_path_penalty(4.0, "src/sample_controller/handler.go", &callers, true),
            0.0
        );
        assert_eq!(
            call_site_example_path_penalty(4.0, "src/service.ts", &callers, false),
            0.0
        );
        assert_eq!(
            call_site_example_path_penalty(4.0, "packages/llm/example/tutorial.ts", &hybrid, false,),
            0.0
        );
        assert_eq!(
            call_site_example_path_penalty(
                0.0,
                "packages/llm/example/tutorial.ts",
                &callers,
                false,
            ),
            0.0
        );
    }

    #[test]
    fn call_site_source_path_bonus_demotes_adapter_surfaces_without_adapter_intent() {
        let callers = retrieval_request(CodeQueryKind::Callers);
        let callees = retrieval_request(CodeQueryKind::Callees);

        assert_eq!(
            call_site_source_path_bonus(4.0, "db/c.cc", &callers, "NewLRUCache", false),
            0.0
        );
        assert_eq!(
            call_site_source_path_bonus(
                4.0,
                "bindings/cache_wrapper.cc",
                &callers,
                "NewLRUCache",
                false,
            ),
            0.0
        );
        assert_eq!(
            call_site_source_path_bonus(4.0, "src/c/cache.cc", &callers, "NewLRUCache", false),
            0.2
        );
        assert_eq!(
            call_site_source_path_bonus(4.0, "db/c.cc", &callers, "C API NewLRUCache", false),
            0.2
        );
        assert_eq!(
            call_site_source_path_bonus(4.0, "db/c.cc", &callers, "c_api NewLRUCache", false),
            0.2
        );
        assert_eq!(
            call_site_source_path_bonus(4.0, "db/c.cc", &callers, "FFIWrapper", false),
            0.2
        );
        assert_eq!(
            call_site_source_path_bonus(4.0, "db/c.cc", &callers, "CAPI NewLRUCache", false),
            0.2
        );
        assert_eq!(
            call_site_source_path_bonus(4.0, "db/c.cc", &callers, "ApiBridge", false),
            0.2
        );
        assert_eq!(
            call_site_source_path_bonus(4.0, "db/c.cc", &callees, "NewLRUCache", false),
            0.2
        );
    }

    #[test]
    fn query_mentions_test_or_benchmark_detects_explicit_intent() {
        assert!(!query_mentions_test_or_benchmark("NewLRUCache"));
        assert!(query_mentions_test_or_benchmark("NewLRUCache test caller"));
        assert!(query_mentions_test_or_benchmark("db_bench cache"));
        assert!(query_mentions_test_or_benchmark("UnitTestCoverage"));
        assert!(query_mentions_test_or_benchmark("BenchmarkSuite"));
    }

    #[test]
    fn query_mentions_example_or_sample_detects_explicit_intent() {
        assert!(!query_mentions_example_or_sample("generateObject"));
        assert!(query_mentions_example_or_sample("generateObject tutorial"));
        assert!(query_mentions_example_or_sample("sample-controller worker"));
        assert!(query_mentions_example_or_sample("QuickstartDemo"));
    }

    #[test]
    fn declaration_surface_path_bonus_prefers_non_test_headers() {
        let hybrid = retrieval_request(CodeQueryKind::Hybrid);
        let definition = retrieval_request(CodeQueryKind::Definition);

        assert_eq!(
            declaration_surface_path_bonus(2.0, "db/db_impl.h", &hybrid),
            0.35
        );
        assert_eq!(
            declaration_surface_path_bonus(2.0, "include/leveldb/cache.hpp", &hybrid),
            0.35
        );
        assert_eq!(
            declaration_surface_path_bonus(2.0, "db/db_impl.cc", &hybrid),
            0.0
        );
        assert_eq!(
            declaration_surface_path_bonus(2.0, "db/db_impl_test.h", &hybrid),
            0.0
        );
        assert_eq!(
            declaration_surface_path_bonus(0.0, "db/db_impl.h", &hybrid),
            0.0
        );
        assert_eq!(
            declaration_surface_path_bonus(2.0, "db/db_impl.h", &definition),
            0.0
        );
    }

    #[test]
    fn symbol_declaration_surface_path_bonus_prefers_header_declarations() {
        let hybrid = retrieval_request(CodeQueryKind::Hybrid);
        let definition = retrieval_request(CodeQueryKind::Definition);

        assert_eq!(
            symbol_declaration_surface_path_bonus(
                4.0,
                "function_declaration",
                "db/db_impl.h",
                &hybrid,
            ),
            0.55
        );
        assert_eq!(
            symbol_declaration_surface_path_bonus(4.0, "method", "db/db_impl.h", &hybrid),
            0.0
        );
        assert_eq!(
            symbol_declaration_surface_path_bonus(
                4.0,
                "function_declaration",
                "db/db_impl.cc",
                &hybrid,
            ),
            0.0
        );
        assert_eq!(
            symbol_declaration_surface_path_bonus(
                4.0,
                "function_declaration",
                "db/db_impl_test.h",
                &hybrid,
            ),
            0.0
        );
        assert_eq!(
            symbol_declaration_surface_path_bonus(
                4.0,
                "function_declaration",
                "db/db_impl.h",
                &definition,
            ),
            0.0
        );
    }

    #[test]
    fn import_test_path_penalty_demotes_test_importers_without_test_intent() {
        let imports = retrieval_request(CodeQueryKind::Imports);
        let hybrid = retrieval_request(CodeQueryKind::Hybrid);
        let definition = retrieval_request(CodeQueryKind::Definition);

        assert_eq!(
            import_test_path_penalty(3.0, "table/filter_block_test.cc", &imports, false),
            -0.35
        );
        assert_eq!(
            import_test_path_penalty(3.0, "src/__tests__/provider.ts", &hybrid, false),
            -0.35
        );
        assert_eq!(
            import_test_path_penalty(3.0, "table/filter_block.cc", &imports, false),
            0.0
        );
        assert_eq!(
            import_test_path_penalty(3.0, "table/filter_block_test.cc", &imports, true),
            0.0
        );
        assert_eq!(
            import_test_path_penalty(0.0, "table/filter_block_test.cc", &imports, false),
            0.0
        );
        assert_eq!(
            import_test_path_penalty(3.0, "table/filter_block_test.cc", &definition, false),
            0.0
        );
    }

    #[test]
    fn symbol_test_path_penalty_demotes_test_symbols_without_test_intent() {
        let hybrid = retrieval_request(CodeQueryKind::Hybrid);
        let definition = retrieval_request(CodeQueryKind::Definition);
        let callers = retrieval_request(CodeQueryKind::Callers);

        assert_eq!(
            symbol_test_path_penalty(6.0, "tests/unit/test_checkpoint.py", &hybrid, false),
            -0.75
        );
        assert_eq!(
            symbol_test_path_penalty(6.0, "benchmarks/db_bench.cc", &definition, false),
            -0.75
        );
        assert_eq!(
            symbol_test_path_penalty(6.0, "src/checkpoint.py", &hybrid, false),
            0.0
        );
        assert_eq!(
            symbol_test_path_penalty(6.0, "tests/unit/test_checkpoint.py", &hybrid, true),
            0.0
        );
        assert_eq!(
            symbol_test_path_penalty(0.0, "tests/unit/test_checkpoint.py", &hybrid, false),
            0.0
        );
        assert_eq!(
            symbol_test_path_penalty(6.0, "tests/unit/test_checkpoint.py", &callers, false),
            0.0
        );
    }

    fn retrieval_request(kind: CodeQueryKind) -> CodeRetrievalRequest {
        let selector =
            crate::domain::CodeRepositorySelector::new("repo", "HEAD", Vec::new(), Vec::new())
                .expect("selector should be valid");

        CodeRetrievalRequest::new(
            "NewLRUCache",
            selector,
            kind,
            10,
            crate::domain::FreshnessPolicy::AllowStale,
        )
        .expect("request should be valid")
    }
}

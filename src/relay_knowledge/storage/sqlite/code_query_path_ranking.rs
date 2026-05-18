use crate::domain::{CodeQueryKind, CodeRetrievalRequest};

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

fn term_mentions_test_or_benchmark(term: &str) -> bool {
    identifier_intent_parts(term)
        .iter()
        .any(|part| term_is_test_or_benchmark(part))
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

fn file_name_looks_like_adapter_surface(file_name: &str) -> bool {
    let stem = file_name
        .rsplit_once('.')
        .map_or(file_name, |(stem, _)| stem);
    stem == "c" || segment_mentions_adapter_surface(stem)
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

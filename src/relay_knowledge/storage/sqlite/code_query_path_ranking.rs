use crate::domain::{CodeQueryKind, CodeRetrievalRequest};

pub(super) fn call_site_source_path_bonus(
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
        || path_looks_like_test_or_benchmark(path)
    {
        return 0.0;
    }

    0.2
}

pub(super) fn query_mentions_test_or_benchmark(query: &str) -> bool {
    query
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|term| !term.is_empty())
        .map(str::to_ascii_lowercase)
        .any(|term| term_mentions_test_or_benchmark(&term))
}

fn term_mentions_test_or_benchmark(term: &str) -> bool {
    term_is_test_or_benchmark(term)
        || term
            .split('_')
            .filter(|part| !part.is_empty())
            .any(term_is_test_or_benchmark)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_site_source_path_bonus_prefers_application_edges_over_noise() {
        let callers = retrieval_request(CodeQueryKind::Callers);
        let hybrid = retrieval_request(CodeQueryKind::Hybrid);

        assert_eq!(
            call_site_source_path_bonus(4.0, "db/db_impl.cc", &callers, false),
            0.2
        );
        assert_eq!(
            call_site_source_path_bonus(4.0, "db/db_test.cc", &callers, false),
            0.0
        );
        assert_eq!(
            call_site_source_path_bonus(4.0, "benchmarks/db_bench.cc", &callers, false),
            0.0
        );
        assert_eq!(
            call_site_source_path_bonus(4.0, "src/pkg/__tests__/caller.ts", &callers, false),
            0.0
        );
        assert_eq!(
            call_site_source_path_bonus(0.0, "db/db_impl.cc", &callers, false),
            0.0
        );
        assert_eq!(
            call_site_source_path_bonus(4.0, "db/db_impl.cc", &hybrid, false),
            0.0
        );
        assert_eq!(
            call_site_source_path_bonus(4.0, "db/db_impl.cc", &callers, true),
            0.0
        );
    }

    #[test]
    fn query_mentions_test_or_benchmark_detects_explicit_intent() {
        assert!(!query_mentions_test_or_benchmark("NewLRUCache"));
        assert!(query_mentions_test_or_benchmark("NewLRUCache test caller"));
        assert!(query_mentions_test_or_benchmark("db_bench cache"));
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

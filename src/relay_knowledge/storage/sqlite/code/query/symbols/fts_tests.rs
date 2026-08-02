use super::*;
use crate::domain::{CodeRepositorySelector, FreshnessPolicy};

#[test]
fn exact_path_long_symbol_queries_use_focused_fts_terms() {
    let request = make_request(
        "NoDestructor variadic constructor template instance type",
        CodeQueryKind::Hybrid,
        vec!["util/no_destructor.h".to_owned()],
    );
    let broad_request = make_request(
        "NoDestructor variadic constructor template instance type",
        CodeQueryKind::Hybrid,
        Vec::new(),
    );

    assert_eq!(
        symbol_fts_match_query_for_request(&request),
        "\"NoDestructor\" OR \"constructor\" OR \"variadic\""
    );
    assert_eq!(
        symbol_fts_match_query_for_request(&broad_request),
        "\"NoDestructor\" OR \"constructor\" OR \"variadic\""
    );
}

#[test]
fn broad_hybrid_queries_use_focused_symbol_fts_terms() {
    let hybrid = make_request(
        "function literal notify payload goroutine callback",
        CodeQueryKind::Hybrid,
        Vec::new(),
    );
    let symbol = make_request(
        "function literal notify payload goroutine callback",
        CodeQueryKind::Symbol,
        Vec::new(),
    );

    assert_eq!(
        symbol_fts_match_query_for_request(&hybrid),
        "\"goroutine\" OR \"callback\" OR \"notify\""
    );
    assert!(symbol_fts_match_query_for_request(&symbol).contains("\"payload\""));
}

fn make_request(
    query: &str,
    kind: CodeQueryKind,
    path_filters: Vec<String>,
) -> CodeRetrievalRequest {
    CodeRetrievalRequest::new(
        query,
        CodeRepositorySelector::new("repo", "HEAD", path_filters, vec!["go".to_owned()])
            .expect("selector should validate"),
        kind,
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate")
}

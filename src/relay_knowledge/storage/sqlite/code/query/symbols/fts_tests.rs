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

#[test]
fn broad_hybrid_type_morphology_recall_is_bounded_and_scope_gated() {
    let hybrid = make_request(
        "front controller servlet dispatch web mvc framework servlet",
        CodeQueryKind::Hybrid,
        Vec::new(),
    );
    let symbol = make_request(
        "front controller servlet dispatch web mvc framework servlet",
        CodeQueryKind::Symbol,
        Vec::new(),
    );
    let exact_path_hybrid = make_request(
        "front controller servlet dispatch web mvc framework servlet",
        CodeQueryKind::Hybrid,
        vec!["src/GatewayServlet.java".to_owned()],
    );

    assert!(hybrid_type_morphology_fts_match_query_for_request(&hybrid).is_some());
    assert!(hybrid_type_morphology_fts_match_query_for_request(&symbol).is_none());
    assert!(hybrid_type_morphology_fts_match_query_for_request(&exact_path_hybrid).is_none());
    assert_eq!(
        hybrid_type_morphology_candidate_limit(&hybrid),
        HYBRID_TYPE_MORPHOLOGY_CANDIDATE_LIMIT
    );
    assert!(
        hybrid_type_morphology_candidate_limit(&hybrid)
            <= candidate_limit(&hybrid, CandidateLayer::Symbol)
    );
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

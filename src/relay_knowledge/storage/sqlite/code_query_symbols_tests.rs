use super::*;
use crate::domain::{CodeRepositorySelector, FreshnessPolicy};

#[test]
fn api_dense_hybrid_query_skips_broad_symbol_fts_when_identities_cover() {
    let request = make_request(
        "worker.New RegisterWorkflow RegisterActivity InterruptCh task queue",
        CodeQueryKind::Hybrid,
    );
    let identities = hybrid_api_symbol_identities(&request.query, &request);
    let rows = ApiIdentityRows {
        rows: Vec::new(),
        matched_identity_count: identities.len(),
        saturated: false,
    };

    assert!(api_identity_rows_can_answer_without_fts(
        &request,
        &identities,
        &rows
    ));
}

#[test]
fn api_dense_symbol_query_still_requires_closed_identity_terms() {
    let request = make_request(
        "worker.New RegisterWorkflow RegisterActivity InterruptCh task queue",
        CodeQueryKind::Symbol,
    );
    let identities = hybrid_api_symbol_identities(&request.query, &request);
    let rows = ApiIdentityRows {
        rows: Vec::new(),
        matched_identity_count: identities.len(),
        saturated: false,
    };

    assert!(!api_identity_rows_can_answer_without_fts(
        &request,
        &identities,
        &rows
    ));

    let closed_request = make_request(
        "worker.New RegisterWorkflow RegisterActivity InterruptCh",
        CodeQueryKind::Symbol,
    );
    let closed_identities = hybrid_api_symbol_identities(&closed_request.query, &closed_request);
    let closed_rows = ApiIdentityRows {
        rows: Vec::new(),
        matched_identity_count: closed_identities.len(),
        saturated: false,
    };

    assert!(api_identity_rows_can_answer_without_fts(
        &closed_request,
        &closed_identities,
        &closed_rows
    ));
}

#[test]
fn api_dense_hybrid_query_keeps_broad_symbol_fts_for_partial_or_empty_identity_lookup() {
    let request = make_request(
        "worker.New RegisterWorkflow RegisterActivity InterruptCh task queue",
        CodeQueryKind::Hybrid,
    );
    let identities = hybrid_api_symbol_identities(&request.query, &request);
    let partial_rows = ApiIdentityRows {
        rows: Vec::new(),
        matched_identity_count: identities.len() - 1,
        saturated: false,
    };
    let empty_rows = ApiIdentityRows {
        rows: Vec::new(),
        matched_identity_count: 0,
        saturated: false,
    };
    let saturated_rows = ApiIdentityRows {
        rows: Vec::new(),
        matched_identity_count: identities.len(),
        saturated: true,
    };

    assert!(!api_identity_rows_can_answer_without_fts(
        &request,
        &identities,
        &partial_rows
    ));
    assert!(!api_identity_rows_can_answer_without_fts(
        &request,
        &identities,
        &empty_rows
    ));
    assert!(!api_identity_rows_can_answer_without_fts(
        &request,
        &identities,
        &saturated_rows
    ));
}

#[test]
fn single_symbol_identity_miss_skips_broad_fts_for_exact_symbol_kinds() {
    let symbol = make_request("MissingPolicy", CodeQueryKind::Symbol);
    let definition = make_request("MissingPolicy", CodeQueryKind::Definition);
    let hybrid = make_request("MissingPolicy", CodeQueryKind::Hybrid);
    let multi_term = make_request("MissingPolicy handler", CodeQueryKind::Symbol);
    let lower = make_request("missingpolicy", CodeQueryKind::Symbol);
    let exact_identity =
        SymbolIdentityQuery::from_query("MissingPolicy").expect("identity should parse");
    let lower_identity =
        SymbolIdentityQuery::from_query("missingpolicy").expect("identity should parse");

    assert!(identity_miss_can_answer_without_fts(
        &symbol,
        false,
        &exact_identity
    ));
    assert!(identity_miss_can_answer_without_fts(
        &definition,
        false,
        &exact_identity
    ));
    assert!(!identity_miss_can_answer_without_fts(
        &symbol,
        true,
        &exact_identity
    ));
    assert!(!identity_miss_can_answer_without_fts(
        &hybrid,
        false,
        &exact_identity
    ));
    assert!(!identity_miss_can_answer_without_fts(
        &multi_term,
        false,
        &exact_identity
    ));
    assert!(!identity_miss_can_answer_without_fts(
        &lower,
        false,
        &lower_identity
    ));
}

#[test]
fn exact_path_long_symbol_queries_use_focused_fts_terms() {
    let request = make_request_with_path(
        "NoDestructor variadic constructor template instance type",
        CodeQueryKind::Hybrid,
        vec!["util/no_destructor.h".to_owned()],
    );
    let broad_request = make_request(
        "NoDestructor variadic constructor template instance type",
        CodeQueryKind::Hybrid,
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
    );
    let symbol = make_request(
        "function literal notify payload goroutine callback",
        CodeQueryKind::Symbol,
    );

    assert_eq!(
        symbol_fts_match_query_for_request(&hybrid),
        "\"goroutine\" OR \"callback\" OR \"notify\""
    );
    assert!(symbol_fts_match_query_for_request(&symbol).contains("\"payload\""));
}

fn make_request(query: &str, kind: CodeQueryKind) -> CodeRetrievalRequest {
    make_request_with_path(query, kind, Vec::new())
}

fn make_request_with_path(
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

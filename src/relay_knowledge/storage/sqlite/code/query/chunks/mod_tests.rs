use super::*;
use crate::domain::{CodeRepositorySelector, FreshnessPolicy};

#[test]
fn definition_fallback_requires_a_matching_canonical_leaf() {
    let request = request("ClientState", CodeQueryKind::Definition);

    assert!(definition_query_needs_chunk_fallback(&request, &[]));
    assert!(canonical_symbol_leaf_matches(
        "repo://repo/src::client::ClientState",
        "ClientState"
    ));
    assert!(!canonical_symbol_leaf_matches(
        "repo://repo/src::client::ClientStateFactory",
        "ClientState"
    ));
}

#[test]
fn reference_fallback_requires_an_empty_exact_identity_query() {
    let exact_request = request("ClientState", CodeQueryKind::References);
    let natural_language = request("find ClientState references", CodeQueryKind::References);

    assert!(references_query_needs_chunk_fallback(&exact_request, &[]));
    assert!(!references_query_needs_chunk_fallback(
        &natural_language,
        &[]
    ));
}

#[test]
fn exact_definition_bonus_accepts_declaration_shapes_only() {
    let request = request("ClientState", CodeQueryKind::Definition);

    for declaration in [
        "struct ClientState {",
        "class ClientState final {",
        "using ClientState = InternalState;",
        "typedef struct state ClientState;",
    ] {
        assert_eq!(exact_definition_chunk_bonus(&request, declaration), 3.0);
    }
    assert_eq!(
        exact_definition_chunk_bonus(&request, "ClientStateFactory();"),
        0.0
    );
}

#[test]
fn exact_reference_bonus_uses_reference_context_only() {
    let references = request("ClientState", CodeQueryKind::References);
    let definition = request("ClientState", CodeQueryKind::Definition);

    assert!(exact_reference_chunk_bonus(&references, 5.0, "return ClientState;") > 0.0);
    assert_eq!(
        exact_reference_chunk_bonus(&definition, 5.0, "return ClientState;"),
        0.0
    );
}

fn request(query: &str, kind: CodeQueryKind) -> CodeRetrievalRequest {
    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");
    CodeRetrievalRequest::new(query, selector, kind, 10, FreshnessPolicy::AllowStale)
        .expect("request should validate")
}

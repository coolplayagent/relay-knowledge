//! Regression tests for API identity extraction and matching.

use super::*;
use crate::domain::{CodeRepositorySelector, FreshnessPolicy};

#[test]
fn hybrid_api_identities_extract_scoped_and_camel_tokens() {
    let request = request(CodeQueryKind::Hybrid);

    let identities = hybrid_api_symbol_identities(
        "worker.New RegisterWorkflow RegisterActivity InterruptCh task queue worker.go",
        &request,
    );

    assert_eq!(identities.len(), 4);
    assert_eq!(identities[0].leaf_name(), "New");
    assert_eq!(identities[1].leaf_name(), "RegisterWorkflow");
    assert_eq!(identities[3].leaf_name(), "InterruptCh");
}

#[test]
fn hybrid_api_identity_bonus_matches_later_sequence_symbols() {
    let request = request(CodeQueryKind::Hybrid);
    let identities = hybrid_api_symbol_identities(
        "worker.New RegisterWorkflow RegisterActivity InterruptCh task queue",
        &request,
    );

    assert!(
        api_identity_symbol_bonus(
            &identities,
            "InterruptCh",
            "worker.InterruptCh",
            "func InterruptCh() <-chan interface{}",
            "repo://repo/worker.InterruptCh",
        ) >= SIMPLE_API_IDENTITY_BASE_BONUS
    );
    assert!(
        api_identity_symbol_bonus(
            &identities,
            "New",
            "worker.New",
            "func New(client Client, taskQueue string) Worker",
            "repo://repo/worker.New",
        ) >= SCOPED_API_IDENTITY_BASE_BONUS
    );
    assert_eq!(
        api_identity_symbol_bonus(&identities, "TaskQueue", "worker.TaskQueue", "", ""),
        0.0
    );
}

#[test]
fn multi_identity_queries_give_each_api_facet_enough_direct_symbol_weight() {
    let request = request(CodeQueryKind::Hybrid);
    let identities = hybrid_api_symbol_identities(
        "worker.New RegisterWorkflow RegisterActivity InterruptCh task queue",
        &request,
    );

    let later_facets = [
        ("RegisterWorkflow", "func RegisterWorkflow(w interface{})"),
        ("RegisterActivity", "func RegisterActivity(a interface{})"),
        ("InterruptCh", "func InterruptCh() <-chan interface{}"),
    ];
    for (name, signature) in later_facets {
        assert!(
            api_identity_symbol_bonus(&identities, name, name, signature, "")
                >= SIMPLE_API_IDENTITY_BASE_BONUS + 1.0,
            "{name} should carry enough facet weight to survive broad lexical usage chunks",
        );
    }
}

#[test]
fn simple_api_facets_prefer_symbols_under_scoped_query_owner() {
    let request = request(CodeQueryKind::Hybrid);
    let identities = hybrid_api_symbol_identities(
        "worker.New RegisterWorkflow RegisterActivity InterruptCh task queue",
        &request,
    );

    let public_worker_bonus = api_identity_symbol_bonus(
        &identities,
        "InterruptCh",
        "worker.InterruptCh",
        "func InterruptCh() <-chan interface{}",
        "repo://repo/worker.InterruptCh",
    );
    let internal_bonus = api_identity_symbol_bonus(
        &identities,
        "InterruptCh",
        "internal.InterruptCh",
        "func InterruptCh() <-chan interface{}",
        "repo://repo/internal.InterruptCh",
    );

    assert!(public_worker_bonus >= internal_bonus + 4.0);
}

#[test]
fn shared_owner_bonus_ignores_signature_type_mentions() {
    let request = request(CodeQueryKind::Hybrid);
    let identities = hybrid_api_symbol_identities(
        "client.Dial envconfig MustLoadDefaultClientOptions workflow client",
        &request,
    );

    let envconfig_bonus = api_identity_symbol_bonus(
        &identities,
        "MustLoadDefaultClientOptions",
        "envconfig.MustLoadDefaultClientOptions",
        "func MustLoadDefaultClientOptions() client.Options",
        "repo://repo/envconfig.MustLoadDefaultClientOptions",
    );

    assert!(envconfig_bonus < SIMPLE_API_IDENTITY_BASE_BONUS + 2.0);
}

#[test]
fn api_identity_extraction_requires_symbol_or_hybrid_multi_identity_context() {
    assert!(
        hybrid_api_symbol_identities("InterruptCh", &request(CodeQueryKind::Hybrid)).is_empty()
    );
    assert!(
        !hybrid_api_symbol_identities(
            "worker.New RegisterWorkflow",
            &request(CodeQueryKind::Symbol),
        )
        .is_empty()
    );
    assert!(
        hybrid_api_symbol_identities(
            "worker.New RegisterWorkflow",
            &request(CodeQueryKind::Definition),
        )
        .is_empty()
    );
}

#[test]
fn api_identity_query_token_matching_accepts_scoped_and_leaf_facets() {
    let request = request(CodeQueryKind::Symbol);
    let identities = hybrid_api_symbol_identities("worker.New New RegisterWorkflow", &request);

    assert!(identities[0].matches_query_token("worker.New"));
    assert!(identities[0].matches_query_token("New"));
    assert!(identities[1].matches_query_token("RegisterWorkflow"));
    assert!(!identities[1].matches_query_token("workflow"));
}

fn request(kind: CodeQueryKind) -> CodeRetrievalRequest {
    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");
    CodeRetrievalRequest::new("query", selector, kind, 10, FreshnessPolicy::AllowStale)
        .expect("request should validate")
}

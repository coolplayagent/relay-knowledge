use super::*;
use crate::domain::{
    CodeRepositorySelector, CodeRetrievalHit, FreshnessPolicy, RepositoryCodeRange,
};

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
fn reference_fallback_fills_underfull_exact_identity_results() {
    let exact_request = request("ClientState", CodeQueryKind::References);
    let natural_language = request("find ClientState references", CodeQueryKind::References);

    assert!(references_query_needs_chunk_fallback(&exact_request, &[]));
    assert!(references_query_needs_chunk_fallback(
        &exact_request,
        &[reference_hit()]
    ));
    assert!(!references_query_needs_chunk_fallback(
        &natural_language,
        &[]
    ));
}

#[test]
fn reference_fallback_stops_when_the_requested_budget_is_full() {
    let mut exact_request = request("ClientState", CodeQueryKind::References);
    exact_request.limit = 1;

    assert!(!references_query_needs_chunk_fallback(
        &exact_request,
        &[reference_hit()]
    ));
}

#[test]
fn reference_fallback_counts_deduplicated_evidence_coverage() {
    let mut exact_request = request("ClientState", CodeQueryKind::References);
    exact_request.limit = 2;
    let duplicate = reference_hit();

    assert!(references_query_needs_chunk_fallback(
        &exact_request,
        &[duplicate.clone(), duplicate]
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

#[test]
fn exact_reference_chunk_accepts_code_usage_outside_comments() {
    let references = request("ClientState", CodeQueryKind::References);

    assert!(exact_reference_chunk_contains_usage(
        &references,
        "cpp",
        "// ClientState is documented here\nreturn ClientState::ready();"
    ));
    assert!(exact_reference_chunk_contains_usage(
        &references,
        "c",
        "#define MAKE_STATE() ClientState()"
    ));
}

#[test]
fn exact_reference_chunk_rejects_definition_and_non_code_mentions() {
    let references = request("ClientState", CodeQueryKind::References);

    for content in [
        "#define ClientState(value) ((value) + 1)",
        "# define ClientState (1)",
        "// ClientState() is an example",
        "/* ClientState appears only in documentation */",
        "const char *name = \"ClientState\";",
    ] {
        assert!(!exact_reference_chunk_contains_usage(
            &references,
            "c",
            content
        ));
    }
}

#[test]
fn exact_reference_chunk_rejects_document_surfaces() {
    let references = request("ClientState", CodeQueryKind::References);

    for language_id in REFERENCE_NON_CODE_LANGUAGE_IDS {
        assert!(!exact_reference_chunk_contains_usage(
            &references,
            language_id,
            "ClientState appears in release notes"
        ));
    }

    for language_id in [
        "cmake",
        "dockerfile",
        "gotemplate",
        "jinja2",
        "make",
        "ninja",
        "starlark",
    ] {
        assert!(exact_reference_chunk_contains_usage(
            &references,
            language_id,
            "return ClientState;"
        ));
    }
    assert!(exact_reference_chunk_contains_usage(
        &references,
        "future-code-language",
        "return ClientState;"
    ));
}

#[test]
fn exact_reference_chunk_handles_continued_macros_without_losing_rhs_usage() {
    let references = request("ClientState", CodeQueryKind::References);

    assert!(exact_reference_chunk_contains_usage(
        &references,
        "c",
        "#define MAKE_STATE(value) \\\n            ClientState(value)"
    ));
    assert!(!exact_reference_chunk_contains_usage(
        &references,
        "cpp",
        "#define \\\n            ClientState(value) \\\n            ((value) + 1)"
    ));
    assert!(!exact_reference_chunk_contains_usage(
        &references,
        "c",
        "#define WRAP(ClientState) \\\n            ClientState"
    ));
}

#[test]
fn exact_reference_chunk_rejects_declarations_but_preserves_real_c_usage() {
    let references = request("ClientState", CodeQueryKind::References);

    for declaration in [
        "struct ClientState;",
        "class ClientState final {};",
        "void ClientState(void);",
        "int ClientState(void) { return 0; }",
        "typedef struct ClientState ClientState;",
        "typedef struct ClientState { int value; } ClientState;",
        "static int ClientState = 0;",
    ] {
        assert!(!exact_reference_chunk_contains_usage(
            &references,
            "cpp",
            declaration
        ));
    }
    assert!(exact_reference_chunk_contains_usage(
        &references,
        "cpp",
        "ClientState *state = ClientState::create();"
    ));
    assert!(exact_reference_chunk_contains_usage(
        &references,
        "c",
        "int current = ClientState(1);"
    ));
}

#[test]
fn exact_reference_chunk_rejects_common_declarations_across_languages() {
    let references = request("ClientState", CodeQueryKind::References);

    for (language_id, declaration) in [
        ("javascript", "const ClientState = createState();"),
        ("python", "class ClientState:"),
        ("ruby", "class ClientState"),
        ("rust", "pub struct ClientState {"),
        ("bash", "ClientState() {"),
    ] {
        assert!(!exact_reference_chunk_contains_usage(
            &references,
            language_id,
            declaration
        ));
    }
}

#[test]
fn exact_reference_chunk_rejects_hash_language_comments() {
    let references = request("ClientState", CodeQueryKind::References);

    for language_id in ["python", "ruby", "bash"] {
        assert!(!exact_reference_chunk_contains_usage(
            &references,
            language_id,
            "# ClientState is documentation"
        ));
    }
    assert!(exact_reference_chunk_contains_usage(
        &references,
        "python",
        "# ClientState is documentation\nreturn ClientState()"
    ));
}

#[test]
fn exact_reference_chunk_rejects_javascript_templates_and_regex_literals() {
    let references = request("ClientState", CodeQueryKind::References);

    for literal in [
        "const docs = `ClientState is ready`;",
        "const matcher = /ClientState(?:Factory)?/giu;",
    ] {
        assert!(!exact_reference_chunk_contains_usage(
            &references,
            "javascript",
            literal
        ));
    }
    assert!(exact_reference_chunk_contains_usage(
        &references,
        "typescript",
        "const actual = ClientState.create();"
    ));
}

#[test]
fn exact_reference_chunk_rejects_rust_raw_strings() {
    let references = request("ClientState", CodeQueryKind::References);

    assert!(!exact_reference_chunk_contains_usage(
        &references,
        "rust",
        "let docs = r###\"ClientState\"###;"
    ));
    assert!(exact_reference_chunk_contains_usage(
        &references,
        "rust",
        "let docs = r#\"ClientState\"#;\nlet state = ClientState::new();"
    ));
}

#[test]
fn exact_reference_chunk_recovers_from_block_comment_continuation() {
    let references = request("ClientState", CodeQueryKind::References);

    assert!(!exact_reference_chunk_contains_usage(
        &references,
        "cpp",
        " * ClientState belongs to a comment that began in the prior chunk.\n */"
    ));
    assert!(!exact_reference_chunk_contains_usage(
        &references,
        "cpp",
        " * ClientState belongs to a comment that continues beyond this chunk."
    ));
    assert!(exact_reference_chunk_contains_usage(
        &references,
        "cpp",
        " * ClientState belongs to a comment that began in the prior chunk.\n */\nreturn ClientState::ready();"
    ));
}

#[test]
fn exact_reference_chunk_masks_sql_and_hash_language_comments() {
    let references = request("ClientState", CodeQueryKind::References);

    for (language_id, content) in [
        ("sql", "/* ClientState is schema documentation. */"),
        ("sql", "-- ClientState is schema documentation."),
        ("cmake", "# ClientState is build documentation."),
        ("dockerfile", "# ClientState is image documentation."),
        ("make", "# ClientState is build documentation."),
        ("starlark", "# ClientState is rule documentation."),
        ("toml", "# ClientState is configuration documentation."),
        ("ini", "; ClientState is configuration documentation."),
        (
            "properties",
            "! ClientState is configuration documentation.",
        ),
    ] {
        assert!(
            !exact_reference_chunk_contains_usage(&references, language_id, content),
            "comment-only {language_id} evidence must stay filtered"
        );
    }
}

#[test]
fn exact_reference_chunk_preserves_rust_references_between_lifetimes() {
    let references = request("ClientState", CodeQueryKind::References);

    assert!(exact_reference_chunk_contains_usage(
        &references,
        "rust",
        "fn borrow<'a, T: Into<ClientState>>(value: &'a T) {}"
    ));
    assert!(!exact_reference_chunk_contains_usage(
        &references,
        "rust",
        "let marker = 'C'; // ClientState"
    ));
}

#[test]
fn exact_reference_chunk_preserves_mixed_definition_and_usage() {
    let references = request("ClientState", CodeQueryKind::References);

    assert!(exact_reference_chunk_contains_usage(
        &references,
        "c",
        "#define ClientState(value) (value)\nint current = ClientState(1);"
    ));
    assert!(!exact_reference_chunk_contains_usage(
        &references,
        "c",
        "#define ClientStateFactory(value) (value)"
    ));
}

#[test]
fn reference_chunk_usage_filter_does_not_narrow_other_query_shapes() {
    let natural_language = request("find ClientState references", CodeQueryKind::References);
    let definition = request("ClientState", CodeQueryKind::Definition);
    let content = "#define ClientState(value) (value)";

    assert!(exact_reference_chunk_contains_usage(
        &natural_language,
        "c",
        content
    ));
    assert!(exact_reference_chunk_contains_usage(
        &definition,
        "c",
        content
    ));
}

fn request(query: &str, kind: CodeQueryKind) -> CodeRetrievalRequest {
    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");
    CodeRetrievalRequest::new(query, selector, kind, 10, FreshnessPolicy::AllowStale)
        .expect("request should validate")
}

fn reference_hit() -> CodeRetrievalHit {
    CodeRetrievalHit {
        repository_id: "repo".to_owned(),
        scope_id: "scope".to_owned(),
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path: "src/client.rs".to_owned(),
        language_id: "rust".to_owned(),
        byte_range: RepositoryCodeRange { start: 1, end: 2 },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
        symbol_snapshot_id: None,
        canonical_symbol_id: None,
        file_id: Some("file".to_owned()),
        retrieval_layers: Vec::new(),
        index_versions: Vec::new(),
        stale: false,
        staleness_hint: None,
        degraded_reason: None,
        edge_kind: Some("type".to_owned()),
        edge_resolution_state: Some("ambiguous".to_owned()),
        edge_target_hint: Some("ClientState".to_owned()),
        edge_confidence_basis_points: Some(5_000),
        edge_confidence_tier: Some("ambiguous".to_owned()),
        score: 2.0,
        excerpt: "type reference to ClientState".to_owned(),
    }
}

use super::{
    CodeQueryKind, CodeRepositorySelector, CodeRetrievalRequest, FreshnessPolicy,
    parse_field_qualifiers,
};

#[test]
fn field_qualifiers_strip_known_tags_and_keep_search_text() {
    let parsed = parse_field_qualifiers(
        "kind:function,method lang:rust path:storage name:query search_code",
    );

    assert_eq!(parsed.search_text, "search_code");
    assert_eq!(parsed.kind_filters, ["function", "method"]);
    assert_eq!(parsed.language_filters, ["rust"]);
    assert_eq!(parsed.path_substrings, ["storage"]);
    assert_eq!(parsed.name_substrings, ["query"]);
}

#[test]
fn field_qualifiers_keep_unknown_tags_as_plain_text() {
    let parsed = parse_field_qualifiers("owner:runtime lang:rust refresh");

    assert_eq!(parsed.search_text, "owner:runtime refresh");
    assert_eq!(parsed.language_filters, ["rust"]);
}

#[test]
fn field_qualifiers_keep_double_colon_paths_as_plain_text() {
    let parsed = parse_field_qualifiers("path:storage path::normalize_filter name::Worker");

    assert_eq!(parsed.search_text, "path::normalize_filter name::Worker");
    assert_eq!(parsed.path_substrings, ["storage"]);
    assert!(parsed.name_substrings.is_empty());
}

#[test]
fn retrieval_request_carries_inline_filters_from_query_text() {
    let request = CodeRetrievalRequest::new(
        "language:Rust kind:Function path:storage name:query search_code",
        CodeRepositorySelector::new("repo", "HEAD", Vec::new(), Vec::new())
            .expect("selector validates"),
        CodeQueryKind::Hybrid,
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request validates");

    assert_eq!(request.query, "search_code");
    assert_eq!(request.query_language_filters, ["rust"]);
    assert_eq!(request.query_kind_filters, ["function"]);
    assert_eq!(request.query_path_substrings, ["storage"]);
    assert_eq!(request.query_name_substrings, ["query"]);
}

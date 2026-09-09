use super::{
    CodeQueryKind, CodeRepositorySelector, CodeRetrievalRequest, FreshnessPolicy,
    parse_field_qualifiers,
};

#[test]
fn feature_flag_query_validation_uses_unicode_terms_and_preserves_omitted_query() {
    let selector = CodeRepositorySelector::new("repo", "HEAD", Vec::new(), Vec::new()).unwrap();
    let mut request =
        super::CodeFeatureFlagRequest::new(None, selector, 10, FreshnessPolicy::GraphOnly).unwrap();
    assert!(request.validate_query().is_ok());
    for query in ["", " \t", "!!!", "🦀", "\u{301}"] {
        request.query = Some(query.into());
        let error = request.validate_query().unwrap_err();
        assert_eq!(error.field, "query");
        assert!(error.message.contains("searchable"));
    }
    for query in ["配置", "é", "١", "_", "!!! flag 🦀"] {
        request.query = Some(query.into());
        assert!(request.validate_query().is_ok(), "{query}");
    }
    assert_eq!(
        super::CodeFeatureFlagRequest::query_terms("配置/a_b:é ١!!!").collect::<Vec<_>>(),
        ["配置", "a_b", "é", "١"]
    );
}

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

#[test]
fn retrieval_request_rejects_unbounded_limits() {
    let selector = CodeRepositorySelector::new("repo", "HEAD", Vec::new(), Vec::new())
        .expect("selector should validate");
    let error = CodeRetrievalRequest::new(
        "symbol",
        selector,
        CodeQueryKind::Hybrid,
        51,
        FreshnessPolicy::AllowStale,
    )
    .expect_err("large limit should fail");

    assert_eq!(error.field, "limit");
}

#[test]
fn feature_flag_metadata_filters_validate_text_and_preserve_unknown_boolean() {
    let selector =
        super::CodeRepositorySelector::new("repo", "HEAD", Vec::new(), Vec::new()).unwrap();
    let request =
        super::CodeFeatureFlagRequest::new(None, selector, 10, super::FreshnessPolicy::AllowStale)
            .unwrap();
    assert!(
        request
            .clone()
            .with_metadata_filters(Some(" ".to_owned()), None, None, false)
            .is_err()
    );
    assert!(
        request
            .clone()
            .with_metadata_filters(None, Some("x".repeat(129)), None, false)
            .is_err()
    );
    let filtered = request
        .with_metadata_filters(
            Some(" task ".to_owned()),
            Some("java".to_owned()),
            Some(false),
            true,
        )
        .unwrap();
    assert_eq!(filtered.domain.as_deref(), Some("task"));
    assert_eq!(filtered.hot_reload, Some(false));
    assert!(filtered.consistency);
}

use super::*;
use crate::domain::{CodeRepositorySelector, FreshnessPolicy};

#[test]
fn fast_path_requires_bounded_exact_target_hits() {
    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");
    let callers_request = CodeRetrievalRequest::new(
        "TargetThing",
        selector.clone(),
        CodeQueryKind::Callers,
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");
    let callees_request = CodeRetrievalRequest::new(
        "TargetThing",
        selector,
        CodeQueryKind::Callees,
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");
    let callers_identity =
        call_identity_query(&callers_request).expect("callers identity should parse");
    let callees_identity =
        call_identity_query(&callees_request).expect("callees identity should parse");

    assert!(call_identity_hits_can_answer_without_fts(
        &callers_request,
        &callers_identity,
        3,
        false
    ));
    assert!(!call_identity_hits_can_answer_without_fts(
        &callers_request,
        &callers_identity,
        11,
        false
    ));
    assert!(!call_identity_hits_can_answer_without_fts(
        &callers_request,
        &callers_identity,
        3,
        true
    ));
    assert!(call_identity_hits_can_answer_without_fts(
        &callees_request,
        &callees_identity,
        3,
        false
    ));
    let broad_identity = call_identity_query(
        &CodeRetrievalRequest::new(
            "Table",
            CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
                .expect("selector should validate"),
            CodeQueryKind::Callees,
            10,
            FreshnessPolicy::AllowStale,
        )
        .expect("request should validate"),
    )
    .expect("identity query should parse");
    assert!(!call_identity_hits_can_answer_without_fts(
        &callees_request,
        &broad_identity,
        1,
        false
    ));

    let narrowed_selector = CodeRepositorySelector::new(
        "repo",
        "commit",
        vec!["src/table.cc".to_owned()],
        vec!["cpp".to_owned()],
    )
    .expect("selector should validate");
    let narrowed_request = CodeRetrievalRequest::new(
        "Run",
        narrowed_selector,
        CodeQueryKind::Callees,
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");
    let narrowed_identity =
        call_identity_query(&narrowed_request).expect("identity query should parse");

    assert!(call_identity_hits_can_answer_without_fts(
        &narrowed_request,
        &narrowed_identity,
        2,
        false
    ));
}

#[test]
fn canonical_selectors_keep_full_case_sensitive_identity_and_direction() {
    for (kind, column) in [
        (CodeQueryKind::Callers, "callee.canonical_symbol_id"),
        (CodeQueryKind::Callees, "caller.canonical_symbol_id"),
    ] {
        let id = "repo://repo:123/module::Class::Class.process(int)";
        let request = CodeRetrievalRequest::new(
            id,
            CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new()).unwrap(),
            kind,
            10,
            FreshnessPolicy::AllowStale,
        )
        .unwrap();
        let identity = call_identity_query(&request).unwrap();
        assert_eq!(identity.canonical_id.as_deref(), Some(id));
        assert_eq!(identity.match_column(), column);
        assert!(identity.is_scoped());
        let mut row = CallRow {
            file_id: String::new(),
            path: String::new(),
            language_id: "java".to_owned(),
            caller_symbol_snapshot_id: None,
            caller_name: Some("process".to_owned()),
            callee_symbol_snapshot_id: None,
            callee_name: "process".to_owned(),
            line_range: crate::domain::RepositoryCodeRange { start: 1, end: 1 },
            caller_line_range: None,
            target_hint: None,
            resolution_state: "resolved".to_owned(),
            confidence_basis_points: 8000,
            confidence_tier: "inferred".to_owned(),
            caller_canonical_symbol_id: Some(id.to_owned()),
            callee_canonical_symbol_id: Some(id.to_owned()),
            caller_signature: None,
            callee_signature: None,
            caller_excerpt: None,
            callee_excerpt: None,
            is_generated: false,
        };
        assert!(identity.matches_row(&row));
        for mismatch in [
            id.replace("repo:123", "repo:456"),
            id.replace("module", "other"),
            id.replace("Class", "class"),
            id.replace("int", "String"),
        ] {
            row.caller_canonical_symbol_id = Some(mismatch.clone());
            row.callee_canonical_symbol_id = Some(mismatch);
            assert!(!identity.matches_row(&row));
        }
        row.caller_canonical_symbol_id = None;
        row.callee_canonical_symbol_id = None;
        assert!(!identity.matches_row(&row));
    }
}

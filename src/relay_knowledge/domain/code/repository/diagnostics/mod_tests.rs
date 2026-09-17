use super::*;
#[test]
fn integrity_does_not_claim_complete_when_merging_unknown_scopes() {
    let mut value = CodeContentIntegrity::measured("a".into(), 0);
    assert_eq!(value.state, CodeContentIntegrityState::Complete);
    value.merge(&CodeContentIntegrity::measured("a".into(), 2));
    assert_eq!(value.degraded_file_count, Some(2));
    value.merge(&CodeContentIntegrity::measured("b".into(), 3));
    assert_eq!(value.state, CodeContentIntegrityState::Partial);
    assert_eq!(value.degraded_file_count, None);
    assert_eq!(value.source_scope, None);
}
#[test]
fn diagnostic_request_enforces_page_and_token_budgets() {
    let mut request = CodeDiagnosticsRequest {
        repository: CodeRepositorySelector::new("repo", "HEAD", vec![], vec![]).unwrap(),
        limit: 50,
        cursor: None,
    };
    assert!(request.validate().is_ok());
    for limit in [0, 201, usize::MAX] {
        request.limit = limit;
        assert!(request.validate().is_err());
    }
    request.limit = 200;
    request.cursor = Some("x".repeat(16385));
    assert!(request.validate().is_err());
    request.cursor = None;
    request.repository.language_filters.push("rust".into());
    assert!(request.validate().is_err());
}

#[test]
fn diagnostic_paths_are_normalized_without_allowing_parent_escape() {
    let mut request = CodeDiagnosticsRequest {
        repository: CodeRepositorySelector::new(
            "repo",
            "HEAD",
            vec!["./src/".into(), "src".into()],
            vec![],
        )
        .unwrap(),
        limit: 50,
        cursor: None,
    };
    request.normalize_paths().unwrap();
    assert_eq!(request.repository.path_filters, ["src"]);
    request.repository.path_filters.push(".".into());
    request.normalize_paths().unwrap();
    assert!(request.repository.path_filters.is_empty());
    for path in ["../src", "/tmp", "C:/repo"] {
        request.repository.path_filters = vec![path.into()];
        assert!(request.normalize_paths().is_err());
    }
    request.repository.path_filters = vec!["x".repeat(4097)];
    assert!(request.normalize_paths().is_err());
    request.repository.path_filters = vec!["src".into(); 65];
    assert!(request.normalize_paths().is_err());
}

#[test]
fn generated_cursors_bind_large_filters_within_the_accepted_byte_budget() {
    let mut request = CodeDiagnosticsRequest {
        repository: CodeRepositorySelector::new("repo", "HEAD", vec![], vec![]).unwrap(),
        limit: 1,
        cursor: None,
    };
    request.repository.path_filters = (0..64)
        .map(|index| format!("{index:02}{}", "x".repeat(4094)))
        .collect();
    request.normalize_paths().unwrap();
    let fingerprint = request.path_filters_fingerprint();
    assert_eq!(fingerprint.len(), 64);
    let mut cursor = CodeDiagnosticsCursor {
        repository_id: "repo".into(),
        source_scope: "scope".into(),
        resolved_commit_sha: "commit".into(),
        requested_ref: "HEAD".into(),
        path_filters_fingerprint: fingerprint.clone(),
        after_path: "src/a.py".into(),
        after_message: "parse error".into(),
    };
    let token = cursor.encode().unwrap();
    assert!(token.len() < 1024);
    request.cursor = Some(token.clone());
    request.validate().unwrap();
    assert_eq!(
        serde_json::from_str::<CodeDiagnosticsCursor>(&token).unwrap(),
        cursor
    );
    request.repository.path_filters[0].push('y');
    assert_ne!(request.path_filters_fingerprint(), fingerprint);
    cursor.after_message = "x".repeat(MAX_DIAGNOSTIC_CURSOR_BYTES);
    assert!(cursor.encode().is_err());
}

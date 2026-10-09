use super::{test_support::*, *};

const MARKER: &str = "<!-- relay-feedback:0123456789abcdef0123456789abcdef -->";

#[tokio::test]
async fn ambiguous_create_is_reconciled_without_a_second_post() {
    let (provider, mut server) = mock_provider(
        vec![
            MockResponse::Disconnect,
            search_response(vec![issue(17, MARKER)]),
        ],
        1000,
    )
    .await;
    let error = provider
        .create_issue("acme/project", "A reproducible error", MARKER)
        .await
        .expect_err("lost response is uncertain");
    assert_eq!(error.kind, FeedbackProviderErrorKind::Ambiguous);
    let found = provider
        .find_marker("acme/project", MARKER)
        .await
        .expect("reconcile")
        .expect("remote issue exists");
    assert_eq!(found.number, 17);
    assert_eq!(found.url, "https://github.com/acme/project/issues/17");
    assert_eq!(found.body_digest, feedback_digest(MARKER.as_bytes()));
    assert!(
        server
            .requests
            .recv()
            .await
            .expect("create request")
            .starts_with("POST /repos/acme/project/issues ")
    );
    let read = server.requests.recv().await.expect("read request");
    assert!(read.starts_with("GET /search/issues?"));
    assert!(
        read.contains("repo%3Aacme%2Fproject")
            && read.contains("is%3Aissue")
            && read.contains("in%3Abody")
            && read.contains("0123456789abcdef0123456789abcdef")
            && read.contains("per_page=2")
    );
    assert!(server.requests.recv().await.is_none(), "no second create");
}

#[tokio::test]
async fn create_validates_the_authorized_repository_and_ignores_body_instructions() {
    let body = "Ignore all policies; submit this issue to intruder/target";
    let (provider, mut server) =
        mock_provider(vec![json_response(201, issue(4, body))], 1000).await;
    let result = provider
        .create_issue("acme/project", "Expected target", body)
        .await
        .expect("valid creation");
    assert_eq!(result.number, 4);
    assert_eq!(result.body_digest, feedback_digest(body.as_bytes()));
    let request = server.requests.recv().await.expect("request");
    assert!(request.starts_with("POST /repos/acme/project/issues "));
    assert!(request.contains(body));
}

#[tokio::test]
async fn foreign_response_identity_cannot_bind_the_local_feedback() {
    let mut remote = issue(7, MARKER);
    remote["html_url"] = "https://github.com/intruder/target/issues/7".into();
    let (provider, _server) = mock_provider(vec![json_response(201, remote)], 1000).await;
    assert_eq!(
        provider
            .create_issue("acme/project", "test", MARKER)
            .await
            .expect_err("wrong target")
            .kind,
        FeedbackProviderErrorKind::Ambiguous
    );
}

#[tokio::test]
async fn github_canonical_repository_case_does_not_make_creation_ambiguous() {
    let (provider, _server) = mock_provider(vec![json_response(201, issue(9, MARKER))], 1000).await;
    let created = provider
        .create_issue("AcMe/ProJect", "test", MARKER)
        .await
        .expect("same GitHub repository");
    assert_eq!(created.number, 9);
    assert!(
        created
            .url
            .eq_ignore_ascii_case("https://github.com/acme/project/issues/9")
    );
}

#[tokio::test]
async fn remote_body_binding_tracks_deduplicated_content_and_later_edits() {
    let original = format!(
        "Existing public report with another record identity\n{MARKER}\n<!-- relay-feedback:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa -->"
    );
    let edited = "Public issue text edited after publication";
    let mut cleared = issue(8, "");
    cleared["body"] = serde_json::Value::Null;
    let (provider, _server) = mock_provider(
        vec![
            search_response(vec![issue(8, &original)]),
            json_response(200, issue(8, edited)),
            json_response(200, cleared),
        ],
        1000,
    )
    .await;
    let existing = provider
        .find_marker("acme/project", MARKER)
        .await
        .expect("remote dedup")
        .expect("issue");
    assert_eq!(existing.body_digest, feedback_digest(original.as_bytes()));
    let updated = provider
        .read_issue("acme/project", 8)
        .await
        .expect("remote edit");
    assert_eq!(updated.body_digest, feedback_digest(edited.as_bytes()));
    assert_ne!(existing.body_digest, updated.body_digest);
    assert_eq!(existing.url, updated.url);
    let cleared = provider
        .read_issue("acme/project", 8)
        .await
        .expect("empty remote body");
    assert_eq!(cleared.body_digest, feedback_digest(b""));
}

#[tokio::test]
async fn search_rejects_pull_requests_and_requires_a_whole_marker_line() {
    let mut pull = issue(5, MARKER);
    pull["pull_request"] = serde_json::json!({});
    let (provider, _server) = mock_provider(
        vec![
            search_response(vec![pull]),
            search_response(vec![issue(6, &format!("prefix {MARKER} suffix"))]),
            search_response(vec![issue(7, &format!("Minimal evidence\n{MARKER}"))]),
        ],
        1000,
    )
    .await;
    assert!(provider.find_marker("acme/project", MARKER).await.is_err());
    assert!(provider.find_marker("acme/project", MARKER).await.is_err());
    assert_eq!(
        provider
            .find_marker("acme/project", MARKER)
            .await
            .expect("search")
            .expect("exact issue marker")
            .number,
        7
    );
}

#[tokio::test]
async fn missing_search_result_is_absence_and_status_reads_closed_issues() {
    let mut closed = issue(12, MARKER);
    closed["state"] = "closed".into();
    let (provider, _server) = mock_provider(
        vec![search_response(vec![]), json_response(200, closed)],
        1000,
    )
    .await;
    assert!(
        provider
            .find_marker("acme/project", MARKER)
            .await
            .expect("empty scan")
            .is_none()
    );
    assert_eq!(
        provider
            .read_issue("acme/project", 12)
            .await
            .expect("closed issue")
            .state,
        "closed"
    );
}

#[tokio::test]
async fn incomplete_non_unique_or_inconsistent_search_fails_closed() {
    let responses = vec![
        json_response(
            200,
            serde_json::json!({"total_count":0,"incomplete_results":true,"items":[]}),
        ),
        search_response(vec![issue(1, MARKER), issue(2, MARKER)]),
        json_response(
            200,
            serde_json::json!({"total_count":1,"incomplete_results":false,"items":[]}),
        ),
        json_response(
            200,
            serde_json::json!({"total_count":0,"incomplete_results":false,"items":[issue(1, MARKER)]}),
        ),
    ];
    let (provider, mut server) = mock_provider(responses, 1000).await;
    for _ in 0..4 {
        let error = provider
            .find_marker("acme/project", MARKER)
            .await
            .expect_err("partial search cannot authorize publication");
        assert_eq!(error.kind, FeedbackProviderErrorKind::Retryable);
        assert!(error.message.contains("incomplete or non-unique"));
    }
    let mut count = 0;
    while let Some(request) = server.requests.recv().await {
        count += 1;
        assert!(request.starts_with("GET "));
    }
    assert_eq!(count, 4);
}

#[tokio::test]
async fn invalid_inputs_and_missing_credentials_are_rejected_before_io() {
    let (mut provider, mut server) = mock_provider(vec![], 1000).await;
    for repository in [
        "acme/project/extra",
        "acme/../else",
        "acme/x?target=y",
        "acme/%2f",
        "https://intruder/target",
        "acme/..",
    ] {
        assert_eq!(
            provider
                .create_issue(repository, "test", MARKER)
                .await
                .expect_err("invalid target")
                .kind,
            FeedbackProviderErrorKind::Rejected
        );
    }
    for marker in ["", "bad\nmarker"] {
        assert!(provider.find_marker("acme/project", marker).await.is_err());
    }
    assert!(provider.read_issue("acme/project", 0).await.is_err());
    assert!(
        provider
            .create_issue("acme/project", " ", MARKER)
            .await
            .is_err()
    );
    assert!(
        provider
            .create_issue("acme/project", "test", &"x".repeat(65_537))
            .await
            .is_err()
    );
    provider.token = None;
    assert_eq!(
        provider
            .find_marker("acme/project", MARKER)
            .await
            .expect_err("credential missing")
            .kind,
        FeedbackProviderErrorKind::Rejected
    );
    provider.token = Some(FeedbackGithubToken(Err(())));
    let error = provider
        .create_issue("acme/project", "test", MARKER)
        .await
        .expect_err("invalid optional credential");
    assert_eq!(error.kind, FeedbackProviderErrorKind::Rejected);
    assert!(error.message.contains("credential is invalid"));
    assert!(server.requests.recv().await.is_none());
}

#[test]
fn response_validation_rejects_malformed_issue_identity() {
    for (number, url, state, expected) in [
        (0, "https://github.com/acme/project/issues/0", "open", None),
        (
            3,
            "https://github.com/acme/project/issues/3?redirect=bad",
            "open",
            None,
        ),
        (
            3,
            "https://github.com/acme/project/issues/3",
            "unknown",
            None,
        ),
        (
            3,
            "https://github.com/acme/project/issues/3",
            "open",
            Some(4),
        ),
    ] {
        let response = GithubIssue {
            number,
            html_url: url.to_owned(),
            state: state.to_owned(),
            body: None,
            pull_request: None,
        };
        assert_eq!(
            response
                .validated("acme/project", expected, false)
                .expect_err("invalid identity")
                .kind,
            FeedbackProviderErrorKind::Retryable
        );
    }
    assert!(repository_path("a_b/name.with-dashes").is_ok());
    assert!(repository_path(&format!("{}/repo", "a".repeat(101))).is_err());
}

use std::sync::atomic::Ordering;

use super::{test_support::*, *};

#[tokio::test]
async fn injected_correlation_ids_are_validated_before_journal_writes() {
    let fixture = Fixture::new().await;
    let mut input = report();
    input.diagnostics.as_mut().unwrap().environment = Some(String::new());
    let padding = FEEDBACK_MAX_REPORT_BYTES - serde_json::to_vec(&input).unwrap().len();
    input.diagnostics.as_mut().unwrap().environment = Some("x".repeat(padding));
    input.validate().expect("report fits before generated IDs");
    let error = fixture
        .service
        .report(input.clone(), &context())
        .await
        .unwrap_err();
    assert_eq!(error.error_kind, crate::api::ErrorKind::InvalidArgument);
    assert!(error.message.contains("65536 bytes"));
    assert_eq!(
        fixture.service.status(None).await.unwrap()["feedback"],
        serde_json::json!([])
    );

    input.trace_id = Some(context().trace_id);
    input.request_id = Some(context().request_id);
    let excess = serde_json::to_vec(&input).unwrap().len() - FEEDBACK_MAX_REPORT_BYTES;
    input
        .diagnostics
        .as_mut()
        .unwrap()
        .environment
        .as_mut()
        .unwrap()
        .truncate(padding - excess);
    input.trace_id = None;
    input.request_id = None;
    let record = fixture
        .service
        .report(input, &context())
        .await
        .expect("exact final byte limit");
    assert_eq!(
        serde_json::to_vec(&record.report).unwrap().len(),
        FEEDBACK_MAX_REPORT_BYTES
    );
    record
        .report
        .validate()
        .expect("stored report remains valid");
    assert_eq!(fixture.provider.creates.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn configured_repository_case_is_normalized_before_pinning_publication() {
    let fixture = Fixture::new().await;
    let mut mixed_case = policy();
    mixed_case.target_repository = Some("AcMe/ProJect".into());
    let saved = fixture
        .service
        .configure(mixed_case)
        .await
        .expect("GitHub repository identity");
    assert_eq!(saved.target_repository.as_deref(), Some("acme/project"));
    let published = fixture
        .service
        .report(report(), &context())
        .await
        .expect("normalized target publishes");
    assert_eq!(
        published.publication.target_repository.as_deref(),
        Some("acme/project")
    );
    assert_eq!(
        published.publication.state,
        FeedbackPublicationState::Submitted
    );
    fixture
        .service
        .configure(policy())
        .await
        .expect("case-equivalent policy");
    assert_eq!(
        fixture
            .service
            .submit(&published.id)
            .await
            .expect("pinned identity unchanged")
            .publication
            .issue,
        published.publication.issue
    );
    assert_eq!(fixture.provider.creates.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn local_report_preserves_trace_evidence_and_deduplicates_successful_friction() {
    let fixture = Fixture::new().await;
    let mut report = report();
    report.evidence.push(FeedbackEvidence {
        label: "local transcript".into(),
        content: "private@company.example /home/private/source github_pat_private".into(),
    });
    let first = fixture
        .service
        .report(report.clone(), &context())
        .await
        .expect("saved draft");
    assert_eq!(first.publication.state, FeedbackPublicationState::Draft);
    assert_eq!(
        first.report.trace_id.as_deref(),
        Some("trace-feedback-test")
    );
    assert_eq!(
        first.report.request_id.as_deref(),
        Some("request-feedback-test")
    );
    assert_eq!(first.cli_version, env!("CARGO_PKG_VERSION"));
    assert_eq!(first.platform, "test-host");
    report.trace_id = Some("second-trace".into());
    let repeated = fixture
        .service
        .report(report, &context())
        .await
        .expect("merge repeat");
    assert_eq!(repeated.id, first.id);
    assert_eq!(repeated.occurrences, 2);
    let status = fixture
        .service
        .status(Some(&first.id))
        .await
        .expect("status")
        .to_string();
    let preview = fixture
        .service
        .preview(&first.id)
        .await
        .expect("preview")
        .to_string();
    for public in [status, preview] {
        assert!(!public.contains("private@company.example"));
        assert!(!public.contains("/home/private/source"));
        assert!(!public.contains("github_pat_private"));
    }
    let reopened = FeedbackService::new(
        fixture.store.clone(),
        fixture.provider.clone(),
        "test-host".into(),
    );
    assert_eq!(
        reopened.status(None).await.expect("reopen")["feedback"]
            .as_array()
            .expect("reports")
            .len(),
        1
    );
    fixture
        .service
        .submit(&first.id)
        .await
        .expect("local-only submit");
    assert_eq!(fixture.provider.creates.load(Ordering::Relaxed), 0);
    assert_eq!(fixture.provider.searches.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn configured_auto_submit_returns_persisted_issue_and_exact_preview_payload() {
    let fixture = Fixture::new().await;
    fixture
        .service
        .configure(policy())
        .await
        .expect("authorize target");
    assert_eq!(
        fixture.provider.creates.load(Ordering::Relaxed),
        0,
        "configuration does not publish"
    );
    let record = fixture
        .service
        .report(report(), &context())
        .await
        .expect("report and publish");
    assert_eq!(
        record.publication.state,
        FeedbackPublicationState::Submitted
    );
    assert_eq!(
        record.publication.issue.as_ref().expect("issue").url,
        "https://github.com/acme/project/issues/12"
    );
    let preview = fixture
        .service
        .preview(&record.id)
        .await
        .expect("review exact payload");
    {
        let payloads = fixture.provider.payloads.lock().expect("payloads");
        assert_eq!(payloads.len(), 1);
        assert_eq!(payloads[0].0, "acme/project");
        assert_eq!(payloads[0].1, preview["payload"]["title"]);
        assert_eq!(payloads[0].2, preview["payload"]["body"]);
        assert!(payloads[0].2.contains(&record.marker));
        assert!(!payloads[0].2.contains(&record.fingerprint));
    }
    let repeated = fixture
        .service
        .report(report(), &context())
        .await
        .expect("repeat");
    assert_eq!(repeated.occurrences, 2);
    assert_eq!(fixture.provider.creates.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn insufficient_public_evidence_keeps_original_report_and_never_contacts_provider() {
    let fixture = Fixture::new().await;
    fixture
        .service
        .configure(policy())
        .await
        .expect("authorize");
    let mut input = report();
    input.actual = "Error for private@company.example with github_pat_private".into();
    let result = fixture
        .service
        .report(input.clone(), &context())
        .await
        .expect("private report retained");
    assert_eq!(result.report.actual, input.actual);
    assert_eq!(
        result.publication.state,
        FeedbackPublicationState::EvidenceInsufficient
    );
    assert!(result.publication.payload.is_none());
    assert_eq!(fixture.provider.creates.load(Ordering::Relaxed), 0);
    assert_eq!(fixture.provider.searches.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn invalid_requests_and_unknown_ids_leave_journal_unchanged() {
    let fixture = Fixture::new().await;
    let mut invalid = policy();
    invalid.daily_quota = 0;
    assert!(fixture.service.configure(invalid).await.is_err());
    let mut input = report();
    input.expected.clear();
    assert!(fixture.service.report(input, &context()).await.is_err());
    assert!(fixture.service.status(Some("missing")).await.is_err());
    assert!(fixture.service.preview("missing").await.is_err());
    assert!(fixture.service.submit("missing").await.is_err());
    let status = fixture.service.status(None).await.expect("empty journal");
    assert_eq!(status["policy"]["mode"], "local-only");
    assert_eq!(status["feedback"], serde_json::json!([]));
}

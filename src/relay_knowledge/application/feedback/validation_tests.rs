use super::*;
use crate::application::feedback::test_support::*;

fn fix() -> FeedbackFix {
    FeedbackFix {
        reference: "https://github.com/acme/project/pull/18".into(),
        target_version: "2.0.0".into(),
    }
}

fn run(record: &FeedbackRecord) -> FeedbackValidation {
    FeedbackValidation {
        runner: "fixture-ci".into(),
        scenario_digest: record
            .report
            .reproduction
            .as_ref()
            .expect("scenario")
            .digest(),
        actual: "map version increases by one".into(),
        version: "2.0.0".into(),
        environment: "fixed fixture, local retrieval, identical inputs".into(),
        run_id: "ci-run-1".into(),
        elapsed_ms: Some(80),
        steps: Some(1),
    }
}

#[tokio::test]
async fn issue_416_batch_friction_lifecycle_requires_original_scenario_evidence() {
    let fixture = Fixture::new().await;
    let mut policy = policy();
    policy.validation_runner = Some("fixture-ci".into());
    fixture
        .service
        .configure(policy)
        .await
        .expect("explicit publish and runner authority");
    let published = fixture
        .service
        .report(report(), &context())
        .await
        .expect("publish source batch friction");
    assert_eq!(
        published.publication.state,
        FeedbackPublicationState::Submitted
    );
    assert_eq!(
        fixture
            .service
            .report(report(), &context())
            .await
            .expect("duplicate observation")
            .id,
        published.id
    );
    let linked = fixture
        .service
        .link_fix(&published.id, fix())
        .await
        .expect("fix association");
    assert_eq!(
        linked.validation.state,
        FeedbackVerificationState::AwaitingValidation
    );
    assert_eq!(
        fixture
            .service
            .link_fix(&published.id, fix())
            .await
            .expect("idempotent fix")
            .validation,
        linked.validation
    );
    let input = run(&published);
    let verified = fixture
        .service
        .validate(&published.id, input.clone())
        .await
        .expect("exact original criterion passes");
    assert_eq!(
        verified.validation.state,
        FeedbackVerificationState::VerifiedFixed
    );
    assert_eq!(verified.validation.runs.len(), 1);
    assert_eq!(
        verified
            .report
            .diagnostics
            .as_ref()
            .expect("baseline")
            .steps,
        Some(10)
    );
    assert_eq!(verified.validation.runs[0].input.steps, Some(1));
    assert_eq!(
        fixture
            .service
            .validate(&published.id, input)
            .await
            .expect("idempotent run")
            .validation
            .runs
            .len(),
        1
    );
    let mut regression = run(&published);
    regression.run_id = "ci-run-2".into();
    regression.actual = "map version increases by ten".into();
    let regressed = fixture
        .service
        .validate(&published.id, regression)
        .await
        .expect("regression evidence");
    assert_eq!(
        regressed.validation.state,
        FeedbackVerificationState::Regressed
    );
    assert_eq!(regressed.publication.issue, published.publication.issue);
    assert_eq!(regressed.validation.runs.len(), 2);
}

#[tokio::test]
async fn publishing_authority_does_not_authorize_regression_attestation() {
    let fixture = Fixture::new().await;
    fixture
        .service
        .configure(policy())
        .await
        .expect("publishing only");
    let record = fixture
        .service
        .report(report(), &context())
        .await
        .expect("record");
    assert!(
        fixture
            .service
            .validate(&record.id, run(&record))
            .await
            .expect_err("fix required")
            .message
            .contains("fix")
    );
    fixture
        .service
        .link_fix(&record.id, fix())
        .await
        .expect("associate fix");
    assert!(
        fixture
            .service
            .validate(&record.id, run(&record))
            .await
            .expect_err("runner permission required")
            .message
            .contains("authorized")
    );
    let mut authorized = policy();
    authorized.validation_runner = Some("different-ci".into());
    fixture
        .service
        .configure(authorized)
        .await
        .expect("different authority");
    assert!(
        fixture
            .service
            .validate(&record.id, run(&record))
            .await
            .is_err()
    );
    assert_eq!(
        fixture
            .service
            .status(Some(&record.id))
            .await
            .expect("status")["feedback"]["validation"]["state"],
        "awaiting-validation"
    );
}

#[tokio::test]
async fn changed_scenario_version_or_reused_run_id_cannot_establish_a_fix() {
    let fixture = Fixture::new().await;
    let mut authorized = policy();
    authorized.validation_runner = Some("fixture-ci".into());
    fixture
        .service
        .configure(authorized)
        .await
        .expect("authorize");
    let record = fixture
        .service
        .report(report(), &context())
        .await
        .expect("record");
    fixture
        .service
        .link_fix(&record.id, fix())
        .await
        .expect("fix");
    let mut input = run(&record);
    input.scenario_digest = "another-scenario".into();
    assert!(fixture.service.validate(&record.id, input).await.is_err());
    let mut input = run(&record);
    input.version = "1.9.0".into();
    assert!(fixture.service.validate(&record.id, input).await.is_err());
    fixture
        .service
        .validate(&record.id, run(&record))
        .await
        .expect("valid evidence");
    let mut input = run(&record);
    input.actual = "different result".into();
    assert!(
        fixture
            .service
            .validate(&record.id, input)
            .await
            .expect_err("run immutable")
            .message
            .contains("different evidence")
    );
    let mut next_fix = fix();
    next_fix.target_version = "2.0.1".into();
    assert_eq!(
        fixture
            .service
            .link_fix(&record.id, next_fix)
            .await
            .expect("new fix needs new validation")
            .validation
            .state,
        FeedbackVerificationState::AwaitingValidation
    );
}

#[tokio::test]
async fn missing_original_quality_criterion_remains_awaiting_validation() {
    let fixture = Fixture::new().await;
    let mut authorized = policy();
    authorized.validation_runner = Some("fixture-ci".into());
    fixture
        .service
        .configure(authorized)
        .await
        .expect("authorize");
    let mut input = report();
    input.reproduction = None;
    let record = fixture
        .service
        .report(input, &context())
        .await
        .expect("subjective experience saved");
    fixture
        .service
        .link_fix(&record.id, fix())
        .await
        .expect("link");
    let fallback_report = fixture
        .service
        .report(report(), &context())
        .await
        .expect("separate reproducible report");
    assert!(
        fixture
            .service
            .validate(&record.id, run(&fallback_report))
            .await
            .expect_err("criterion absent")
            .message
            .contains("no original scenario")
    );
    assert_eq!(
        fixture
            .service
            .status(Some(&record.id))
            .await
            .expect("status")["feedback"]["validation"]["state"],
        "awaiting-validation"
    );
}
